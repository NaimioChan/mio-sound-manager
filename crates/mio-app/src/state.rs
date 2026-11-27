//! The processes the shell owns: the audio engine, a decode worker, the library.
//!
//! Three long-lived things, each with its own threading rule:
//!
//! * The **engine** starts once and keeps a WASAPI stream open for the life of the
//!   process (architecture section 4). Publishing is lock-free — `Engine::publish`
//!   takes `&self` — so any thread can start playback with one atomic store, and
//!   polling the playhead needs nothing but the [`PlaybackMonitor`].
//! * The **decode worker** is one thread behind a last-in-wins queue. Decoding a
//!   3-minute 96kHz file is 112ms of solid CPU; running that inside a command
//!   would stall the webview. The queue keeps only the newest request, so holding
//!   the down-arrow through a list decodes the row the user stopped on instead of
//!   every row passed over.
//! * The **library** is a SQLite handle plus a folder path, replaced when the user
//!   opens a different folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::Instant;

use mio_engine::waveform::DEFAULT_BUCKETS;
use mio_engine::{
    Backend, DecodedAudio, Decoder, Engine, EngineConfig, OutputFormat, PlaybackMonitor,
    PublishSource,
};

use crate::audio_config::{AudioConfig, AudioStore};
use crate::commands::{
    AppStatus, AudioDeviceDto, FolderNodeDto, LibraryStatus, MatchingEditRequest, PlaybackPosition,
    SearchRequest, SoundDto, WaveformDto,
};
use crate::error::{AppError, AppResult};
use crate::history::{EditKind, History};
use crate::library::Library;
use crate::session::{Session, SessionStore};

/// Length of one decode window for long files (>60 s), in seconds.
///
/// Architecture 3.3's budget: a stereo 96 kHz window is a few MB of PCM, and a
/// window this long decodes in a fraction of the time playback takes to
/// consume it, so the decode-ahead thread stays ahead of the render callback
/// even on a cold disk. The engine's queue is one window deep, so the resident
/// PCM for a 22-minute file is two windows plus the LRU accounting — never the
/// whole file.
const WINDOW_SECONDS: f64 = 15.0;

/// What the app learns about audio when it opens a device.
#[derive(Debug, Clone)]
pub struct EngineInfo {
    pub device_name: String,
    pub backend: Backend,
    pub format: OutputFormat,
    pub endpoint_frames: u32,
    pub engine_period_ms: f64,
}

/// A decode request handed to the worker.
///
/// The decoder travels with the job rather than living on the worker thread: the
/// device format can change at runtime (the user switches to an ASIO driver at
/// 48kHz, say), and a worker holding its own copy would keep resampling to the
/// format the app started with.
struct DecodeJob {
    path: String,
    decoder: Decoder,
    max_duration_s: Option<f64>,
    window: Option<mio_engine::DecodeWindow>,
    reply: Sender<AppResult<Arc<DecodedAudio>>>,
}

/// The loop switch, as both the engine and the remembered copy hold it.
///
/// `region` is in seconds and `None` means the whole clip. Kept next to the gain
/// for the same reason: a device switch rebuilds the engine, and a setting that
/// only lived inside the engine would be silently reset by it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct LoopSetting {
    enabled: bool,
    region: Option<(f64, f64)>,
}

/// The loop state the renderer reads and writes.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopState {
    pub enabled: bool,
    /// `[start, end)` in seconds, or `None` for the whole clip.
    pub region: Option<(f64, f64)>,
}

/// The live audio output, plus what to do when there is none.
///
/// Everything behind one mutex because switching devices has to be atomic: a
/// half-swapped state would hand `publish` the new engine's slots and the old
/// decoder's format, and every clip would come out at the wrong rate. The audio
/// callback never touches this — it only reads the atomics inside `Shared`.
struct AudioSlot {
    engine: Option<Engine>,
    monitor: Option<PlaybackMonitor>,
    error: Option<String>,
    info: Option<EngineInfo>,
    /// The device format the decoder converts to. Falls back to 48k/stereo/f32
    /// when no device opened, so the waveform scanner still has something to
    /// resample against.
    format: OutputFormat,
    backend: Backend,
    device_filter: Option<String>,
    asio_frames: Option<u32>,
}

#[derive(Debug, Clone)]
struct PlaybackError {
    path: String,
    position_s: f64,
}

fn retry_decode<T, E, F>(mut decode: F) -> std::result::Result<T, E>
where
    F: FnMut() -> std::result::Result<T, E>,
{
    match decode() {
        Ok(value) => Ok(value),
        Err(_) => decode(),
    }
}

impl AudioSlot {
    fn open(backend: Backend, device_filter: Option<String>, asio_frames: Option<u32>) -> Self {
        let opened = Engine::start(EngineConfig {
            backend,
            device_filter: device_filter.clone(),
            buffer_duration_hns: 0,
            asio_buffer_frames: asio_frames,
        });

        match opened {
            Ok(engine) => {
                let monitor = engine.monitor();
                let info = EngineInfo {
                    device_name: monitor.device_name().to_string(),
                    backend: monitor.backend(),
                    format: monitor.format(),
                    endpoint_frames: monitor.endpoint_frames(),
                    engine_period_ms: monitor.engine_period_ms(),
                };
                Self {
                    format: info.format,
                    engine: Some(engine),
                    monitor: Some(monitor),
                    error: None,
                    info: Some(info),
                    backend,
                    device_filter,
                    asio_frames,
                }
            }
            Err(e) => Self {
                engine: None,
                monitor: None,
                error: Some(e.to_string()),
                info: None,
                format: OutputFormat {
                    sample_rate: 48_000,
                    channels: 2,
                    sample: mio_engine::DeviceSample::F32,
                },
                backend,
                device_filter,
                asio_frames,
            },
        }
    }

    fn decoder(&self) -> Decoder {
        Decoder::new(self.format)
    }
}

/// One-slot mailbox with last-in-wins replacement.
///
/// A plain `sync_channel(1)` would either block the sender or fail when full; the
/// behaviour needed here is "the newest request replaces whatever is waiting",
/// which is a swap under a mutex plus a condvar wakeup. The mutex is only ever
/// held for the swap, never across a decode.
struct LatestSlot {
    inner: Mutex<Option<DecodeJob>>,
    ready: Condvar,
    closed: Mutex<bool>,
}

impl LatestSlot {
    fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            ready: Condvar::new(),
            closed: Mutex::new(false),
        }
    }

    /// Put `job` in the slot, replacing and cancelling whatever was there.
    fn submit(&self, job: DecodeJob) {
        let Ok(mut slot) = self.inner.lock() else {
            return;
        };
        if let Some(previous) = slot.replace(job) {
            let _ = previous
                .reply
                .send(Err(AppError::internal("superseded by a newer request")));
        }
        self.ready.notify_one();
    }

    /// Block until a job arrives, then take it.
    fn take(&self) -> Option<DecodeJob> {
        let mut slot = self.inner.lock().ok()?;
        loop {
            if let Some(job) = slot.take() {
                return Some(job);
            }
            if *self.closed.lock().ok()? {
                return None;
            }
            slot = self.ready.wait(slot).ok()?;
        }
    }

    fn close(&self) {
        if let Ok(mut closed) = self.closed.lock() {
            *closed = true;
        }
        self.ready.notify_all();
    }
}

/// The whole application state, managed by Tauri.
pub struct AppState {
    /// The render device, the format it needs, and the reason it is not there.
    /// One mutex because switching device swaps all of it at once.
    audio: Arc<Mutex<AudioSlot>>,

    jobs: Arc<LatestSlot>,
    worker: Mutex<Option<JoinHandle<()>>>,

    /// Every open library, keyed by root. Several at once is the point: a sample
    /// collection is usually split across packs, and switching folders should not
    /// mean reopening and reindexing.
    libraries: Mutex<LibrarySet>,

    /// The last decoded clip, so replaying the same sound is a publish rather
    /// than a decode. Keyed by path so a different sound never reuses it.
    last_clip: Arc<Mutex<Option<(String, Weak<DecodedAudio>)>>>,
    prefetch_cache: Arc<Mutex<PrefetchCache>>,
    prefetch_sender: SyncSender<(String, Decoder, u64)>,
    prefetch_worker: Mutex<Option<JoinHandle<()>>>,
    prefetch_closed: Arc<std::sync::atomic::AtomicBool>,
    prefetch_generation: Arc<AtomicU64>,
    failed_decodes: Arc<Mutex<HashMap<String, (u64, u128)>>>,
    playback_error: Arc<Mutex<Option<PlaybackError>>>,
    playback_error_seq: Arc<AtomicU64>,

    /// Publishes the render thread never reached (superseded inside one engine
    /// period). Diagnostic for the status bar.
    dropped_publishes: AtomicU64,

    /// Bumped on every play/stop so a decode that finishes late can be discarded
    /// instead of starting stale audio.
    play_epoch: Arc<AtomicU64>,

    /// The output gain, mirrored outside the audio lock.
    ///
    /// The engine holds the authoritative copy inside its `Shared` block, but that
    /// copy is only reachable while a device is open. Keeping the number here means
    /// a slider moved while the device is gone is remembered, and reading it for the
    /// UI never has to wait on the audio mutex.
    gain_bits: AtomicU32,

    /// The loop switch and region, mirrored outside the audio lock for the same
    /// reason as the gain: a device switch must not lose them.
    loop_state: Mutex<LoopSetting>,
    /// Lock-free copy used by a long-file window chain. The chain must observe
    /// loop toggles without taking the control mutex on its worker thread.
    streaming_loop: Arc<std::sync::atomic::AtomicBool>,

    /// Undo/redo for library edits. `None` in tests that do not want history.
    history: Mutex<History>,

    /// Where the open-library list is remembered between runs. `None` in tests,
    /// which must not touch the user's real session file.
    session: Option<SessionStore>,

    /// Where the chosen output device is remembered. Same file, same rules.
    audio_config: Option<AudioStore>,
}

/// The open libraries plus which one the window is showing.
///
/// Kept as one struct behind one lock so "add a library" and "switch to it" cannot
/// interleave into a state where the active root is not in the set.
#[derive(Default)]
pub struct LibrarySet {
    libs: Vec<Arc<Library>>,
    active: Option<PathBuf>,
}

impl LibrarySet {
    fn find(&self, root: &Path) -> Option<Arc<Library>> {
        self.libs.iter().find(|l| l.root() == root).cloned()
    }

    /// The active library, falling back to the first one when the active root is
    /// gone (a library removed while it was selected).
    fn active(&self) -> Option<Arc<Library>> {
        match &self.active {
            Some(root) => self.find(root).or_else(|| self.libs.first().cloned()),
            None => self.libs.first().cloned(),
        }
    }

    fn roots(&self) -> Vec<PathBuf> {
        self.libs.iter().map(|l| l.root().to_path_buf()).collect()
    }
}

impl AppState {
    /// Open the audio device once. Called from Tauri's `setup`.
    pub fn boot() -> Self {
        Self::with_device(None)
    }

    /// Boot against a named device. Used by tests and by the harness. `None` picks
    /// the system default, which on this machine is a virtual card (FxSound) whose
    /// latency numbers mean nothing — `AGENTS.md` says so for a reason.
    pub fn with_device(device_filter: Option<String>) -> Self {
        Self::with_audio(Backend::Wasapi, device_filter, None)
    }

    /// Boot against a specific backend and device.
    pub fn with_audio(
        backend: Backend,
        device_filter: Option<String>,
        asio_frames: Option<u32>,
    ) -> Self {
        let audio = AudioSlot::open(backend, device_filter, asio_frames);

        let jobs = Arc::new(LatestSlot::new());
        let failed_decodes = Arc::new(Mutex::new(HashMap::new()));
        let playback_error = Arc::new(Mutex::new(None));
        let worker = spawn_decoder(Arc::clone(&jobs), Arc::clone(&failed_decodes));
        let prefetch_cache = Arc::new(Mutex::new(PrefetchCache::new()));
        let (prefetch_sender, prefetch_receiver) = sync_channel(4);
        let prefetch_closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let prefetch_generation = Arc::new(AtomicU64::new(0));
        let prefetch_worker = spawn_prefetcher(
            prefetch_receiver,
            Arc::clone(&prefetch_cache),
            Arc::clone(&prefetch_closed),
            Arc::clone(&prefetch_generation),
        );

        Self {
            audio: Arc::new(Mutex::new(audio)),
            jobs,
            worker: Mutex::new(Some(worker)),
            libraries: Mutex::new(LibrarySet::default()),
            last_clip: Arc::new(Mutex::new(None)),
            prefetch_cache,
            prefetch_sender,
            prefetch_worker: Mutex::new(Some(prefetch_worker)),
            prefetch_closed,
            prefetch_generation,
            failed_decodes,
            playback_error,
            playback_error_seq: Arc::new(AtomicU64::new(0)),
            dropped_publishes: AtomicU64::new(0),
            play_epoch: Arc::new(AtomicU64::new(0)),
            gain_bits: AtomicU32::new(1.0f32.to_bits()),
            loop_state: Mutex::new(LoopSetting::default()),
            streaming_loop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            history: Mutex::new(crate::history::History::new()),
            session: None,
            audio_config: None,
        }
    }

    /// Point the state at a session file and reopen the libraries it remembers.
    ///
    /// Called once from Tauri's `setup` with the app's config directory. Tests do
    /// not call it, so a test run never reads or writes the user's real session.
    ///
    /// Reopening is best-effort and non-blocking: the databases are opened (which
    /// only migrates the schema) but **no index pass runs here**. A first launch
    /// after a long absence would otherwise block the window for a minute before
    /// it could paint; instead the UI asks for a pass and shows its bar.
    pub fn attach_session(&mut self, dir: impl AsRef<Path>) {
        let store = SessionStore::new(&dir);
        let session = store.load();

        let mut set = LibrarySet::default();
        for root in session.present() {
            match Library::open(&root) {
                Ok(library) => set.libs.push(Arc::new(library)),
                Err(e) => log::warn!("could not reopen {}: {e}", root.display()),
            }
        }
        if !set.libs.is_empty() {
            set.active = session
                .first_open()
                .filter(|root| set.libs.iter().any(|l| l.root() == root.as_path()));
            if set.active.is_none() {
                set.active = set.libs.first().map(|l| l.root().to_path_buf());
            }
        }

        if let Ok(mut slot) = self.libraries.lock() {
            *slot = set;
        }
        // Opening the remembered database is deliberately fast. Any pending
        // metadata/UCS migration is picked up by the same background index pass
        // used for normal incremental reindexing, so a large old library cannot
        // hold the WebView on an about:blank first frame.
        for library in self.all_libraries().unwrap_or_default() {
            let _ = library.reindex();
        }
        self.session = Some(store);

        // The device choice lives in the same directory. Reading it here rather
        // than in `boot` keeps one entry point for "restore what the user had",
        // and it means a test that never calls `attach_session` never reads the
        // real config.
        let audio_store = AudioStore::new(dir);
        let config = audio_store.load();
        let wanted = (
            config.backend(),
            config.device.clone(),
            config.asio_buffer_frames,
        );
        let current = self
            .audio
            .lock()
            .ok()
            .map(|a| (a.backend, a.device_filter.clone(), a.asio_frames));
        // Only reopen when the remembered choice differs from what boot opened.
        // Opening an ASIO driver is a few milliseconds but it takes the device
        // for a moment, and re-opening the same WASAPI endpoint would drop the
        // stream for no reason.
        if current.as_ref() != Some(&wanted) {
            self.open_audio(wanted.0, wanted.1.clone(), wanted.2);
        }
        self.audio_config = Some(audio_store);
    }

    /// Swap the output device.
    ///
    /// Everything that depends on the format has to move with it: the decoder the
    /// worker uses, the cached clip (which was converted to the old format and
    /// would play as garbage at the new rate), and the slot the render thread
    /// reads.
    ///
    /// The old engine is dropped **before** the new one is opened, not after. ASIO
    /// allows one loaded driver per process and the SDK's loader closes whatever is
    /// open, so opening the replacement while the old engine still lives would
    /// either fail outright or leave the old callback registered against buffers
    /// the driver just released.
    ///
    /// Returns the error text when the new device could not be opened. The app
    /// stays usable in that state: playback reports the error, everything else
    /// keeps working, and the settings dialog can offer another device.
    pub fn open_audio(
        &self,
        backend: Backend,
        device_filter: Option<String>,
        asio_frames: Option<u32>,
    ) -> Option<String> {
        self.bump_epoch();
        if let Ok(mut slot) = self.audio.lock() {
            // Dropping the engine here stops its stream and joins its render
            // thread, which is what releases the device.
            slot.engine = None;
            slot.monitor = None;
            slot.info = None;
        }

        let next = AudioSlot::open(backend, device_filter, asio_frames);
        // A new engine starts at unity. Restore the gain the user set before the
        // switch, otherwise changing device silently resets the volume slider's
        // effect while the slider still shows the old position.
        let gain = f32::from_bits(self.gain_bits.load(Ordering::Acquire));
        if let Some(engine) = next.engine.as_ref() {
            engine.set_gain(gain);
        }
        // The loop switch is remembered for the same reason, and it is restored
        // before the engine can be handed a clip: `set_loop` also clears
        // `finished`, which on a fresh engine is already false.
        if let Ok(setting) = self.loop_state.lock() {
            self.streaming_loop.store(false, Ordering::Release);
            if let Some(engine) = next.engine.as_ref() {
                engine.set_loop(setting.enabled, setting.region);
            }
        }
        let error = next.error.clone();
        if let Ok(mut slot) = self.audio.lock() {
            *slot = next;
        }
        // A clip in the cache is in the old device format; keeping it would make
        // the next play of that path skip the decode and hand the render thread
        // bytes it cannot interpret.
        if let Ok(mut cached) = self.last_clip.lock() {
            *cached = None;
        }
        if let Ok(mut cache) = self.prefetch_cache.lock() {
            *cache = PrefetchCache::new();
        }
        self.prefetch_generation.fetch_add(1, Ordering::AcqRel);
        error
    }

    /// Switch device and remember the choice. `device` is the name the picker
    /// showed, which is what `EngineConfig::device_filter` matches.
    pub fn set_audio_device(
        &self,
        backend: Backend,
        device: Option<String>,
        asio_frames: Option<u32>,
    ) -> AppResult<AppStatus> {
        let error = self.open_audio(backend, device.clone(), asio_frames);
        if let Some(store) = self.audio_config.as_ref() {
            let config = AudioConfig {
                backend: Some(backend.label().to_lowercase()),
                device,
                asio_buffer_frames: asio_frames,
            };
            if let Err(e) = store.save(&config) {
                log::warn!("could not write the audio config: {e}");
            }
        }
        // A failed open is not a command failure: the state now describes a
        // device that is not there, and `status` is how the window finds out.
        let _ = error;
        self.status()
    }

    /// Every output device, both backends, for the settings picker.
    ///
    /// ASIO drivers are only probed when nothing is playing on ASIO: the probe
    /// loads the driver, and the SDK's loader closes whatever driver is already
    /// open, which would kill the live stream. A plain WASAPI stream is not
    /// affected, so the channel counts are still filled in then.
    pub fn list_audio_devices(&self) -> Vec<AudioDeviceDto> {
        let probing_asio = !matches!(self.audio.lock().map(|a| a.backend), Ok(Backend::Asio));
        mio_engine::output::list_devices(probing_asio)
            .into_iter()
            .map(|d| AudioDeviceDto {
                backend: d.backend.label().to_string(),
                name: d.name,
                label: d.label,
                channels: d.channels,
                buffer_frames: d.buffer_frames,
                default: d.default,
                available: d.available,
            })
            .collect()
    }

    /// Whether this build can open ASIO drivers at all. The dialog uses it to
    /// explain an empty ASIO section instead of showing nothing.
    pub fn asio_compiled(&self) -> bool {
        cfg!(feature = "asio")
    }

    /// Write the current open-library list back to the session file.
    ///
    /// Called after anything that changes the set. Failures are logged, not
    /// surfaced: losing the list costs one folder pick, and an error dialog for it
    /// would be worse than the problem.
    pub fn persist_session(&self) {
        let Some(store) = self.session.as_ref() else {
            return;
        };
        let Ok(set) = self.libraries.lock() else {
            return;
        };
        let session = Session {
            libraries: set.roots(),
            active: set.active.clone(),
        };
        drop(set);
        if let Err(e) = store.save(&session) {
            log::warn!("could not write session file: {e}");
        }
    }

    pub fn engine_info(&self) -> Option<EngineInfo> {
        self.audio.lock().ok().and_then(|a| a.info.clone())
    }

    pub fn engine_error(&self) -> Option<String> {
        self.audio.lock().ok().and_then(|a| a.error.clone())
    }

    pub fn device_filter(&self) -> Option<String> {
        self.audio.lock().ok().and_then(|a| a.device_filter.clone())
    }

    pub fn backend(&self) -> Backend {
        self.audio
            .lock()
            .map(|a| a.backend)
            .unwrap_or(Backend::Wasapi)
    }

    /// A polling handle for the playhead. `None` when no device opened.
    pub fn monitor(&self) -> Option<PlaybackMonitor> {
        self.audio.lock().ok().and_then(|a| a.monitor.clone())
    }

    /// A decoder configured for the current device format.
    pub fn decoder(&self) -> Decoder {
        self.audio.lock().map(|a| a.decoder()).unwrap_or_else(|_| {
            Decoder::new(OutputFormat {
                sample_rate: 48_000,
                channels: 2,
                sample: mio_engine::DeviceSample::F32,
            })
        })
    }

    pub fn output_format(&self) -> OutputFormat {
        self.audio.lock().map(|a| a.format).unwrap_or(OutputFormat {
            sample_rate: 48_000,
            channels: 2,
            sample: mio_engine::DeviceSample::F32,
        })
    }

    /// Run `f` against the live engine, or report why there is none.
    ///
    /// Every playback call goes through this rather than holding the audio lock
    /// itself: the engine's publish path is lock-free, so the lock is only held
    /// for the duration of the call and never across a decode.
    fn with_engine<T>(&self, f: impl FnOnce(&Engine) -> T) -> AppResult<T> {
        let audio = self
            .audio
            .lock()
            .map_err(|_| AppError::internal("audio lock poisoned"))?;
        match audio.engine.as_ref() {
            Some(engine) => Ok(f(engine)),
            None => Err(AppError::NoAudioDevice(
                audio.error.clone().unwrap_or_default(),
            )),
        }
    }

    /// Publish a decoded clip and return the publish sequence number.
    pub fn publish(&self, clip: Arc<DecodedAudio>, source: PublishSource) -> AppResult<u64> {
        self.with_engine(|engine| engine.publish(clip, source))
    }

    /// Publish starting `start_at` seconds in. A seek is a starting offset on an
    /// immutable buffer, so it costs the same atomic store a normal start does.
    pub fn publish_at(&self, clip: Arc<DecodedAudio>, start_at: f64) -> AppResult<u64> {
        let format = self.output_format();
        let window_start_s = clip
            .window_start_frames
            .saturating_sub(clip.trimmed_leading_frames) as f64
            / clip.source_rate as f64;
        let relative_s = if clip.complete {
            start_at.max(0.0)
        } else {
            (start_at - window_start_s).max(0.0)
        };
        let offset = (relative_s * format.sample_rate as f64).round() as u64;
        self.with_engine(|engine| engine.publish_from(clip, offset))
    }

    pub fn stop_playback(&self) -> AppResult<()> {
        self.play_epoch.fetch_add(1, Ordering::AcqRel);
        self.clear_playback_error();
        self.with_engine(|engine| engine.stop())
    }

    /// Freeze the loaded clip. Distinct from stop: the slot stays populated and
    /// the playhead stays where it is, so Space resumes rather than restarts.
    pub fn pause_playback(&self) -> AppResult<()> {
        self.with_engine(|engine| engine.pause())
    }

    pub fn resume_playback(&self) -> AppResult<()> {
        self.with_engine(|engine| engine.resume())
    }

    /// True while a clip is loaded and paused.
    pub fn is_paused(&self) -> bool {
        self.with_engine(|engine| engine.paused()).unwrap_or(false)
    }

    /// Set the output gain on the live engine.
    ///
    /// Returns the value the engine actually holds, which is the clamped one. When
    /// no device is open the call is not an error: the value is remembered so that
    /// opening a device later starts at the volume the user set, and the returned
    /// number is what the slider should show.
    pub fn set_gain(&self, gain: f32) -> AppResult<f32> {
        let clamped = gain.clamp(0.0, 4.0);
        self.gain_bits.store(clamped.to_bits(), Ordering::Release);
        if let Ok(audio) = self.audio.lock() {
            if let Some(engine) = audio.engine.as_ref() {
                engine.set_gain(clamped);
            }
        }
        Ok(clamped)
    }

    /// The gain currently applied. Falls back to the remembered value when the
    /// device is closed, so the slider never jumps to unity on a device error.
    pub fn gain(&self) -> f32 {
        f32::from_bits(self.gain_bits.load(Ordering::Acquire))
    }

    /// Loop the loaded clip, optionally within `[start_s, end_s)`.
    ///
    /// `region` is `None` for the whole clip. The engine holds the state, so this
    /// is one atomic write on the live engine plus a remembered copy for when the
    /// device is reopened — the same shape as the gain, and for the same reason:
    /// a switch flipped while no device is open must not be forgotten.
    pub fn set_loop(&self, enabled: bool, region: Option<(f64, f64)>) -> AppResult<LoopState> {
        let mut remembered = self
            .loop_state
            .lock()
            .map_err(|_| AppError::internal("loop lock poisoned"))?;
        *remembered = LoopSetting {
            enabled,
            region: region.filter(|(from, to)| to > from),
        };
        let incomplete = self
            .last_clip
            .lock()
            .ok()
            .and_then(|slot| {
                slot.as_ref()
                    .and_then(|(_, clip)| clip.upgrade())
                    .map(|clip| !clip.complete)
            })
            .unwrap_or(false);
        self.streaming_loop
            .store(enabled && incomplete, Ordering::Release);
        if let Ok(audio) = self.audio.lock() {
            if let Some(engine) = audio.engine.as_ref() {
                engine.set_loop(enabled, region);
                let _ = engine.set_streaming_loop(enabled && incomplete);
            }
        }
        Ok(LoopState {
            enabled,
            region: remembered.region,
        })
    }

    /// The loop switch and its region, as the engine has them.
    pub fn loop_state(&self) -> LoopState {
        if let Ok(audio) = self.audio.lock() {
            if let Some(engine) = audio.engine.as_ref() {
                let (enabled, region) = engine.loop_state();
                return LoopState { enabled, region };
            }
        }
        match self.loop_state.lock() {
            Ok(setting) => LoopState {
                enabled: setting.enabled,
                region: setting.region,
            },
            Err(_) => LoopState {
                enabled: false,
                region: None,
            },
        }
    }

    /// Render `[start_s, end_s)` of `path` to a WAV in `dir` and return its path.
    ///
    /// This is what makes "drag just the selection into a DAW" possible: a DAW
    /// accepts a file, not a frame range, so the region is written out first and
    /// that path is what the drag carries. The file is named after the source plus
    /// the region, so a DAW's clip name says which part of which sample it is.
    ///
    /// Written at the source's own rate and channel count — an export is a copy of
    /// the material, not a monitoring path, and resampling it to the interface's
    /// current format would quietly change what the user drags into their project.
    pub async fn export_region(
        &self,
        path: &str,
        start_s: f64,
        end_s: f64,
        dir: &std::path::Path,
    ) -> AppResult<String> {
        if !mio_index::is_audio_path(std::path::Path::new(path)) {
            return Err(AppError::NotAudio(path.to_string()));
        }
        if end_s <= start_s {
            return Err(AppError::internal("the selected region is empty"));
        }

        let decoder = self.decoder().with_window(mio_engine::DecodeWindow::at(
            start_s.max(0.0),
            (end_s - start_s).max(0.0),
        ));
        let source_path = path.to_string();
        let out_dir = dir.to_path_buf();
        let stem = std::path::Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "region".into());
        // Seconds with two decimals, sanitised: a Windows path cannot carry `:`,
        // and the numbers are there so two selections of one file do not collide.
        let name = format!(
            "{} {:.2}-{:.2}s.wav",
            sanitize_file_name(&stem),
            start_s.max(0.0),
            end_s
        );
        let out_path = out_dir.join(name);

        let written = tauri::async_runtime::spawn_blocking(move || {
            std::fs::create_dir_all(&out_dir).map_err(|e| {
                AppError::internal(format!("could not create the export folder: {e}"))
            })?;
            let source = decoder.decode_source(&source_path)?;
            mio_engine::export::write_wav_region(&out_path, &source, 0.0, end_s - start_s)
                .map(|frames| (out_path, frames))
                .map_err(AppError::from)
        })
        .await
        .map_err(|e| AppError::internal(format!("export task failed: {e}")))??;

        let (path, frames) = written;
        if frames == 0 {
            return Err(AppError::internal(
                "the selected region is outside the file",
            ));
        }
        Ok(path.display().to_string())
    }

    /// Invalidate any decode in flight. Called when playback is stopped or when a
    /// newer play request arrives.
    pub fn bump_epoch(&self) -> u64 {
        self.play_epoch.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn clear_playback_error(&self) {
        if let Ok(mut error) = self.playback_error.lock() {
            *error = None;
        }
    }

    pub fn report_playback_error(&self, path: String, position_s: f64) {
        if let Ok(mut error) = self.playback_error.lock() {
            *error = Some(PlaybackError { path, position_s });
            self.playback_error_seq.fetch_add(1, Ordering::AcqRel);
        }
    }

    pub fn take_playback_error(&self) -> Option<crate::commands::PlaybackErrorDto> {
        self.playback_error
            .lock()
            .ok()?
            .take()
            .map(|error| crate::commands::PlaybackErrorDto {
                path: error.path,
                position_s: error.position_s,
            })
    }

    pub fn playback_error_seq(&self) -> u64 {
        self.playback_error_seq.load(Ordering::Acquire)
    }

    pub fn epoch(&self) -> u64 {
        self.play_epoch.load(Ordering::Acquire)
    }

    /// The clip for `path` if it is already decoded.
    pub fn cached_clip(&self, path: &str) -> Option<Arc<DecodedAudio>> {
        if let Ok(slot) = self.last_clip.lock() {
            if let Some((cached_path, clip)) = slot.as_ref() {
                if cached_path == path {
                    return clip.upgrade();
                }
            }
        }
        self.prefetch_cache.lock().ok()?.get(path)
    }

    pub fn remember_clip(&self, path: String, clip: Arc<DecodedAudio>) {
        let mut cache = self.prefetch_cache.lock().ok();
        if let Some(cache) = cache.as_mut() {
            cache.insert(path.clone(), Arc::clone(&clip));
        }
        if let Ok(mut slot) = self.last_clip.lock() {
            *slot = Some((path, Arc::downgrade(&clip)));
        }
    }

    pub fn prefetch_neighbors(&self, paths: &[String]) {
        let decoder = self.decoder();
        for path in paths.iter().take(PrefetchCache::MAX_NEIGHBORS) {
            let requested = self
                .prefetch_cache
                .lock()
                .map(|mut cache| cache.request(path))
                .unwrap_or(false);
            if !requested {
                continue;
            }
            if self
                .prefetch_sender
                .try_send((
                    path.clone(),
                    decoder,
                    self.prefetch_generation.load(Ordering::Acquire),
                ))
                .is_err()
            {
                if let Ok(mut cache) = self.prefetch_cache.lock() {
                    cache.finish_request(path);
                }
            }
        }
    }

    pub fn note_dropped_publish(&self) {
        self.dropped_publishes.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dropped_publishes(&self) -> u64 {
        self.dropped_publishes.load(Ordering::Relaxed)
    }

    /// Queue a decode, replacing any request that has not started yet.
    ///
    /// The returned receiver yields the clip, or an error. A receiver whose job
    /// was replaced gets `superseded`, which the caller reports as "another play
    /// request arrived first" rather than as a failure.
    ///
    /// The decoder is attached to the job, not read off the worker, so a device
    /// switch mid-session cannot leave a decode converting to the old format.
    pub fn request_decode(
        &self,
        path: String,
        max_duration_s: Option<f64>,
        window: Option<mio_engine::DecodeWindow>,
    ) -> AppResult<Receiver<AppResult<Arc<DecodedAudio>>>> {
        if let Some((size, modified)) = decode_fingerprint(&path) {
            if self
                .failed_decodes
                .lock()
                .ok()
                .and_then(|failed| failed.get(&path).copied())
                == Some((size, modified))
            {
                return Err(AppError::internal(format!(
                    "decoding this unchanged file previously exceeded the time limit: {path}"
                )));
            }
        }
        let (tx, rx) = channel();
        let decoder = self.decoder();
        // A full slot is impossible (one job, replaced), so a failed send means
        // the worker is gone.
        match try_submit(
            &self.jobs,
            DecodeJob {
                path,
                decoder,
                max_duration_s,
                window,
                reply: tx,
            },
        ) {
            Ok(()) => Ok(rx),
            Err(_) => Err(AppError::internal("decode worker is not running")),
        }
    }

    async fn await_decode(
        &self,
        receiver: Receiver<AppResult<Arc<DecodedAudio>>>,
    ) -> AppResult<Arc<DecodedAudio>> {
        tauri::async_runtime::spawn_blocking(move || {
            receiver.recv_timeout(std::time::Duration::from_secs(35))
        })
        .await
        .map_err(|e| AppError::internal(format!("decode task failed: {e}")))?
        .map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => {
                AppError::internal("decode worker did not respond before the 35s command limit")
            }
            std::sync::mpsc::RecvTimeoutError::Disconnected => {
                AppError::internal("decode worker closed before answering")
            }
        })?
    }

    /// The library the window is showing.
    pub fn library(&self) -> AppResult<Arc<Library>> {
        self.libraries
            .lock()
            .map_err(|_| AppError::internal("library lock poisoned"))?
            .active()
            .ok_or(AppError::NoLibrary)
    }

    /// Every open library, for undo to find the one an edit belonged to.
    pub fn all_libraries(&self) -> AppResult<Vec<Arc<Library>>> {
        Ok(self
            .libraries
            .lock()
            .map_err(|_| AppError::internal("library lock poisoned"))?
            .libs
            .clone())
    }

    /// Roots of every open library, in the order they were opened.
    pub fn library_roots(&self) -> AppResult<Vec<PathBuf>> {
        Ok(self
            .libraries
            .lock()
            .map_err(|_| AppError::internal("library lock poisoned"))?
            .roots())
    }

    /// Root folder of the active library, if any.
    pub fn library_root(&self) -> AppResult<PathBuf> {
        Ok(self.library()?.root().to_path_buf())
    }

    /// Open a folder, replacing whatever was open.
    ///
    /// Kept for the single-library path (tests, the CLI harness). The UI uses
    /// [`add_library`](Self::add_library) and [`remove_library`](Self::remove_library).
    pub fn set_library(&self, library: Option<Arc<Library>>) -> AppResult<()> {
        let mut set = self
            .libraries
            .lock()
            .map_err(|_| AppError::internal("library lock poisoned"))?;
        set.libs.clear();
        set.active = None;
        if let Some(library) = library {
            set.active = Some(library.root().to_path_buf());
            set.libs.push(library);
        }
        Ok(())
    }

    // ------------------------------------------------------- IPC-facing logic
    //
    // These are what the Tauri commands call. They live here, not in
    // `commands.rs`, so an integration test can drive them without a window.

    /// Everything the window needs at startup.
    pub fn status(&self) -> AppResult<AppStatus> {
        let info = self.engine_info();
        let library = self.library().ok();
        let backend = info
            .as_ref()
            .map(|i| i.backend)
            .unwrap_or_else(|| self.backend());
        Ok(AppStatus {
            audio_available: info.is_some(),
            audio_error: self.engine_error(),
            device_name: info.as_ref().map(|i| i.device_name.clone()),
            backend: backend.label().to_string(),
            sample_rate: info.as_ref().map(|i| i.format.sample_rate),
            channels: info.as_ref().map(|i| i.format.channels),
            sample_format: info.as_ref().map(|i| i.format.sample.label().to_string()),
            endpoint_frames: info.as_ref().map(|i| i.endpoint_frames),
            engine_period_ms: info.as_ref().map(|i| i.engine_period_ms),
            library: match library {
                Some(lib) => Some(crate::commands::library_status(&lib)?),
                None => None,
            },
        })
    }

    /// Start playing `path`, optionally `start_at` seconds in.
    ///
    /// The path is taken as given. The renderer addresses sounds by absolute path,
    /// same as the reference implementation; re-resolving it against the library
    /// root would break playback of a file indexed under a different root.
    ///
    /// Decoding happens on the decode worker. A repeat of the same sound skips it
    /// — the previous clip is still in `last_clip` — which is what makes arrowing
    /// back and forth cost one atomic store.
    ///
    /// Files longer than 60 s are played as a chain of bounded windows
    /// (`WINDOW_SECONDS`, architecture 3.3): the first publish is a window at
    /// `start_at` (or the head), and `start_window_chain` decodes each following
    /// window and hands it to the engine's queue, which the render callback
    /// consumes in the callback where the previous buffer runs out. The full
    /// file is never resident: a 22-minute mono WAV plays through ~40 windows of
    /// a few MB each, and seeking to an undecoded position is the same window
    /// decode as a start, just further in.
    pub async fn play(&self, path: &str, start_at: Option<f64>) -> AppResult<()> {
        if !mio_index::is_audio_path(std::path::Path::new(path)) {
            return Err(AppError::NotAudio(path.to_string()));
        }
        // Any decode already in flight belongs to a request this one supersedes.
        let epoch = self.bump_epoch();
        self.clear_playback_error();
        // A superseded decode-ahead may have parked a window in the engine.
        self.with_engine(|engine| engine.clear_queued()).ok();

        let looping = self
            .loop_state
            .lock()
            .map(|state| state.enabled)
            .unwrap_or(false);
        let probe = mio_index::probe_file(std::path::Path::new(path));
        let duration_s = probe.duration_s;
        // A missing duration is not evidence that a file is short. Large files
        // with damaged or non-standard headers take the bounded path as well.
        let long_file = duration_s.is_some_and(|duration| duration > 60.0)
            || duration_s.is_none() && probe.filesize >= 4 * 1024 * 1024
            || full_decode_bytes(&probe, self.output_format())
                .is_some_and(|bytes| bytes > mio_engine::MAX_PCM_BYTES);
        let start = start_at.unwrap_or(0.0).max(0.0);

        // A complete cached clip serves any position (it holds the whole file).
        // A prefix window only serves its own start.
        if let Some(clip) = self.cached_clip(path) {
            if clip.complete {
                let result = match start_at {
                    Some(at) if at > 0.0 => self.publish_at(Arc::clone(&clip), at),
                    _ => self.publish(Arc::clone(&clip), PublishSource::Initial),
                };
                result?;
                self.remember_clip(path.to_string(), Arc::clone(&clip));
                return Ok(());
            }
            let window_start_s =
                clip.window_start_frames
                    .saturating_sub(clip.trimmed_leading_frames) as f64
                    / clip.source_rate as f64;
            let window_end_s = window_start_s + clip.frames as f64 / clip.format.sample_rate as f64;
            if !long_file || (start >= window_start_s && start < window_end_s) {
                let result = match start_at {
                    Some(at) if at > 0.0 => self.publish_at(Arc::clone(&clip), at),
                    _ => self.publish(Arc::clone(&clip), PublishSource::Initial),
                };
                result?;
                self.remember_clip(path.to_string(), Arc::clone(&clip));
                if looping && !clip.complete {
                    let _ = self.with_engine(|engine| engine.set_streaming_loop(true));
                }
                self.start_window_chain(path.to_string(), Arc::clone(&clip), epoch);
                return Ok(());
            }
        }

        let window = long_file.then(|| mio_engine::DecodeWindow::at(start, WINDOW_SECONDS));
        let clip = match self.await_decode(self.request_decode(path.to_string(), None, window)?) .await {
            Ok(clip) => clip,
            Err(error) if window.is_none() && matches!(&error, AppError::Engine(e) if e.is_memory_budget()) => {
                log::warn!("full decode exceeded the safety budget for {path}; retrying with windows");
                let fallback = Some(mio_engine::DecodeWindow::at(start, WINDOW_SECONDS));
                self.await_decode(self.request_decode(path.to_string(), None, fallback)?).await?
            }
            Err(error) => return Err(error),
        };

        // A newer play or a stop arrived while this decoded. Dropping the result
        // is correct: starting it now would play a sound the user already left.
        if self.epoch() != epoch {
            return Ok(());
        }

        if window.is_some() {
            // The decode starts exactly at the window's first frame, so the
            // buffer plays from frame 0 of the clip.
            self.with_engine(|engine| engine.publish_from(Arc::clone(&clip), 0))?;
            if looping {
                let _ = self.with_engine(|engine| engine.set_streaming_loop(true));
            }
        } else {
            match start_at {
                Some(at) if at > 0.0 => self.publish_at(Arc::clone(&clip), at)?,
                _ => self.publish(Arc::clone(&clip), PublishSource::Initial)?,
            };
        }
        self.remember_clip(path.to_string(), Arc::clone(&clip));
        if !clip.complete {
            self.start_window_chain(path.to_string(), Arc::clone(&clip), epoch);
        }
        Ok(())
    }

    /// Decode the following windows of a long file and queue each for the
    /// render callback, until the file is complete.
    ///
    /// Runs on a dedicated thread (the old `mio-decode-ahead`), one window at a
    /// time. The engine's queue holds at most one pending window, so memory
    /// stays at two windows plus whatever the LRU accounts. Every step checks
    /// the play epoch and the prefetch generation: a newer play, a stop or a
    /// device switch invalidates the whole chain.
    fn start_window_chain(&self, path: String, first: Arc<DecodedAudio>, epoch: u64) {
        let audio = Arc::clone(&self.audio);
        let last_clip = Arc::clone(&self.last_clip);
        let prefetch_cache = Arc::clone(&self.prefetch_cache);
        let prefetch_generation = Arc::clone(&self.prefetch_generation);
        let playback_error = Arc::clone(&self.playback_error);
        let playback_error_seq = Arc::clone(&self.playback_error_seq);
        let requested_generation = prefetch_generation.load(Ordering::Acquire);
        let engine_format = self.output_format();
        let play_epoch = Arc::clone(&self.play_epoch);
        let streaming_loop = Arc::clone(&self.streaming_loop);
        let decoder = self.decoder();
        let total_frames = first.total_frames;
        let total_frames_known = first.total_frames_known;
        let first_window_start = (first
            .window_start_frames
            .saturating_sub(first.trimmed_leading_frames) as f64
            * engine_format.sample_rate as f64
            / first.source_rate as f64)
            .round() as usize;
        let mut next_start = first_window_start + first.frames;
        std::thread::Builder::new()
            .name("mio-decode-ahead".into())
            .spawn(move || {
                let alive = |epoch: u64| -> bool {
                    play_epoch.load(Ordering::Acquire) == epoch
                        && prefetch_generation.load(Ordering::Acquire) == requested_generation
                };
                // `total_frames` and the chain cursor are both in device frames.
                let total_device_frames = total_frames;
                // Frames already covered by published or queued windows. The
                // chain stops when that reaches the file's total; the render
                // callback reports `finished` on its own for the last segment.
                while alive(epoch)
                    && (streaming_loop.load(Ordering::Acquire)
                        || !total_frames_known
                        || next_start < total_device_frames)
                {
                    let decode_start = if total_frames_known && next_start >= total_device_frames {
                        0
                    } else {
                        next_start
                    };
                    let window_start_s = decode_start as f64 / engine_format.sample_rate as f64;
                    let window = mio_engine::DecodeWindow::at(window_start_s, WINDOW_SECONDS);
                    let mut attempts = 0;
                    let mut last_error = None;
                    let decoded = retry_decode(|| {
                        attempts += 1;
                        match decoder.with_window(window).decode_with_source(&path) {
                            Ok((clip, _)) => Ok(clip),
                            Err(error) => {
                                log::warn!(
                                    "long-file window decode failed for {path} at {window_start_s:.3}s (attempt {attempts}): {error}"
                                );
                                last_error = Some(error);
                                Err(())
                            }
                        }
                    })
                    .ok();
                    let clip = match decoded {
                        Some(clip) => clip,
                        None => {
                            if play_epoch
                                .compare_exchange(
                                    epoch,
                                    epoch + 1,
                                    Ordering::AcqRel,
                                    Ordering::Acquire,
                                )
                                .is_ok()
                            {
                                if let Ok(slot) = audio.lock() {
                                    if let Some(engine) = slot.engine.as_ref() {
                                        engine.stop();
                                    }
                                }
                                if let Ok(mut error) = playback_error.lock() {
                                    *error = Some(PlaybackError {
                                        path: path.clone(),
                                        position_s: window_start_s,
                                    });
                                    playback_error_seq.fetch_add(1, Ordering::AcqRel);
                                }
                            }
                            if let Some(error) = last_error {
                                log::error!(
                                    "long-file window decode abandoned for {path} at {window_start_s:.3}s after retry: {error}"
                                );
                            }
                            return;
                        }
                    };
                    if clip.frames == 0 {
                        break;
                    }
                    if !alive(epoch) {
                        return;
                    }
                    let clip = Arc::new(clip);
                    // Queue for the render callback. The queue holds one
                    // window; wait until the callback has consumed the
                    // previous one before queueing the next. Pause freezes
                    // the callback's consumption, so wait for that too.
                    let mut queued = false;
                    while alive(epoch) {
                        let monitor = audio.lock().ok().and_then(|slot| slot.monitor.clone());
                        let Some(monitor) = monitor else { return };
                        let snapshot = monitor.snapshot();
                        if snapshot.paused {
                            std::thread::sleep(std::time::Duration::from_millis(20));
                            continue;
                        }
                        if !queued {
                            if let Ok(slot) = audio.lock() {
                                if let Some(engine) = slot.engine.as_ref() {
                                    engine.queue_next(Arc::clone(&clip));
                                    queued = true;
                                } else {
                                    return;
                                }
                            } else {
                                return;
                            }
                        } else {
                            // Queued and still playing this chain. Wait for
                            // the callback to take it before producing the
                            // next window: the queue is one deep, and the
                            // position having moved past the queued boundary
                            // proves the handoff happened.
                            let queue_empty = audio
                                .lock()
                                .ok()
                                .and_then(|slot| {
                                    slot.engine.as_ref().map(|engine| !engine.has_queued())
                                })
                                .unwrap_or(false);
                            if queue_empty {
                                break;
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    if !alive(epoch) {
                        return;
                    }
                    let actual_start = (clip
                        .window_start_frames
                        .saturating_sub(clip.trimmed_leading_frames)
                        as f64
                        * engine_format.sample_rate as f64
                        / clip.source_rate as f64)
                        .round() as usize;
                    let actual_end = actual_start.saturating_add(clip.frames);
                    // The final window is `complete`; remember it so a replay
                    // of this file serves any position without a decode.
                    if clip.complete && actual_start == 0 && clip.frames >= total_device_frames {
                        if let Ok(mut cache) = prefetch_cache.lock() {
                            cache.insert(path.clone(), Arc::clone(&clip));
                        }
                        if let Ok(mut cache) = last_clip.lock() {
                            *cache = Some((path.clone(), Arc::downgrade(&clip)));
                        }
                    }
                    if clip.complete {
                        if streaming_loop.load(Ordering::Acquire) {
                            next_start = 0;
                        } else {
                            break;
                        }
                    } else {
                        next_start = actual_end;
                    }
                }
            })
            .ok();
    }

    /// The playhead, as atomics only.
    pub fn playback_position(&self) -> PlaybackPosition {
        match self.monitor() {
            Some(monitor) => {
                let snap = monitor.snapshot();
                PlaybackPosition {
                    position_s: snap.position_s,
                    duration_s: snap.duration_s,
                    playing: snap.playing,
                    paused: snap.paused,
                    finished: snap.finished,
                    error_seq: self.playback_error_seq(),
                }
            }
            None => PlaybackPosition {
                position_s: 0.0,
                duration_s: 0.0,
                playing: false,
                paused: false,
                finished: true,
                error_seq: self.playback_error_seq(),
            },
        }
    }
    /// Peak envelope for the waveform panel.
    ///
    /// ~1000 floats, never PCM: `AGENTS.md` forbids decoded audio crossing into
    /// the webview, and a 1000-point envelope is the same size for a 3-second
    /// sample and a 3-minute one. Decoding the whole file is the cost; a 3-minute
    /// 96kHz file is ~112ms of CPU, which is why `decode_ms` comes back with the
    /// data and why the front end draws nothing until it resolves.
    pub async fn waveform(&self, path: &str, buckets: Option<usize>) -> AppResult<WaveformDto> {
        if !mio_index::is_audio_path(std::path::Path::new(path)) {
            return Err(AppError::NotAudio(path.to_string()));
        }
        let decoder = self.decoder();
        let buckets = buckets.unwrap_or(DEFAULT_BUCKETS).clamp(16, 16_384);
        let owned = path.to_string();

        if let Some(clip) = self.cached_clip(path) {
            if clip.complete {
                if let Some(source) = clip.source.as_ref() {
                    let waveform = mio_engine::waveform::Waveform::from_source(source, buckets);
                    return Ok(WaveformDto {
                        peaks: waveform.peaks,
                        lanes: waveform.lanes,
                        duration_s: waveform.duration_s,
                        sample_rate: waveform.sample_rate,
                        channels: waveform.channels,
                        decode_ms: 0.0,
                    });
                }
            }
        }

        let started = Instant::now();
        let scan = tauri::async_runtime::spawn_blocking(move || {
            mio_engine::waveform::Waveform::scan(&decoder, &owned, buckets)
        })
        .await
        .map_err(|e| AppError::internal(format!("waveform task failed: {e}")))?;
        let waveform = scan?;

        Ok(WaveformDto {
            peaks: waveform.peaks,
            lanes: waveform.lanes,
            duration_s: waveform.duration_s,
            sample_rate: waveform.sample_rate,
            channels: waveform.channels,
            decode_ms: started.elapsed().as_secs_f64() * 1000.0,
        })
    }

    /// Add a library folder and index it **before returning**.
    ///
    /// Blocking on purpose. The user's call: an index you can browse half of is
    /// worse than a progress bar that says how long it will be, because every row
    /// you can see might be missing half its siblings. So this runs the pass on the
    /// calling thread and reports progress through `progress`; the UI shows a modal
    /// bar and the window stays unusable until it returns.
    ///
    /// On a 280k library that is 64 seconds for a first pass and ~5s incremental,
    /// both measured (`docs/index.md`). A pass that is already running for this
    /// folder is not started twice.
    pub fn add_library<F>(&self, root: &str, full: bool, mut progress: F) -> AppResult<LibraryStatus>
    where
        F: FnMut(usize, usize),
    {
        self.add_library_with_events(root, full, |event| {
            if matches!(event.phase, mio_index::IndexPhase::Indexing | mio_index::IndexPhase::Ready) {
                progress(event.done, event.total);
            }
        })
    }

    pub fn add_library_with_events<F>(
        &self,
        root: &str,
        full: bool,
        progress: F,
    ) -> AppResult<LibraryStatus>
    where
        F: FnMut(crate::library::IndexProgress),
    {
        let path = PathBuf::from(root);
        if !path.is_dir() {
            return Err(AppError::internal(format!("not a folder: {root}")));
        }

        // Re-adding a folder that is already open just reindexes it: the user
        // means "refresh", not "open a second copy".
        let library = {
            let mut set = self
                .libraries
                .lock()
                .map_err(|_| AppError::internal("library lock poisoned"))?;
            match set.find(&path) {
                Some(library) => library,
                None => {
                    let library = Arc::new(Library::open(&path)?);
                    set.libs.push(Arc::clone(&library));
                    set.active = Some(path.clone());
                    library
                }
            }
        };

        library.index_blocking_with_events(full, progress)?;

        // Selecting the folder that was just indexed is what the user expects;
        // adding a second library should not silently leave the first one active.
        if let Ok(mut set) = self.libraries.lock()
            && set.find(&path).is_some_and(|current| Arc::ptr_eq(&current, &library))
        {
            set.active = Some(path);
        }
        self.persist_session();
        crate::commands::library_status(&library)
    }

    /// Close a library. Returns whether the active one changed.
    pub fn remove_library(&self, root: &str) -> AppResult<bool> {
        let path = PathBuf::from(root);
        let changed = {
            let mut set = self
                .libraries
                .lock()
                .map_err(|_| AppError::internal("library lock poisoned"))?;
            let before = set.active.clone();
            if let Some(library) = set.find(&path) {
                library.close();
            }
            set.libs.retain(|l| l.root() != path.as_path());
            if before.as_deref() == Some(path.as_path()) {
                set.active = set.libs.first().map(|l| l.root().to_path_buf());
            }
            set.active != before
        };
        self.persist_session();
        Ok(changed)
    }

    pub(crate) fn close_libraries(&self) {
        if let Ok(libraries) = self.all_libraries() {
            for library in libraries {
                library.close();
            }
        }
    }

    /// Show a different open library.
    pub fn set_active_library(&self, root: &str) -> AppResult<()> {
        let path = PathBuf::from(root);
        {
            let mut set = self
                .libraries
                .lock()
                .map_err(|_| AppError::internal("library lock poisoned"))?;
            if set.find(&path).is_none() {
                return Err(AppError::internal(format!("library is not open: {root}")));
            }
            set.active = Some(path);
        }
        self.persist_session();
        Ok(())
    }

    /// Every open library with its counts, for the sidebar.
    pub fn library_list(&self) -> AppResult<Vec<LibraryStatus>> {
        let libraries = self.all_libraries()?;
        let active = self.library_root().ok();
        libraries
            .iter()
            .map(|library| {
                let mut status = crate::commands::library_status(library)?;
                status.active = active.as_deref() == Some(library.root());
                Ok(status)
            })
            .collect()
    }

    /// Reindex one library, blocking, with progress.
    ///
    /// `root` names the library; `None` means the active one. A background library
    /// can be reindexed without switching to it, which is what the sidebar's
    /// context menu does.
    pub fn reindex_library<F>(
        &self,
        root: Option<&str>,
        full: bool,
        mut progress: F,
    ) -> AppResult<LibraryStatus>
    where
        F: FnMut(usize, usize),
    {
        self.reindex_library_with_events(root, full, |event| {
            if matches!(event.phase, mio_index::IndexPhase::Indexing | mio_index::IndexPhase::Ready) {
                progress(event.done, event.total);
            }
        })
    }

    pub fn reindex_library_with_events<F>(
        &self,
        root: Option<&str>,
        full: bool,
        progress: F,
    ) -> AppResult<LibraryStatus>
    where
        F: FnMut(crate::library::IndexProgress),
    {
        let library = match root {
            Some(root) => {
                let path = PathBuf::from(root);
                self.libraries
                    .lock()
                    .map_err(|_| AppError::internal("library lock poisoned"))?
                    .find(&path)
                    .ok_or_else(|| AppError::internal(format!("library is not open: {root}")))?
            }
            None => self.library()?,
        };
        library.index_blocking_with_events(full, progress)?;
        crate::commands::library_status(&library)
    }

    pub fn library_status(&self) -> AppResult<Option<LibraryStatus>> {
        match self.library() {
            Ok(lib) => Ok(Some(crate::commands::library_status(&lib)?)),
            Err(AppError::NoLibrary) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn search(&self, request: SearchRequest) -> AppResult<Vec<SoundDto>> {
        let library = self.library()?;
        let params = request.into_params()?;
        Ok(library
            .search(&params)?
            .into_iter()
            .map(SoundDto::from)
            .collect())
    }

    fn matching_edit_params(edit: &MatchingEditRequest) -> AppResult<mio_index::SearchParams> {
        let mut params = edit.request.clone().into_params()?;
        params.offset = edit.offset.unwrap_or(0);
        params.limit = edit.limit.unwrap_or(u32::MAX);
        Ok(params)
    }

    /// How many rows the filter matches, for the status bar's "38 of 27,525".
    pub fn count_matching(&self, request: SearchRequest) -> AppResult<u64> {
        let library = self.library()?;
        Ok(library.count_matching(&request.into_params()?)?)
    }

    pub fn list_folders(&self) -> AppResult<FolderNodeDto> {
        Ok(self.library()?.folder_tree()?.into())
    }

    /// Facet counts, favorites and total, in one call for the sidebar.
    pub fn sidebar(&self) -> AppResult<crate::commands::SidebarDto> {
        let library = self.library()?;
        let facets = library
            .facets()?
            .into_iter()
            .map(|f| crate::commands::FacetDto {
                category: f.category,
                subcategory: f.subcategory,
                count: f.count,
            })
            .collect();
        let ucs_facets = library
            .ucs_facets()?
            .into_iter()
            .map(|f| crate::commands::UcsFacetDto {
                category: f.category,
                subcategory: f.subcategory,
                count: f.count,
            })
            .collect();
        let (favorites, total) = library.summary()?;
        Ok(crate::commands::SidebarDto {
            facets,
            ucs_facets,
            favorites,
            total,
        })
    }

    /// Star or unstar many sounds in one transaction, recording it for undo.
    pub fn set_favorite(&self, paths: &[String], favorite: bool) -> AppResult<u64> {
        let library = self.library()?;
        let kind = if favorite {
            EditKind::Favorite
        } else {
            EditKind::Unfavorite
        };
        let edit = crate::history::run(&library, kind, paths, || {
            Ok(library.set_favorite(paths, favorite)?)
        })?;
        self.record(edit);
        Ok(paths.len() as u64)
    }

    /// Assign one category to many sounds, marking them manual, recording for undo.
    pub fn set_category(
        &self,
        paths: &[String],
        category: &str,
        subcategory: Option<&str>,
    ) -> AppResult<u64> {
        let library = self.library()?;
        let edit = crate::history::run(&library, EditKind::SetCategory, paths, || {
            Ok(library.set_category(paths, category, subcategory)?)
        })?;
        self.record(edit);
        Ok(paths.len() as u64)
    }

    pub fn set_ucs_category(
        &self,
        paths: &[String],
        category: &str,
        subcategory: Option<&str>,
        cat_id: &str,
        projected_category: &str,
        projected_subcategory: Option<&str>,
    ) -> AppResult<u64> {
        let library = self.library()?;
        let edit = crate::history::run(&library, EditKind::SetUcsCategory, paths, || {
            Ok(library.set_ucs_category(
                paths,
                category,
                subcategory,
                cat_id,
                projected_category,
                projected_subcategory,
            )?)
        })?;
        self.record(edit);
        Ok(paths.len() as u64)
    }

    pub fn set_ucs_category_matching(
        &self,
        edit: MatchingEditRequest,
        category: &str,
        subcategory: Option<&str>,
        cat_id: &str,
        projected_category: &str,
        projected_subcategory: Option<&str>,
    ) -> AppResult<u64> {
        let library = self.library()?;
        let params = Self::matching_edit_params(&edit)?;
        let (before, changed) = library.set_ucs_category_matching(
            &params,
            &edit.exclude,
            category,
            subcategory,
            cat_id,
            projected_category,
            projected_subcategory,
        )?;
        if changed == 0 {
            return Ok(0);
        }
        self.record(crate::history::edit_from_snapshot(
            &library,
            EditKind::SetUcsCategory,
            before,
        ));
        Ok(changed)
    }

    /// Drop the manual category on many sounds, recording for undo.
    pub fn clear_category(&self, paths: &[String]) -> AppResult<u64> {
        let library = self.library()?;
        let edit = crate::history::run(&library, EditKind::ClearCategory, paths, || {
            Ok(library.clear_category(paths)?)
        })?;
        self.record(edit);
        Ok(paths.len() as u64)
    }

    pub fn set_favorite_matching(
        &self,
        edit: MatchingEditRequest,
        favorite: bool,
    ) -> AppResult<u64> {
        let library = self.library()?;
        let params = Self::matching_edit_params(&edit)?;
        let kind = if favorite {
            EditKind::Favorite
        } else {
            EditKind::Unfavorite
        };
        let (before, changed) = library.set_favorite_matching(&params, &edit.exclude, favorite)?;
        if changed == 0 {
            return Ok(0);
        }
        let entry = crate::history::edit_from_snapshot(&library, kind, before);
        self.record(entry);
        Ok(changed)
    }

    pub fn set_category_matching(
        &self,
        edit: MatchingEditRequest,
        category: &str,
        subcategory: Option<&str>,
    ) -> AppResult<u64> {
        let library = self.library()?;
        let params = Self::matching_edit_params(&edit)?;
        let (before, changed) =
            library.set_category_matching(&params, &edit.exclude, category, subcategory)?;
        if changed == 0 {
            return Ok(0);
        }
        let entry = crate::history::edit_from_snapshot(&library, EditKind::SetCategory, before);
        self.record(entry);
        Ok(changed)
    }

    pub fn clear_category_matching(&self, edit: MatchingEditRequest) -> AppResult<u64> {
        let library = self.library()?;
        let params = Self::matching_edit_params(&edit)?;
        let (before, changed) = library.clear_category_matching(&params, &edit.exclude)?;
        if changed == 0 {
            return Ok(0);
        }
        let entry = crate::history::edit_from_snapshot(&library, EditKind::ClearCategory, before);
        self.record(entry);
        Ok(changed)
    }

    fn record(&self, edit: crate::history::Edit) {
        if let Ok(mut history) = self.history.lock() {
            history.push(edit);
        }
    }

    /// Undo the last edit. Returns what kind it was, if any.
    pub fn undo(&self) -> AppResult<Option<EditKind>> {
        let libraries = self.all_libraries()?;
        let mut history = self
            .history
            .lock()
            .map_err(|_| AppError::internal("history lock poisoned"))?;
        history.undo(&libraries)
    }

    /// Redo the last undone edit.
    pub fn redo(&self) -> AppResult<Option<EditKind>> {
        let libraries = self.all_libraries()?;
        let mut history = self
            .history
            .lock()
            .map_err(|_| AppError::internal("history lock poisoned"))?;
        history.redo(&libraries)
    }

    /// What undo/redo would do, so the UI can name it in the user's language.
    pub fn history_state(&self) -> (Option<EditKind>, Option<EditKind>) {
        let history = match self.history.lock() {
            Ok(h) => h,
            Err(_) => return (None, None),
        };
        (history.undo_kind(), history.redo_kind())
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        self.close_libraries();
        if let Ok(mut slot) = self.prefetch_worker.lock() {
            if let Some(handle) = slot.take() {
                self.prefetch_closed.store(true, Ordering::Release);
                let _ = handle.join();
            }
        }
        self.jobs.close();
        if let Ok(mut slot) = self.worker.lock() {
            if let Some(handle) = slot.take() {
                let _ = handle.join();
            }
        }
    }
}

fn try_submit(jobs: &LatestSlot, job: DecodeJob) -> std::result::Result<(), ()> {
    // `submit` never blocks and never fails except on a poisoned lock, which
    // means the process is already in trouble.
    jobs.submit(job);
    Ok(())
}

/// The decode worker thread.
///
/// One thread, not a pool: the queue holds a single job at a time and replaces
/// it, so a pool would add threads that are idle by construction. Per-file decode
/// is 1–6ms for a typical sample (measured, architecture 3.1), which is far below
/// the ~12ms of device queueing that dominates a click anyway.
fn spawn_decoder(
    jobs: Arc<LatestSlot>,
    failed_decodes: Arc<Mutex<HashMap<String, (u64, u128)>>>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("mio-decode".into())
        .spawn(move || {
            while let Some(job) = jobs.take() {
                let path = job.path.clone();
                let mut decoder = job
                    .decoder
                    .with_max_duration(job.max_duration_s.unwrap_or(f64::INFINITY));
                if let Some(window) = job.window {
                    decoder = decoder.with_window(window);
                }
                let result = decoder.decode_with_source(&job.path);
                if matches!(&result, Err(mio_engine::EngineError::DecodeTimeout { .. })) {
                    if let Some(fingerprint) = decode_fingerprint(&path) {
                        if let Ok(mut failed) = failed_decodes.lock() {
                            failed.insert(path.clone(), fingerprint);
                        }
                    }
                } else if result.is_ok() {
                    if let Ok(mut failed) = failed_decodes.lock() {
                        failed.remove(&path);
                    }
                }
                let clip = result
                    .map(|(clip, _source)| Arc::new(clip))
                    .map_err(AppError::from);
                // A dropped receiver means the command was cancelled (the window
                // closed, or a newer request superseded it). Either way the decode
                // result is not needed.
                let _ = job.reply.send(clip);
            }
        })
        .expect("spawn decode worker")
}

fn spawn_prefetcher(
    receiver: Receiver<(String, Decoder, u64)>,
    cache: Arc<Mutex<PrefetchCache>>,
    closed: Arc<std::sync::atomic::AtomicBool>,
    generation: Arc<AtomicU64>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("mio-prefetch".into())
        .spawn(move || {
            while !closed.load(Ordering::Acquire) {
                let Ok((path, decoder, requested_generation)) =
                    receiver.recv_timeout(std::time::Duration::from_millis(100))
                else {
                    continue;
                };
                if let Ok(mut cache) = cache.lock() {
                    if cache.get(&path).is_some() {
                        cache.finish_request(&path);
                        continue;
                    }
                }
                let max_duration_s = mio_index::probe_file(std::path::Path::new(&path))
                    .duration_s
                    .filter(|duration| *duration > 60.0)
                    .map(|_| 5.0);
                let decoder = max_duration_s
                    .map(|seconds| decoder.with_max_duration(seconds))
                    .unwrap_or(decoder);
                let result = decoder.decode_with_source(&path);
                if let Ok(mut cache) = cache.lock() {
                    cache.finish_request(&path);
                    if generation.load(Ordering::Acquire) == requested_generation {
                        if let Ok((clip, _)) = result {
                            cache.insert(path, Arc::new(clip));
                        }
                    }
                }
                if closed.load(Ordering::Acquire) {
                    break;
                }
            }
        })
        .expect("spawn prefetch worker")
}

struct PrefetchCache {
    entries: HashMap<String, (Arc<DecodedAudio>, Instant, u64)>,
    pending: std::collections::HashSet<String>,
    bytes: u64,
}

impl PrefetchCache {
    const MAX_NEIGHBORS: usize = 4;
    const MAX_ENTRIES: usize = Self::MAX_NEIGHBORS + 1;
    /// The LRU budget is independent from the decoder's per-clip safety valve.
    const MAX_BYTES: u64 = 256 * 1024 * 1024;

    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            pending: std::collections::HashSet::new(),
            bytes: 0,
        }
    }

    fn request(&mut self, path: &str) -> bool {
        if self.entries.contains_key(path) || !self.pending.insert(path.to_string()) {
            return false;
        }
        true
    }

    fn finish_request(&mut self, path: &str) {
        self.pending.remove(path);
    }

    fn get(&mut self, path: &str) -> Option<Arc<DecodedAudio>> {
        let (clip, used, _) = self.entries.get_mut(path)?;
        *used = Instant::now();
        Some(Arc::clone(clip))
    }

    fn insert(&mut self, path: String, clip: Arc<DecodedAudio>) {
        let size = clip.bytes.len() as u64
            + clip.source.as_ref().map_or(0, |source| {
                (source.samples.len() * std::mem::size_of::<f32>()) as u64
            });
        self.insert_with_size(path, clip, size);
    }

    fn insert_with_size(&mut self, path: String, clip: Arc<DecodedAudio>, size: u64) {
        if size > Self::MAX_BYTES {
            return;
        }
        if let Some((_, _, old_size)) = self.entries.remove(&path) {
            self.bytes = self.bytes.saturating_sub(old_size);
        }
        while self.entries.len() >= Self::MAX_ENTRIES
            || self.bytes.saturating_add(size) > Self::MAX_BYTES
        {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, used, _))| *used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some((_, _, old_size)) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(old_size);
            }
        }
        self.bytes = self.bytes.saturating_add(size);
        self.entries.insert(path, (clip, Instant::now(), size));
    }
}

fn decode_fingerprint(path: &str) -> Option<(u64, u128)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some((metadata.len(), modified.as_nanos()))
}

/// Strip the characters Windows refuses in a file name.
///
/// An exported region is named after the source file, and sample libraries carry
/// names like `kick: tight/01` that are legal on disk but not in a component of a
/// path. Replacing rather than dropping keeps the name readable.
fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    // Windows also refuses a trailing dot or space. Trimmed at both ends: a sample
    // library does contain names that begin with a space, and an exported file
    // called " kick.wav" is a nuisance in a DAW's clip list.
    cleaned.trim_matches(['.', ' ']).to_string()
}

/// Decide whether a declared file can be resident before opening the decoder.
///
/// The safety valve in the engine remains necessary for dishonest/truncated
/// headers, but ordinary long files should enter the window path without first
/// allocating hundreds of megabytes and discovering the problem late.
fn full_decode_bytes(meta: &mio_index::AudioMeta, format: OutputFormat) -> Option<usize> {
    let seconds = meta.duration_s.filter(|value| value.is_finite() && *value > 0.0)?;
    let source_rate = meta.sample_rate? as f64;
    let source_channels = meta.channels? as usize;
    let frames = (seconds * source_rate).ceil() as usize;
    let source = frames
        .saturating_mul(source_channels.max(1))
        .saturating_mul(std::mem::size_of::<f32>());
    let device = ((frames as f64 * format.sample_rate as f64 / source_rate).ceil() as usize)
        .saturating_mul(format.frame_bytes());
    Some(source.saturating_add(device))
}

#[cfg(test)]
mod state_tests {
    use super::*;

    fn clip(path: &str, bytes: usize) -> Arc<DecodedAudio> {
        Arc::new(DecodedAudio {
            bytes: vec![0; bytes],
            frames: bytes / 4,
            total_frames: bytes / 4,
            format: OutputFormat {
                sample_rate: 48_000,
                channels: 1,
                sample: mio_engine::DeviceSample::F32,
            },
            source_rate: 48_000,
            source_channels: 1,
            decode_ms: 0.0,
            resampled: false,
            path: path.into(),
            trimmed_leading_frames: 0,
            source: None,
            complete: true,
            total_frames_known: true,
            window_start_frames: 0,
        })
    }

    #[test]
    fn prefetch_cache_enforces_entry_and_byte_limits_and_updates_lru() {
        let mut cache = PrefetchCache::new();
        for index in 0..PrefetchCache::MAX_ENTRIES {
            cache.insert(format!("{index}"), clip(&index.to_string(), 1024));
        }
        assert!(cache.get("0").is_some());
        cache.insert("new".into(), clip("new", 1024));
        assert_eq!(cache.entries.len(), PrefetchCache::MAX_ENTRIES);
        assert!(cache.get("0").is_some(), "recently used entries stay hot");
        assert!(
            cache.get("1").is_none(),
            "least recently used entry is evicted"
        );

        cache.insert_with_size(
            "oversized".into(),
            clip("oversized", 1),
            PrefetchCache::MAX_BYTES + 1,
        );
        assert!(cache.get("oversized").is_none());
    }

    #[test]
    fn window_decode_retry_recovers_after_one_transient_failure() {
        let mut calls = 0;
        let value = retry_decode(|| {
            calls += 1;
            if calls == 1 { Err("transient") } else { Ok(42) }
        });
        assert_eq!(value, Ok(42));
        assert_eq!(calls, 2);
    }

    #[test]
    fn window_decode_retry_stops_after_two_failures() {
        let mut calls = 0;
        let value: Result<i32, &str> = retry_decode(|| {
            calls += 1;
            Err("broken")
        });
        assert_eq!(value, Err("broken"));
        assert_eq!(calls, 2);
    }
}
