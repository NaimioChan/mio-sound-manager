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
    pub finished: bool,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Case-insensitive substring of the render device's friendly name. `None`
    /// uses the system default — on this machine that is a virtual device
    /// (FxSound), whose latency numbers mean nothing.
    pub device_filter: Option<String>,
    /// Endpoint buffer duration in 100 ns units. 0 asks for the device default,
    /// which shared mode pins to the engine period regardless of the request.
    pub buffer_duration_hns: i64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            device_filter: None,
            buffer_duration_hns: 0,
        }
    }
}

struct Shared {
    slots: [ArcSwapOption<DecodedAudio>; 2],
    active: AtomicUsize,
    pub_ns: AtomicU64,
    pub_seq: AtomicU64,
    pos_frames: AtomicU64,
    /// Frame count of the clip in the active slot, published alongside it so a
    /// position poll never has to take an `arc-swap` guard on the audio thread's
    /// slot. Pure atomics on the polling path (architecture section 7.3).
    active_frames: AtomicU64,
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
}

/// Live playback engine. Dropping it stops the stream and joins the render thread.
pub struct Engine {
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
    format: OutputFormat,
    device_name: String,
    endpoint_frames: u32,
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
}

impl PlaybackMonitor {
    pub fn snapshot(&self) -> PlaybackSnapshot {
        let position = self.shared.pos_frames.load(Ordering::Acquire);
        let frames = self.shared.active_frames.load(Ordering::Acquire);
        let finished = self.shared.finished.load(Ordering::Acquire);
        PlaybackSnapshot {
            position_s: position as f64 / self.format.sample_rate as f64,
            duration_s: frames as f64 / self.format.sample_rate as f64,
            playing: frames > 0 && !finished,
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

    pub fn engine_period_ms(&self) -> f64 {
        self.endpoint_frames as f64 / 2.0 / self.format.sample_rate as f64 * 1000.0
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

        let handle = thread::Builder::new()
            .name("reslip-render".into())
            .spawn(move || render_thread(audio_shared, filter, buffer_hns, tx))
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

    pub fn engine_period_ms(&self) -> f64 {
        self.endpoint_frames as f64 / 2.0 / self.format.sample_rate as f64 * 1000.0
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
        self.shared.finished.store(true, Ordering::Release);
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

struct StreamInfo {
    format: OutputFormat,
    device_name: String,
    endpoint_frames: u32,
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
fn join_pro_audio_group() -> Option<windows::Win32::Foundation::HANDLE> {
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
    let mut scratch = vec![0u8; max_frames * bytes_per_frame];
    let mut seen_seq = shared.pub_seq.load(Ordering::Acquire);

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
        // audible gap. A finished clip is not counted: its slot stays populated
        // by design, and silence after it is correct.
        let padding = client.get_current_padding().unwrap_or(0);
        if padding == 0 && !shared.finished.load(Ordering::Acquire) {
            let active_idx = shared.active.load(Ordering::Acquire) & 1;
            if shared.slots[active_idx].load().is_some() {
                shared.underruns.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Read the slot before the sequence number: `publish` writes the clip and
        // the timestamp before flipping `active`, so seeing a new sequence here
        // guarantees the clip in this slot is the one that publish installed.
        let active = shared.active.load(Ordering::Acquire) & 1;
        let seq = shared.pub_seq.load(Ordering::Acquire);
        let guard = shared.slots[active].load();
        let clip = guard.as_ref();

        let mut filled = 0usize;

        if clip.is_none() {
            shared.silent_callbacks.fetch_add(1, Ordering::Relaxed);
        }

        if let Some(clip) = clip {
            let pos = shared.pos_frames.load(Ordering::Acquire) as usize;
            let frame_bytes = clip.frame_bytes();

            if frame_bytes == bytes_per_frame && pos < clip.frames {
                let n = (clip.frames - pos).min(space);
                let start = pos * frame_bytes;
                scratch[..n * frame_bytes]
                    .copy_from_slice(&clip.bytes[start..start + n * frame_bytes]);
                filled = n * frame_bytes;
                shared.pos_frames.store((pos + n) as u64, Ordering::Release);

                // The first callback able to write this publish is the moment the
                // click becomes audio, so that is what gets measured.
                if seq != seen_seq {
                    let pub_ns = shared.pub_ns.load(Ordering::Relaxed);
                    let now = shared.now_ns();
                    let queued = client.get_current_padding().unwrap_or(0) as f64;
                    let sr = clip.format.sample_rate as f64;
                    let queued_ms = queued / sr * 1000.0;
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
        seen_seq = seq;

        // Silence for whatever the clip did not cover. Written in place, so the
        // callback never allocates.
        scratch[filled..want_bytes].fill(0);

        if render
            .write_to_device(&scratch[..want_bytes], None)
            .is_err()
        {
            // A failed write means the stream is gone (device removed, session
            // reset). Stop instead of spinning on a dead client.
            shared.running.store(false, Ordering::Release);
            break;
        }
    }
}
