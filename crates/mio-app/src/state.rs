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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use mio_engine::waveform::DEFAULT_BUCKETS;
use mio_engine::{
    Backend, DecodedAudio, Decoder, Engine, EngineConfig, OutputFormat, PlaybackMonitor,
    PublishSource,
};

use crate::audio_config::{AudioConfig, AudioStore};
use crate::commands::{
    AppStatus, AudioDeviceDto, FolderNodeDto, LibraryStatus, PlaybackPosition, SearchRequest,
    SoundDto, WaveformDto,
};
use crate::error::{AppError, AppResult};
use crate::history::History;
use crate::library::Library;
use crate::session::{Session, SessionStore};

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
    reply: Sender<AppResult<Arc<DecodedAudio>>>,
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
    audio: Mutex<AudioSlot>,

    jobs: Arc<LatestSlot>,
    worker: Mutex<Option<JoinHandle<()>>>,

    /// Every open library, keyed by root. Several at once is the point: a sample
    /// collection is usually split across packs, and switching folders should not
    /// mean reopening and reindexing.
    libraries: Mutex<LibrarySet>,

    /// The last decoded clip, so replaying the same sound is a publish rather
    /// than a decode. Keyed by path so a different sound never reuses it.
    last_clip: Mutex<Option<(String, Arc<DecodedAudio>)>>,

    /// Publishes the render thread never reached (superseded inside one engine
    /// period). Diagnostic for the status bar.
    dropped_publishes: AtomicU64,

    /// Bumped on every play/stop so a decode that finishes late can be discarded
    /// instead of starting stale audio.
    play_epoch: AtomicU64,

    /// The output gain, mirrored outside the audio lock.
    ///
    /// The engine holds the authoritative copy inside its `Shared` block, but that
    /// copy is only reachable while a device is open. Keeping the number here means
    /// a slider moved while the device is gone is remembered, and reading it for the
    /// UI never has to wait on the audio mutex.
    gain_bits: AtomicU32,

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
        let worker = spawn_decoder(Arc::clone(&jobs));

        Self {
            audio: Mutex::new(audio),
            jobs,
            worker: Mutex::new(Some(worker)),
            libraries: Mutex::new(LibrarySet::default()),
            last_clip: Mutex::new(None),
            dropped_publishes: AtomicU64::new(0),
            play_epoch: AtomicU64::new(0),
            gain_bits: AtomicU32::new(1.0f32.to_bits()),
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
            set.active = session.first_open().filter(|root| {
                set.libs.iter().any(|l| l.root() == root.as_path())
            });
            if set.active.is_none() {
                set.active = set.libs.first().map(|l| l.root().to_path_buf());
            }
        }

        if let Ok(mut slot) = self.libraries.lock() {
            *slot = set;
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
        let probing_asio = !matches!(
            self.audio.lock().map(|a| a.backend),
            Ok(Backend::Asio)
        );
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
        self.audio
            .lock()
            .map(|a| a.decoder())
            .unwrap_or_else(|_| Decoder::new(OutputFormat {
                sample_rate: 48_000,
                channels: 2,
                sample: mio_engine::DeviceSample::F32,
            }))
    }

    pub fn output_format(&self) -> OutputFormat {
        self.audio
            .lock()
            .map(|a| a.format)
            .unwrap_or(OutputFormat {
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
        let offset = (start_at.max(0.0) * self.output_format().sample_rate as f64) as u64;
        self.with_engine(|engine| engine.publish_from(clip, offset))
    }

    pub fn stop_playback(&self) -> AppResult<()> {
        self.play_epoch.fetch_add(1, Ordering::AcqRel);
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

    /// Invalidate any decode in flight. Called when playback is stopped or when a
    /// newer play request arrives.
    pub fn bump_epoch(&self) -> u64 {
        self.play_epoch.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn epoch(&self) -> u64 {
        self.play_epoch.load(Ordering::Acquire)
    }

    /// The clip for `path` if it is already decoded.
    pub fn cached_clip(&self, path: &str) -> Option<Arc<DecodedAudio>> {
        let slot = self.last_clip.lock().ok()?;
        let (cached_path, clip) = slot.as_ref()?;
        (cached_path == path).then(|| Arc::clone(clip))
    }

    pub fn remember_clip(&self, path: String, clip: Arc<DecodedAudio>) {
        if let Ok(mut slot) = self.last_clip.lock() {
            *slot = Some((path, clip));
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
    pub fn request_decode(&self, path: String) -> AppResult<Receiver<AppResult<Arc<DecodedAudio>>>> {
        let (tx, rx) = channel();
        let decoder = self.decoder();
        // A full slot is impossible (one job, replaced), so a failed send means
        // the worker is gone.
        match try_submit(
            &self.jobs,
            DecodeJob {
                path,
                decoder,
                reply: tx,
            },
        ) {
            Ok(()) => Ok(rx),
            Err(_) => Err(AppError::internal("decode worker is not running")),
        }
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
        let backend = info.as_ref().map(|i| i.backend).unwrap_or_else(|| self.backend());
        Ok(AppStatus {
            audio_available: info.is_some(),
            audio_error: self.engine_error(),
            device_name: info.as_ref().map(|i| i.device_name.clone()),
            backend: backend.label().to_string(),
            sample_rate: info.as_ref().map(|i| i.format.sample_rate),
            channels: info.as_ref().map(|i| i.format.channels),
            sample_format: info
                .as_ref()
                .map(|i| i.format.sample.label().to_string()),
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
    pub async fn play(&self, path: &str, start_at: Option<f64>) -> AppResult<()> {
        if !mio_index::is_audio_path(std::path::Path::new(path)) {
            return Err(AppError::NotAudio(path.to_string()));
        }
        // Any decode already in flight belongs to a request this one supersedes.
        let epoch = self.bump_epoch();

        if let Some(clip) = self.cached_clip(path) {
            return match start_at {
                Some(at) if at > 0.0 => self.publish_at(clip, at).map(|_| ()),
                _ => self.publish(clip, PublishSource::Initial).map(|_| ()),
            };
        }

        let receiver = self.request_decode(path.to_string())?;
        let clip = tauri::async_runtime::spawn_blocking(move || receiver.recv())
            .await
            .map_err(|e| AppError::internal(format!("decode task failed: {e}")))?
            .map_err(|_| AppError::internal("decode worker closed before answering"))??;

        // A newer play or a stop arrived while this decoded. Dropping the result
        // is correct: starting it now would play a sound the user already left.
        if self.epoch() != epoch {
            return Ok(());
        }

        self.remember_clip(path.to_string(), Arc::clone(&clip));
        match start_at {
            Some(at) if at > 0.0 => self.publish_at(clip, at).map(|_| ()),
            _ => self.publish(clip, PublishSource::Initial).map(|_| ()),
        }
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
                }
            }
            None => PlaybackPosition {
                position_s: 0.0,
                duration_s: 0.0,
                playing: false,
                paused: false,
                finished: true,
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

        let started = Instant::now();
        let scan = tauri::async_runtime::spawn_blocking(move || {
            mio_engine::waveform::Waveform::scan(&decoder, &owned, buckets)
        })
        .await
        .map_err(|e| AppError::internal(format!("waveform task failed: {e}")))?;
        let waveform = scan?;

        Ok(WaveformDto {
            peaks: waveform.peaks,
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
    pub fn add_library<F>(&self, root: &str, full: bool, progress: F) -> AppResult<LibraryStatus>
    where
        F: FnMut(usize, usize),
    {
        let path = PathBuf::from(root);
        if !path.is_dir() {
            return Err(AppError::internal(format!("not a folder: {root}")));
        }

        // Re-adding a folder that is already open just reindexes it: the user
        // means "refresh", not "open a second copy".
        let existing = self
            .libraries
            .lock()
            .map_err(|_| AppError::internal("library lock poisoned"))?
            .find(&path);

        let library = match existing {
            Some(library) => library,
            None => {
                let library = Arc::new(Library::open(&path)?);
                let mut set = self
                    .libraries
                    .lock()
                    .map_err(|_| AppError::internal("library lock poisoned"))?;
                set.libs.push(Arc::clone(&library));
                set.active = Some(path.clone());
                library
            }
        };

        library.index_blocking_with(full, progress)?;

        // Selecting the folder that was just indexed is what the user expects;
        // adding a second library should not silently leave the first one active.
        if let Ok(mut set) = self.libraries.lock() {
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
            set.libs.retain(|l| l.root() != path.as_path());
            if before.as_deref() == Some(path.as_path()) {
                set.active = set.libs.first().map(|l| l.root().to_path_buf());
            }
            set.active != before
        };
        self.persist_session();
        Ok(changed)
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
        progress: F,
    ) -> AppResult<LibraryStatus>
    where
        F: FnMut(usize, usize),
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
        library.index_blocking_with(full, progress)?;
        crate::commands::library_status(&library)
    }

    pub fn library_status(&self) -> AppResult<Option<LibraryStatus>> {
        match self.library() {
            Ok(lib) => Ok(Some(crate::commands::library_status(&lib)?)),
            Err(AppError::NoLibrary) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn search(
        &self,
        request: SearchRequest,
    ) -> AppResult<Vec<SoundDto>> {
        let library = self.library()?;
        let params = request.into_params()?;
        Ok(library
            .search(&params)?
            .into_iter()
            .map(SoundDto::from)
            .collect())
    }

    /// Every path the filter matches, for "select all".
    ///
    /// A separate query rather than `search` with a huge limit: `search` clamps at
    /// 5,000 rows because a table page has no reason to be bigger, and selecting
    /// everything is not a table page. Only paths come back, so a 280k selection
    /// is a few megabytes of strings rather than tens of megabytes of row DTOs.
    pub fn matching_paths(&self, request: SearchRequest) -> AppResult<Vec<String>> {
        let library = self.library()?;
        // `limit: 0` means "no limit" for this call specifically; the index's
        // clamp is for pages, and this is not a page.
        let mut params = request.into_params()?;
        params.limit = u32::MAX;
        Ok(library.search(&params)?.into_iter().map(|s| s.path).collect())
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
        let (favorites, total) = library.summary()?;
        Ok(crate::commands::SidebarDto {
            facets,
            favorites,
            total,
        })
    }

    /// Star or unstar many sounds in one transaction, recording it for undo.
    pub fn set_favorite(&self, paths: &[String], favorite: bool) -> AppResult<u64> {
        let library = self.library()?;
        let label = if favorite { "Add to Favorites" } else { "Remove from Favorites" };
        let edit = crate::history::run(&library, label, paths, || {
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
        let edit = crate::history::run(&library, "Set Category", paths, || {
            Ok(library.set_category(paths, category, subcategory)?)
        })?;
        self.record(edit);
        Ok(paths.len() as u64)
    }

    /// Drop the manual category on many sounds, recording for undo.
    pub fn clear_category(&self, paths: &[String]) -> AppResult<u64> {
        let library = self.library()?;
        let edit = crate::history::run(&library, "Clear Category", paths, || {
            Ok(library.clear_category(paths)?)
        })?;
        self.record(edit);
        Ok(paths.len() as u64)
    }

    fn record(&self, edit: crate::history::Edit) {
        if let Ok(mut history) = self.history.lock() {
            history.push(edit);
        }
    }

    /// Undo the last edit. Returns the label that was undone, if any.
    pub fn undo(&self) -> AppResult<Option<String>> {
        let libraries = self.all_libraries()?;
        let mut history = self
            .history
            .lock()
            .map_err(|_| AppError::internal("history lock poisoned"))?;
        history.undo(&libraries)
    }

    /// Redo the last undone edit.
    pub fn redo(&self) -> AppResult<Option<String>> {
        let libraries = self.all_libraries()?;
        let mut history = self
            .history
            .lock()
            .map_err(|_| AppError::internal("history lock poisoned"))?;
        history.redo(&libraries)
    }

    /// What undo/redo would do, for the menu labels.
    pub fn history_state(&self) -> (Option<String>, Option<String>) {
        let history = match self.history.lock() {
            Ok(h) => h,
            Err(_) => return (None, None),
        };
        (
            history.undo_label().map(str::to_string),
            history.redo_label().map(str::to_string),
        )
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
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
fn spawn_decoder(jobs: Arc<LatestSlot>) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("mio-decode".into())
        .spawn(move || {
            while let Some(job) = jobs.take() {
                let clip = job
                    .decoder
                    .decode(&job.path)
                    .map(Arc::new)
                    .map_err(AppError::from);
                // A dropped receiver means the command was cancelled (the window
                // closed, or a newer request superseded it). Either way the decode
                // result is not needed.
                let _ = job.reply.send(clip);
            }
        })
        .expect("spawn decode worker")
}
