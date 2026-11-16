//! WASAPI shared-mode output with dual-slot switching.
//!
//! Layout, from `docs/architecture.md` section 4:
//!
//! * A dedicated render thread owns every COM object. It has to: `IAudioClient`
//!   is `!Send`, so it can only be created on the thread that uses it.
//! * Two slots hold decoded clips. The render callback reads `active` and copies
//!   from that slot; nothing else happens there.
//! * Switching playback is one atomic store of the slot index. Old clips are
//!   released by `arc-swap`, which waits for the render thread's guard to be
//!   dropped before freeing, so the audio thread never deallocates a clip.
//!
//! The render callback allocates nothing, locks nothing, does no IO, and never
//! formats a string. Measurements leave it through a preallocated lock-free queue.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use arc_swap::ArcSwapOption;
use crossbeam_queue::ArrayQueue;
use wasapi::{
    AudioClient, AudioRenderClient, Device, DeviceEnumerator, Direction, Handle, StreamMode,
    WaveFormat, initialize_mta,
};

use crate::decode::{DecodedAudio, DeviceSample, OutputFormat};
use crate::error::{EngineError, Result};

/// Which audio backend the engine talks to.
///
/// WASAPI shared mode is the default and stays the default: it needs no driver
/// beyond what Windows ships, and it does not take the device away from a DAW.
/// ASIO is the option for users who want the callback period down to a few
/// milliseconds; it is compiled in only behind the `asio` feature because the
/// Steinberg SDK cannot be redistributed and building against it needs libclang.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Wasapi,
    Asio,
}

impl Backend {
    pub fn label(self) -> &'static str {
        match self {
            Backend::Wasapi => "WASAPI",
            Backend::Asio => "ASIO",
        }
    }

    /// Parse the wire name (`"wasapi"` / `"asio"`), case-insensitively.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "wasapi" | "shared" | "" => Some(Backend::Wasapi),
            "asio" => Some(Backend::Asio),
            _ => None,
        }
    }
}

/// One publish → audible observation, produced by the render thread.
#[derive(Debug, Clone, Copy)]
pub struct LatencyReport {
    /// Which publish this belongs to, starting at 1.
    pub seq: u64,
    /// Publish → the callback wrote the first frame of that clip.
    pub to_write_ms: f64,
    /// Audio already queued in the endpoint when we wrote. On shared mode this is
    /// the ~12ms the application cannot get rid of, so it dominates the total.
    pub queued_ms: f64,
    /// `to_write_ms + queued_ms`: publish → first sample reaches the DAC.
    pub to_audible_ms: f64,
}

/// Who initiated a publish. Reporting only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishSource {
    Initial,
    Switch,
}

/// Playback position, read without touching the audio thread's slots.
///
/// `position_s` is where the render thread has *written* to, which runs ahead of
/// what the listener hears by the endpoint queue depth (~12ms shared). Correct
/// for a playhead; not a sync reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackSnapshot {
    pub position_s: f64,
    pub duration_s: f64,
    pub playing: bool,
    /// Paused with the clip still loaded. The playhead stays where it is, so
    /// resuming costs nothing and the position does not jump back to zero.
    pub paused: bool,
    pub finished: bool,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Which backend to open. WASAPI shared mode unless the user asked for ASIO
    /// in the settings dialog.
    pub backend: Backend,
    /// Case-insensitive substring of the render device's friendly name. `None`
    /// uses the system default — on this machine that is a virtual device
    /// (FxSound), whose latency numbers mean nothing.
    pub device_filter: Option<String>,
    /// Endpoint buffer duration in 100 ns units. 0 asks for the device default,
    /// which shared mode pins to the engine period regardless of the request.
    pub buffer_duration_hns: i64,
    /// ASIO callback block size in frames. `None` asks the driver for its
    /// preferred size, which is the only safe default: an ASIO driver rejects a
    /// size outside its own min/max range and some report a granularity that
    /// makes an arbitrary request fail.
    pub asio_buffer_frames: Option<u32>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            backend: Backend::Wasapi,
            device_filter: None,
            buffer_duration_hns: 0,
            asio_buffer_frames: None,
        }
    }
}

pub(crate) struct Shared {
    slots: [ArcSwapOption<DecodedAudio>; 2],
    active: AtomicUsize,
    pub_ns: AtomicU64,
    pub_seq: AtomicU64,
    pos_frames: AtomicU64,
    /// Frame count of the clip in the active slot, published alongside it so a
    /// position poll never has to take an `arc-swap` guard on the audio thread's
    /// slot. Pure atomics on the polling path (architecture section 7.3).
    active_frames: AtomicU64,
    /// Set by `pause`, cleared by `publish` and `resume`. While set the render
    /// callback writes silence and leaves `pos_frames` alone, so the playhead
    /// freezes exactly where the listener heard it stop.
    paused: AtomicBool,
    finished: AtomicBool,
    running: AtomicBool,
    measurements: ArrayQueue<LatencyReport>,
    /// Callbacks where the endpoint had run dry, i.e. audible gaps. Any non-zero
    /// count during a switch test means the switch glitched.
    underruns: AtomicU64,
    /// Callbacks served while the active slot was empty (silence by design
    /// between clips, counted separately from real underruns).
    silent_callbacks: AtomicU64,
    t0: Instant,
}

impl Shared {
    fn now_ns(&self) -> u64 {
        self.t0.elapsed().as_nanos() as u64
    }

    /// Whether the render thread should keep going. The WASAPI loop polls the
    /// flag directly; the ASIO thread has no loop of its own, so it needs a way
    /// in, and it only exists when ASIO was compiled in.
    #[cfg(feature = "asio")]
    pub(crate) fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
}

/// The slot-reading half of a render callback, shared by both backends.
///
/// It owns the scratch buffer and the last sequence number it saw, so it can be
/// moved into a callback closure and used with `&mut self` — no lock, no
/// allocation, no atomics beyond the ones already in [`Shared`]. Both backends
/// produce interleaved bytes in the device's own sample format; WASAPI writes
/// them straight out, ASIO splits them into per-channel planes.
pub(crate) struct Mixer {
    scratch: Vec<u8>,
    seen_seq: u64,
}

impl Mixer {
    pub(crate) fn new(max_frames: usize, frame_bytes: usize) -> Self {
        Self {
            scratch: vec![0u8; max_frames.max(1) * frame_bytes.max(1)],
            seen_seq: 0,
        }
    }

    /// The whole scratch buffer, for a backend that writes it out itself.
    pub(crate) fn scratch(&self) -> &[u8] {
        &self.scratch
    }

    /// Copy the active slot's next `want_frames` frames into the scratch buffer
    /// and return how many bytes of real audio that was.
    ///
    /// Everything after the returned length is silence, written in place. A
    /// paused clip, a finished clip, an empty slot and a clip whose format does
    /// not match the device all produce zero filled bytes — the callback still
    /// writes silence, it just has nothing to copy.
    ///
    /// `queued_frames` is the device's already-queued depth in frames, used only
    /// to complete the latency report. WASAPI can ask the endpoint; ASIO has no
    /// such query and passes the driver's own reported output latency.
    pub(crate) fn fill(
        &mut self,
        shared: &Shared,
        want_frames: usize,
        frame_bytes: usize,
        queued_frames: u32,
    ) -> usize {
        let want_bytes = want_frames * frame_bytes;
        if want_bytes > self.scratch.len() {
            // The device asked for more than the buffer it reported at startup.
            // Writing silence is correct; resizing here would allocate in the
            // callback, which is the one thing this must never do.
            self.scratch.fill(0);
            return 0;
        }

        // Read the slot before the sequence number: `publish` writes the clip and
        // the timestamp before flipping `active`, so seeing a new sequence here
        // guarantees the clip in this slot is the one that publish installed.
        let active = shared.active.load(Ordering::Acquire) & 1;
        let seq = shared.pub_seq.load(Ordering::Acquire);
        let guard = shared.slots[active].load();
        let clip = guard.as_ref();

        if clip.is_none() {
            shared.silent_callbacks.fetch_add(1, Ordering::Relaxed);
        }

        let mut filled = 0usize;

        if let Some(clip) = clip {
            let pos = shared.pos_frames.load(Ordering::Acquire) as usize;
            let paused = shared.paused.load(Ordering::Acquire);
            let clip_frame_bytes = clip.frame_bytes();

            if paused {
                // Paused: leave `pos_frames` where it is. The clip stays in the
                // slot so resuming is one flag flip, and the position the
                // listener hears is the position the playhead shows.
            } else if clip_frame_bytes == frame_bytes && pos < clip.frames {
                let n = (clip.frames - pos).min(want_frames);
                let start = pos * clip_frame_bytes;
                self.scratch[..n * clip_frame_bytes]
                    .copy_from_slice(&clip.bytes[start..start + n * clip_frame_bytes]);
                filled = n * clip_frame_bytes;
                shared.pos_frames.store((pos + n) as u64, Ordering::Release);

                // The first callback able to write this publish is the moment the
                // click becomes audio, so that is what gets measured.
                if seq != self.seen_seq {
                    let pub_ns = shared.pub_ns.load(Ordering::Relaxed);
                    let now = shared.now_ns();
                    let sr = clip.format.sample_rate as f64;
                    let queued_ms = queued_frames as f64 / sr * 1000.0;
                    let to_write_ms = now.saturating_sub(pub_ns) as f64 / 1e6;
                    // A full queue drops the sample; acceptable for a diagnostic
                    // and it must never block the callback.
                    let _ = shared.measurements.push(LatencyReport {
                        seq,
                        to_write_ms,
                        queued_ms,
                        to_audible_ms: to_write_ms + queued_ms,
                    });
                }

                if pos + n >= clip.frames {
                    shared.finished.store(true, Ordering::Release);
                }
            }
        }
        self.seen_seq = seq;

        // Silence for whatever the clip did not cover. Written in place, so the
        // callback never allocates.
        self.scratch[filled..want_bytes].fill(0);
        filled
    }
}

/// Live playback engine. Dropping it stops the stream and joins the render thread.
pub struct Engine {
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
    format: OutputFormat,
    device_name: String,
    endpoint_frames: u32,
    backend: Backend,
    started: Instant,
}

/// A read-only handle to a running engine, cheap to clone and safe to poll.
///
/// This exists so the webview's playhead (`get_playback_position`, called at
/// 60fps) never has to reach the playback control thread. Every method here is
/// atomics only: no lock, no allocation, no round trip. The control thread owns
/// the [`Engine`]; the UI owns a `PlaybackMonitor`.
#[derive(Clone)]
pub struct PlaybackMonitor {
    shared: Arc<Shared>,
    format: OutputFormat,
    device_name: String,
    endpoint_frames: u32,
    backend: Backend,
}

impl PlaybackMonitor {
    pub fn snapshot(&self) -> PlaybackSnapshot {
        let position = self.shared.pos_frames.load(Ordering::Acquire);
        let frames = self.shared.active_frames.load(Ordering::Acquire);
        let paused = self.shared.paused.load(Ordering::Acquire);
        let finished = self.shared.finished.load(Ordering::Acquire);
        PlaybackSnapshot {
            position_s: position as f64 / self.format.sample_rate as f64,
            duration_s: frames as f64 / self.format.sample_rate as f64,
            // A paused clip is loaded and parked: still "playing" in the sense of
            // having a live slot, but the playhead must not advance and Space must
            // resume it rather than restart it.
            playing: frames > 0 && !finished && !paused,
            paused: frames > 0 && !finished && paused,
            finished,
        }
    }

    pub fn format(&self) -> OutputFormat {
        self.format
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn endpoint_frames(&self) -> u32 {
        self.endpoint_frames
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// One callback period, in milliseconds.
    ///
    /// WASAPI shared mode: the endpoint buffer is two engine periods, so the
    /// period is half of it. ASIO: one callback fills exactly one driver buffer,
    /// so the period *is* the buffer — which is why the number drops from ~10ms
    /// to ~5ms or less without changing anything else.
    pub fn engine_period_ms(&self) -> f64 {
        let frames = match self.backend {
            Backend::Wasapi => self.endpoint_frames as f64 / 2.0,
            Backend::Asio => self.endpoint_frames as f64,
        };
        frames / self.format.sample_rate as f64 * 1000.0
    }

    pub fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Acquire)
    }

    pub fn silent_callbacks(&self) -> u64 {
        self.shared.silent_callbacks.load(Ordering::Acquire)
    }

    /// Take every latency observation so far. Used by the status bar to show the
    /// measured `publish → 出声` of the last publish, not a documented target.
    pub fn drain_reports(&self) -> Vec<LatencyReport> {
        let mut out = Vec::new();
        while let Some(r) = self.shared.measurements.pop() {
            out.push(r);
        }
        out
    }
}

impl Engine {
    /// A polling handle for the UI. Cloning the engine's `Arc<Shared>` costs one
    /// atomic increment and keeps the measurement queue shared with the render
    /// thread, so reports drained here are the same ones the engine produces.
    pub fn monitor(&self) -> PlaybackMonitor {
        PlaybackMonitor {
            shared: Arc::clone(&self.shared),
            format: self.format,
            device_name: self.device_name.clone(),
            endpoint_frames: self.endpoint_frames,
            backend: self.backend,
        }
    }
}

impl Engine {
    /// Open the device and start the persistent render thread.
    ///
    /// The device format is discovered on the render thread (it owns the COM
    /// objects), so this blocks until the stream is live and the format known.
    /// That happens once, at startup.
    pub fn start(config: EngineConfig) -> Result<Self> {
        let shared = Arc::new(Shared {
            slots: [const { ArcSwapOption::const_empty() }; 2],
            active: AtomicUsize::new(0),
            pub_ns: AtomicU64::new(0),
            pub_seq: AtomicU64::new(0),
            pos_frames: AtomicU64::new(0),
            active_frames: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            running: AtomicBool::new(true),
            measurements: ArrayQueue::new(4096),
            underruns: AtomicU64::new(0),
            silent_callbacks: AtomicU64::new(0),
            t0: Instant::now(),
        });

        let (tx, rx) = sync_channel::<Result<StreamInfo>>(1);
        let audio_shared = Arc::clone(&shared);
        let buffer_hns = config.buffer_duration_hns;
        let filter = config.device_filter.clone();
        let backend = config.backend;
        // Only the ASIO arm reads this; the WASAPI arm ignores it.
        #[cfg_attr(not(feature = "asio"), allow(unused_variables))]
        let asio_frames = config.asio_buffer_frames;

        let handle = thread::Builder::new()
            .name("reslip-render".into())
            .spawn(move || match backend {
                Backend::Wasapi => render_thread(audio_shared, filter, buffer_hns, tx),
                #[cfg(feature = "asio")]
                Backend::Asio => crate::asio::render_thread(audio_shared, filter, asio_frames, tx),
                #[cfg(not(feature = "asio"))]
                Backend::Asio => {
                    let _ = tx.send(Err(EngineError::Unsupported(
                        "this build has no ASIO support; rebuild with --features asio".into(),
                    )));
                }
            })
            .map_err(EngineError::Io)?;

        let info = rx.recv().map_err(|_| {
            EngineError::Audio("render thread exited before reporting its format".into())
        })??;

        Ok(Self {
            shared,
            handle: Some(handle),
            format: info.format,
            device_name: info.device_name,
            endpoint_frames: info.endpoint_frames,
            backend,
            started: Instant::now(),
        })
    }

    pub fn format(&self) -> OutputFormat {
        self.format
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Endpoint buffer size in frames. Two engine periods on shared mode.
    pub fn endpoint_frames(&self) -> u32 {
        self.endpoint_frames
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// One callback period, in milliseconds. See [`PlaybackMonitor`] for why the
    /// two backends divide differently.
    pub fn engine_period_ms(&self) -> f64 {
        let frames = match self.backend {
            Backend::Wasapi => self.endpoint_frames as f64 / 2.0,
            Backend::Asio => self.endpoint_frames as f64,
        };
        frames / self.format.sample_rate as f64 * 1000.0
    }

    /// Publish a clip for playback and return its sequence number.
    ///
    /// The clip goes into the inactive slot, then the active index flips. That
    /// single atomic store is the whole switch: the render thread cannot observe
    /// a half-installed clip, and there is no queue to drain.
    ///
    /// Takes `&self` on purpose. Everything it touches is an atomic store or an
    /// `arc-swap` store, so a `play` command arriving on any thread can publish
    /// without a lock and without waiting for a control thread to become free.
    pub fn publish(&self, clip: Arc<DecodedAudio>, _source: PublishSource) -> u64 {
        self.publish_from(clip, 0)
    }

    /// Publish a clip and start it `offset_frames` in.
    ///
    /// `start_at` in the IPC surface lands here. The clip is not re-decoded or
    /// trimmed: the offset is a starting position on an immutable buffer, so a
    /// seek costs the same atomic store a normal start does (architecture 3.3 —
    /// every supported format is seekable from the header, so this never needs a
    /// decode from the top).
    pub fn publish_from(&self, clip: Arc<DecodedAudio>, offset_frames: u64) -> u64 {
        let current = self.shared.active.load(Ordering::Acquire);
        let next = 1 - current;

        let total = clip.frames as u64;
        let offset = offset_frames.min(total);

        self.shared.slots[next].store(Some(clip));
        self.shared.pos_frames.store(offset, Ordering::Release);
        // Total frames, not remaining: `snapshot` derives the clip duration from
        // it, and the playhead still starts at `offset`.
        self.shared.active_frames.store(total, Ordering::Release);
        self.shared.finished.store(false, Ordering::Release);
        // A new publish always starts audible: leaving `paused` set would play a
        // silent clip, which is the one state that looks like a broken engine.
        self.shared.paused.store(false, Ordering::Release);

        // Timestamp and sequence become visible before the flip, so the render
        // thread can trust both the moment it sees the new active index.
        let seq = self.shared.pub_seq.fetch_add(1, Ordering::AcqRel) + 1;
        self.shared
            .pub_ns
            .store(self.shared.now_ns(), Ordering::Relaxed);
        self.shared.active.store(next, Ordering::Release);
        seq
    }

    /// Stop playback now: clear both slots and park `finished`.
    ///
    /// This is the `stop` half of the IPC surface. It does not touch the stream,
    /// so the next `publish` costs one atomic store, same as any other switch.
    /// The endpoint keeps the audio it already queued (shared mode cannot drop
    /// it), which is why a stop is heard as a very short tail rather than a cut.
    pub fn stop(&self) {
        self.shared.active.store(0, Ordering::Release);
        self.shared.slots[0].store(None);
        self.shared.slots[1].store(None);
        self.shared.pos_frames.store(0, Ordering::Release);
        self.shared.active_frames.store(0, Ordering::Release);
        self.shared.paused.store(false, Ordering::Release);
        self.shared.finished.store(true, Ordering::Release);
    }

    /// Pause at the current position, keeping the clip loaded.
    ///
    /// `stop` clears the slots and parks the playhead at zero; that is the wrong
    /// behaviour for Space. Pause leaves both slots alone and only flips a flag,
    /// so the render callback starts writing silence and `pos_frames` stops
    /// moving. [`resume`](Self::resume) then costs one atomic store and continues
    /// from the exact frame the listener heard, with no decode and no re-seek.
    pub fn pause(&self) {
        self.shared.paused.store(true, Ordering::Release);
    }

    /// Continue a paused clip from where it stopped. A no-op when nothing is
    /// loaded, so a stray Space on an empty dock does not fabricate state.
    pub fn resume(&self) {
        if self.shared.active_frames.load(Ordering::Acquire) > 0
            && !self.shared.finished.load(Ordering::Acquire)
        {
            self.shared.paused.store(false, Ordering::Release);
        }
    }

    /// True while a clip is loaded and paused.
    pub fn paused(&self) -> bool {
        self.shared.paused.load(Ordering::Acquire)
    }

    /// Playback position of the active clip, in frames.
    ///
    /// This is where the render thread has *written* to, which runs ahead of what
    /// the listener hears by the endpoint queue depth (~12ms shared). Correct for
    /// drawing a playhead; not a sync reference.
    pub fn position_frames(&self) -> u64 {
        self.shared.pos_frames.load(Ordering::Acquire)
    }

    pub fn position_seconds(&self) -> f64 {
        self.position_frames() as f64 / self.format.sample_rate as f64
    }

    /// True once the active clip has been fully written to the endpoint.
    pub fn finished(&self) -> bool {
        self.shared.finished.load(Ordering::Acquire)
    }

    /// Everything the playhead needs, in one pass of atomic reads.
    pub fn snapshot(&self) -> PlaybackSnapshot {
        self.monitor().snapshot()
    }

    /// Take every latency observation the render thread has produced.
    pub fn drain_reports(&self) -> Vec<LatencyReport> {
        let mut out = Vec::new();
        while let Some(r) = self.shared.measurements.pop() {
            out.push(r);
        }
        out
    }

    /// Callbacks where playback had data to deliver but the endpoint was empty.
    /// Non-zero means an audible gap happened.
    pub fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Acquire)
    }

    /// Callbacks that wrote silence because no clip is playing. Not an error.
    pub fn silent_callbacks(&self) -> u64 {
        self.shared.silent_callbacks.load(Ordering::Acquire)
    }

    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.running.store(false, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

pub(crate) struct StreamInfo {
    pub(crate) format: OutputFormat,
    pub(crate) device_name: String,
    pub(crate) endpoint_frames: u32,
}
/// One output device, as the settings dialog lists it.
///
/// The two backends name their devices differently and that difference matters
/// to the user: a WASAPI endpoint is a Windows audio endpoint ("Speakers
/// (TOPPING Pro USB Audio)"), while an ASIO device is a driver ("TOPPING Pro USB
/// Audio Device") whose channel count and buffer size are properties of the
/// driver. One struct with the backend attached lets the UI draw one list.
#[derive(Debug, Clone)]
pub struct DeviceChoice {
    pub backend: Backend,
    /// Passed back as `EngineConfig::device_filter`. Empty means "the system
    /// default" for WASAPI; ASIO has no default, so the first driver is used.
    pub name: String,
    /// Human label for the picker. For ASIO this is the driver name; for WASAPI
    /// the endpoint's friendly name, same as `name`.
    pub label: String,
    /// Output channels, when known without opening the device. WASAPI reports
    /// this from the endpoint; ASIO only knows it after loading the driver.
    pub channels: Option<usize>,
    /// Callback period hint in frames, for ASIO drivers that report a preferred
    /// buffer size.
    pub buffer_frames: Option<u32>,
    /// True for the endpoint WASAPI would use by default.
    pub default: bool,
    /// Whether the device is believed usable. WASAPI endpoints are always `true`
    /// (enumerating one means it exists); an ASIO driver is `false` when a probe
    /// ran and failed, which is how "the interface is unplugged" reaches the
    /// picker.
    pub available: bool,
}

/// Every device both backends can offer.
///
/// Best-effort: a backend that is not compiled in, or one whose enumeration
/// fails, contributes nothing rather than failing the whole list. The settings
/// dialog needs to be able to say "ASIO: no driver installed" while still
/// listing the WASAPI endpoints.
///
/// `probe_asio` decides whether ASIO drivers are opened to read their channel
/// counts. Pass `false` while an ASIO driver is live in this process: probing
/// calls the SDK's `loadDriver`, which closes whatever driver is currently open,
/// so it would kill the stream it is supposed to describe.
pub fn list_devices(probe_asio: bool) -> Vec<DeviceChoice> {
    let mut out = Vec::new();

    let default_name = default_render_device_name();
    if let Ok(names) = list_render_devices() {
        for name in names {
            out.push(DeviceChoice {
                backend: Backend::Wasapi,
                label: name.clone(),
                default: default_name.as_deref() == Some(name.as_str()),
                name,
                channels: None,
                buffer_frames: None,
                available: true,
            });
        }
    }

    #[cfg(feature = "asio")]
    for driver in crate::asio::list_devices(probe_asio) {
        out.push(DeviceChoice {
            backend: Backend::Asio,
            label: driver.name.clone(),
            name: driver.name,
            channels: (driver.outputs > 0).then_some(driver.outputs as usize),
            buffer_frames: driver.buffer_frames,
            // ASIO has no system default; the first driver is what an empty
            // filter selects, so marking it would be a lie about the OS.
            default: false,
            available: driver.available,
        });
    }

    out
}

/// The friendly name of the endpoint WASAPI uses when no filter is given, so the
/// picker can mark it. `None` when there is no default device at all.
pub fn default_render_device_name() -> Option<String> {
    let _ = initialize_mta();
    let en = DeviceEnumerator::new().ok()?;
    en.get_default_device(Direction::Render)
        .ok()?
        .get_friendlyname()
        .ok()
}

/// Render device names, as `--device` matches them. Exposed so the CLI never
/// invents its own enumeration and drifts from the engine's matching rules.
pub fn list_render_devices() -> Result<Vec<String>> {
    let _ = initialize_mta();
    let en = DeviceEnumerator::new()?;
    let mut names = Vec::new();
    for dev in &en.get_device_collection(Direction::Render)? {
        let dev = dev?;
        names.push(dev.get_friendlyname()?);
    }
    Ok(names)
}

/// Pick a render device by substring, or the default when no filter is given.
fn pick_device(en: &DeviceEnumerator, filter: Option<&str>) -> Result<Device> {
    let Some(filter) = filter else {
        return en.get_default_device(Direction::Render).map_err(Into::into);
    };

    let lower = filter.to_lowercase();
    let mut available = Vec::new();
    for dev in &en.get_device_collection(Direction::Render)? {
        let dev = dev?;
        let name = dev.get_friendlyname()?;
        if name.to_lowercase().contains(&lower) {
            return Ok(dev);
        }
        available.push(name);
    }

    Err(EngineError::DeviceNotFound {
        filter: filter.to_string(),
        available: available.join(" | "),
    })
}

/// Map the device mix format onto the two layouts this engine can produce.
///
/// Shared-mode devices hand out either 32-bit float or 16-bit integer in
/// practice. Anything else is reported rather than silently mis-read.
fn output_format_of(mix: &WaveFormat) -> Result<OutputFormat> {
    let bits = mix.get_bitspersample();
    let sample = match mix.get_subformat()? {
        wasapi::SampleType::Float if bits == 32 => DeviceSample::F32,
        wasapi::SampleType::Int if bits == 16 => DeviceSample::I16,
        other => {
            return Err(EngineError::Unsupported(format!(
                "device mix format is {other} / {bits} bit; this engine emits f32 or i16"
            )));
        }
    };
    Ok(OutputFormat {
        sample_rate: mix.get_samplespersec(),
        channels: mix.get_nchannels() as usize,
        sample,
    })
}

/// Ask Windows for a real-time-ish thread. Best effort: failing here costs
/// scheduling jitter, not correctness, so it must never stop playback.
///
/// Also used by the ASIO backend: the driver calls its buffer callback on the
/// thread that started it, so promoting this thread is what gets that callback a
/// real-time priority.
pub(crate) fn join_pro_audio_group() -> Option<windows::Win32::Foundation::HANDLE> {
    use windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW;
    use windows::core::PCWSTR;

    // "Pro Audio" is the MMCSS task DAWs and audio servers register under.
    let name: Vec<u16> = "Pro Audio\0".encode_utf16().collect();
    let mut index = 0u32;
    unsafe { AvSetMmThreadCharacteristicsW(PCWSTR::from_raw(name.as_ptr()), &mut index).ok() }
}

fn render_thread(
    shared: Arc<Shared>,
    filter: Option<String>,
    buffer_hns: i64,
    ready: SyncSender<Result<StreamInfo>>,
) {
    let _mmcss = join_pro_audio_group();

    let opened = (|| -> Result<(StreamInfo, AudioClient, Handle, AudioRenderClient, usize)> {
        let _ = initialize_mta();
        let en = DeviceEnumerator::new()?;
        let dev = pick_device(&en, filter.as_deref())?;
        let device_name = dev.get_friendlyname().unwrap_or_else(|_| "?".into());

        let mut client = dev.get_iaudioclient()?;
        let mix = client.get_mixformat()?;
        let format = output_format_of(&mix)?;

        client.initialize_client(
            &mix,
            Direction::Render,
            StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: buffer_hns,
            },
            None,
        )?;
        let endpoint_frames = client.get_buffer_size()?;

        // Held for the whole stream lifetime. Dropping it closes the event, and
        // Start() then fails with AUDCLNT_E_EVENTHANDLE_NOT_SET.
        let event = client.set_get_eventhandle()?;
        let render = client.get_audiorenderclient()?;

        let info = StreamInfo {
            format,
            device_name,
            endpoint_frames,
        };
        Ok((info, client, event, render, format.frame_bytes()))
    })();

    let (info, client, event, render, bytes_per_frame) = match opened {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };

    // Prime the endpoint with silence so the first real write is not preceded by
    // an underrun, then start and report readiness.
    let space = client.get_available_space_in_frames().unwrap_or(0);
    if space > 0 {
        let silence = vec![0u8; space as usize * bytes_per_frame];
        let _ = render.write_to_device(&silence, None);
    }
    if let Err(e) = client.start_stream() {
        let _ = ready.send(Err(e.into()));
        return;
    }
    let _ = ready.send(Ok(info));

    render_loop(&shared, &client, &event, &render, bytes_per_frame);
    let _ = client.stop_stream();
}

fn render_loop(
    shared: &Arc<Shared>,
    client: &AudioClient,
    event: &Handle,
    render: &AudioRenderClient,
    bytes_per_frame: usize,
) {
    // Sized for the largest block the device can ask for, so the callback only
    // ever writes into memory that already exists.
    let max_frames = client.get_buffer_size().unwrap_or(0) as usize;
    if max_frames == 0 || bytes_per_frame == 0 {
        shared.running.store(false, Ordering::Release);
        return;
    }
    let mut mixer = Mixer::new(max_frames, bytes_per_frame);

    while shared.running.load(Ordering::Acquire) {
        // A timeout is not an error: it means the device had nothing to give.
        if event.wait_for_event(200).is_err() {
            continue;
        }

        let space = match client.get_available_space_in_frames() {
            Ok(0) | Err(_) => continue,
            Ok(n) => (n as usize).min(max_frames),
        };
        let want_bytes = space * bytes_per_frame;

        // Padding is what the endpoint still has queued ahead of us. Zero means
        // the device is starving right now; if a clip is mid-playback that is an
        // audible gap. A finished or paused clip is not counted: its slot stays
        // populated by design, and silence after it is correct.
        let paused = shared.paused.load(Ordering::Acquire);
        let padding = client.get_current_padding().unwrap_or(0);
        if padding == 0 && !paused && !shared.finished.load(Ordering::Acquire) {
            let active_idx = shared.active.load(Ordering::Acquire) & 1;
            if shared.slots[active_idx].load().is_some() {
                shared.underruns.fetch_add(1, Ordering::Relaxed);
            }
        }

        // The padding read is also the queue depth the latency report needs, so
        // the slot fill and the measurement share one query.
        mixer.fill(shared, space, bytes_per_frame, padding);

        if render
            .write_to_device(&mixer.scratch()[..want_bytes], None)
            .is_err()
        {
            // A failed write means the stream is gone (device removed, session
            // reset). Stop instead of spinning on a dead client.
            shared.running.store(false, Ordering::Release);
            break;
        }
    }
}
