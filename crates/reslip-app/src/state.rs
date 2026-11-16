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

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use reslip_engine::waveform::DEFAULT_BUCKETS;
use reslip_engine::{
    DecodedAudio, Decoder, Engine, EngineConfig, OutputFormat, PlaybackMonitor, PublishSource,
};

use crate::commands::{
    AppStatus, FolderNodeDto, LibraryStatus, PlaybackPosition, SearchRequest, SoundDto,
    WaveformDto,
};
use crate::error::{AppError, AppResult};
use crate::library::Library;

/// What the app learns about audio at startup and never afterwards.
#[derive(Debug, Clone)]
pub struct EngineInfo {
    pub device_name: String,
    pub format: OutputFormat,
    pub endpoint_frames: u32,
    pub engine_period_ms: f64,
}

/// A decode request handed to the worker.
struct DecodeJob {
    path: String,
    reply: Sender<AppResult<Arc<DecodedAudio>>>,
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
    /// `None` when no render device could be opened. The window still runs; the
    /// status command reports why playback is unavailable.
    engine: Option<Engine>,
    monitor: Option<PlaybackMonitor>,
    engine_error: Option<String>,
    info: Option<EngineInfo>,
    device_filter: Option<String>,

    decoder: Decoder,
    jobs: Arc<LatestSlot>,
    worker: Mutex<Option<JoinHandle<()>>>,

    library: Mutex<Option<Arc<Library>>>,

    /// The last decoded clip, so replaying the same sound is a publish rather
    /// than a decode. Keyed by path so a different sound never reuses it.
    last_clip: Mutex<Option<(String, Arc<DecodedAudio>)>>,

    /// Publishes the render thread never reached (superseded inside one engine
    /// period). Diagnostic for the status bar.
    dropped_publishes: AtomicU64,

    /// Bumped on every play/stop so a decode that finishes late can be discarded
    /// instead of starting stale audio.
    play_epoch: AtomicU64,
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
        let opened = Engine::start(EngineConfig {
            device_filter: device_filter.clone(),
            buffer_duration_hns: 0,
        });

        let (engine, monitor, info, engine_error) = match opened {
            Ok(engine) => {
                let monitor = engine.monitor();
                let info = EngineInfo {
                    device_name: monitor.device_name().to_string(),
                    format: monitor.format(),
                    endpoint_frames: monitor.endpoint_frames(),
                    engine_period_ms: monitor.engine_period_ms(),
                };
                (Some(engine), Some(monitor), Some(info), None)
            }
            Err(e) => (None, None, None, Some(e.to_string())),
        };

        let format = info
            .as_ref()
            .map(|i| i.format)
            .unwrap_or(OutputFormat {
                sample_rate: 48_000,
                channels: 2,
                sample: reslip_engine::DeviceSample::F32,
            });

        let jobs = Arc::new(LatestSlot::new());
        let worker = spawn_decoder(Arc::clone(&jobs), Decoder::new(format));

        Self {
            engine,
            monitor,
            engine_error,
            info,
            device_filter,
            decoder: Decoder::new(format),
            jobs,
            worker: Mutex::new(Some(worker)),
            library: Mutex::new(None),
            last_clip: Mutex::new(None),
            dropped_publishes: AtomicU64::new(0),
            play_epoch: AtomicU64::new(0),
        }
    }

    pub fn engine_info(&self) -> Option<&EngineInfo> {
        self.info.as_ref()
    }

    pub fn engine_error(&self) -> Option<&str> {
        self.engine_error.as_deref()
    }

    pub fn device_filter(&self) -> Option<&str> {
        self.device_filter.as_deref()
    }

    pub fn monitor(&self) -> Option<&PlaybackMonitor> {
        self.monitor.as_ref()
    }

    pub fn decoder(&self) -> &Decoder {
        &self.decoder
    }

    pub fn output_format(&self) -> OutputFormat {
        self.decoder.format()
    }

    fn engine(&self) -> AppResult<&Engine> {
        self.engine.as_ref().ok_or_else(|| {
            AppError::NoAudioDevice(self.engine_error.clone().unwrap_or_default())
        })
    }

    /// Publish a decoded clip and return the publish sequence number.
    pub fn publish(&self, clip: Arc<DecodedAudio>, source: PublishSource) -> AppResult<u64> {
        Ok(self.engine()?.publish(clip, source))
    }

    /// Publish starting `start_at` seconds in. A seek is a starting offset on an
    /// immutable buffer, so it costs the same atomic store a normal start does.
    pub fn publish_at(&self, clip: Arc<DecodedAudio>, start_at: f64) -> AppResult<u64> {
        let offset = (start_at.max(0.0) * self.output_format().sample_rate as f64) as u64;
        Ok(self.engine()?.publish_from(clip, offset))
    }

    pub fn stop_playback(&self) -> AppResult<()> {
        self.play_epoch.fetch_add(1, Ordering::AcqRel);
        self.engine()?.stop();
        Ok(())
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
    pub fn request_decode(&self, path: String) -> AppResult<Receiver<AppResult<Arc<DecodedAudio>>>> {
        let (tx, rx) = channel();
        // A full slot is impossible (one job, replaced), so a failed send means
        // the worker is gone.
        match try_submit(&self.jobs, DecodeJob { path, reply: tx }) {
            Ok(()) => Ok(rx),
            Err(_) => Err(AppError::internal("decode worker is not running")),
        }
    }

    pub fn library(&self) -> AppResult<Arc<Library>> {
        self.library
            .lock()
            .map_err(|_| AppError::internal("library lock poisoned"))?
            .clone()
            .ok_or(AppError::NoLibrary)
    }

    pub fn set_library(&self, library: Option<Arc<Library>>) -> AppResult<()> {
        let mut slot = self
            .library
            .lock()
            .map_err(|_| AppError::internal("library lock poisoned"))?;
        *slot = library;
        Ok(())
    }

    /// Root folder of the open library, if any.
    pub fn library_root(&self) -> AppResult<PathBuf> {
        Ok(self.library()?.root().to_path_buf())
    }

    // ------------------------------------------------------- IPC-facing logic
    //
    // These are what the Tauri commands call. They live here, not in
    // `commands.rs`, so an integration test can drive them without a window.

    /// Everything the window needs at startup.
    pub fn status(&self) -> AppResult<AppStatus> {
        let info = self.engine_info();
        let library = self.library().ok();
        Ok(AppStatus {
            audio_available: info.is_some(),
            audio_error: self.engine_error().map(str::to_string),
            device_name: info.map(|i| i.device_name.clone()),
            sample_rate: info.map(|i| i.format.sample_rate),
            channels: info.map(|i| i.format.channels),
            sample_format: info.map(|i| i.format.sample.label().to_string()),
            endpoint_frames: info.map(|i| i.endpoint_frames),
            engine_period_ms: info.map(|i| i.engine_period_ms),
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
        if !reslip_index::is_audio_path(std::path::Path::new(path)) {
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
                    finished: snap.finished,
                }
            }
            None => PlaybackPosition {
                position_s: 0.0,
                duration_s: 0.0,
                playing: false,
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
        if !reslip_index::is_audio_path(std::path::Path::new(path)) {
            return Err(AppError::NotAudio(path.to_string()));
        }
        let decoder = self.decoder().clone();
        let buckets = buckets.unwrap_or(DEFAULT_BUCKETS).clamp(16, 16_384);
        let owned = path.to_string();

        let started = Instant::now();
        let scan = tauri::async_runtime::spawn_blocking(move || {
            reslip_engine::waveform::Waveform::scan(&decoder, &owned, buckets)
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

    /// Open a library folder and start an incremental pass.
    ///
    /// Re-opening the same folder does not rebuild anything: it returns the
    /// current status and starts a pass only if none is running.
    pub fn open_library(&self, root: &str) -> AppResult<LibraryStatus> {
        let path = PathBuf::from(root);
        if !path.is_dir() {
            return Err(AppError::internal(format!("not a folder: {root}")));
        }

        if let Ok(existing) = self.library() {
            if existing.root() == path.as_path() {
                existing.reindex()?;
                return crate::commands::library_status(&existing);
            }
        }

        let library = Arc::new(Library::open(&path)?);
        library.reindex()?;
        let status = crate::commands::library_status(&library)?;
        self.set_library(Some(library))?;
        Ok(status)
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

    pub fn list_folders(&self) -> AppResult<FolderNodeDto> {
        Ok(self.library()?.folder_tree()?.into())
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
fn spawn_decoder(jobs: Arc<LatestSlot>, decoder: Decoder) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("reslip-decode".into())
        .spawn(move || {
            while let Some(job) = jobs.take() {
                let clip = decoder.decode(&job.path).map(Arc::new).map_err(AppError::from);
                // A dropped receiver means the command was cancelled (the window
                // closed, or a newer request superseded it). Either way the decode
                // result is not needed.
                let _ = job.reply.send(clip);
            }
        })
        .expect("spawn decode worker")
}
