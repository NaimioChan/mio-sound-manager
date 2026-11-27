//! WASAPI shared-mode output with dual-slot switching.
//!
//! Layout, from `docs/architecture.md` section 4:
//!
//! * A dedicated render thread owns every COM object. It has to: `IAudioClient`
//!   is `!Send`, so it can only be created on the thread that uses it.
//! * Two slots hold decoded clips together with their own coordinates. The
//!   render callback reads `active` and copies from that slot; nothing else
//!   happens there.
//! * Switching playback is one atomic store of a complete slot state. Old clips
//!   are released when their last `Arc` goes away, which may be the callback
//!   thread; what the callback guarantees is no allocation, no lock and no IO.
//!
//! The render callback allocates nothing, locks nothing, does no IO, and never
//! formats a string. Measurements leave it through a preallocated lock-free queue.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
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
    /// Whether the render thread is looping the clip.
    pub looping: bool,
    /// The looped region in seconds, or `None` when it is the whole clip.
    pub loop_region_s: Option<(f64, f64)>,
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

/// Everything that gives one published buffer its meaning: the samples, where
/// the buffer starts in the file, and the generation tag that ties the playhead
/// to it.
///
/// The clip and its coordinates travel as one `Arc`, so the render callback can
/// never observe the clip from one publish next to the offset of another. The
/// playhead is the only mutable part and it carries the same generation tag
/// (see [`Shared::pos_tagged`]), which is what closes the seek window the old
/// three separate atomics left open.
pub(crate) struct SlotState {
    pub clip: Arc<DecodedAudio>,
    /// Frame of this buffer that playback starts at. The seek offset for a
    /// `publish_from`, zero for a normal publish and for a queued window.
    pub start: u64,
    /// First frame of the buffer in the file's audible stream, in device frames.
    /// Zero for a full decode; the window origin for a window decode.
    pub window_start: u64,
    /// Whole file length in device frames, for the duration readout.
    pub total_frames: u64,

    /// Generation tag, stored alongside the playhead so the callback can tell
    /// whether a position belongs to the buffer it is holding. Bumped by every
    /// publish and every handoff.
    pub tag: u32,
}

/// The playhead packs its owning generation in the high 32 bits and the buffer
/// offset in the low 32. One atomic word, so "which publish is this position
/// for" can never be read half written.
///
/// 32 bits of frames is not a limit in practice: a buffer is either a
/// `WINDOW_SECONDS` window or a full decode, and a full decode is bounded by
/// `MAX_PCM_BYTES` (≈ 20 minutes of 48k stereo f32), far below 2^32 frames.
const POS_FRAMES_MASK: u64 = u32::MAX as u64;

fn pack_pos(tag: u32, frames: u64) -> u64 {
    ((tag as u64) << 32) | frames.min(POS_FRAMES_MASK)
}

fn unpack_pos(word: u64) -> (u32, u64) {
    ((word >> 32) as u32, word & POS_FRAMES_MASK)
}

pub(crate) struct Shared {
    /// Two play slots. Index `active` holds the buffer the callback copies;
    /// publishing writes the other one and flips `active`, so the clip and all
    /// of its coordinates arrive together.
    slots: [ArcSwapOption<SlotState>; 2],
    /// The queued next segment for long-material playback. Published by
    /// `queue_next` from the decode-ahead thread; the render callback consumes
    /// it in the callback where the current buffer runs out and swaps it into
    /// the active slot itself, so the handoff never waits for a polling loop.
    next: ArcSwapOption<SlotState>,
    active: AtomicUsize,
    pub_ns: AtomicU64,
    pub_seq: AtomicU64,
    /// Generation counter for [`SlotState::gen`]. Never reused within a process.
    tag_seq: AtomicU64,

    pos_tagged: AtomicU64,
    /// Output gain as raw `f32` bits. The slider writes it, the render callback
    /// reads it; nothing else touches it. Stored as bits because atomics have no
    /// `f32` variant and a lock in the callback is not an option.
    gain_bits: AtomicU32,
    /// Set by `pause`, cleared by `publish` and `resume`. While set the render
    /// callback writes silence and leaves `pos_frames` alone, so the playhead
    /// freezes exactly where the listener heard it stop.
    paused: AtomicBool,
    finished: AtomicBool,
    /// Loop the clip instead of stopping at its end.
    ///
    /// Three atomics rather than one `Option<(u64, u64)>` because the callback
    /// cannot take a lock and an `Option` has no atomic form. The order is
    /// start, end, then `looping` last: the callback only reads the range once
    /// `looping` is already true, and a half-written range is clamped to the clip
    /// anyway, so the worst case is one callback of the old region.
    looping: AtomicBool,
    /// A long-file window chain owns the boundary instead of wrapping inside
    /// the active buffer. This lets the chain queue the next window, including
    /// a return to the first window for a loop, without the callback restarting
    /// the current window prematurely.
    streaming_loop: AtomicBool,
    /// First frame of the looped region. `end == 0` means the whole clip.
    loop_start: AtomicU64,
    /// One past the last frame of the looped region.
    loop_end: AtomicU64,
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
    #[cfg(test)]
    fn new() -> Arc<Self> {
        Arc::new(Self {
            slots: [const { ArcSwapOption::const_empty() }; 2],
            next: ArcSwapOption::const_empty(),
            active: AtomicUsize::new(0),
            pub_ns: AtomicU64::new(0),
            pub_seq: AtomicU64::new(0),
            tag_seq: AtomicU64::new(1),
            pos_tagged: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            looping: AtomicBool::new(false),
            streaming_loop: AtomicBool::new(false),
            loop_start: AtomicU64::new(0),
            loop_end: AtomicU64::new(0),
            running: AtomicBool::new(true),
            measurements: ArrayQueue::new(4096),
            underruns: AtomicU64::new(0),
            silent_callbacks: AtomicU64::new(0),
            gain_bits: AtomicU32::new(1.0f32.to_bits()),
            t0: Instant::now(),
        })
    }

    fn now_ns(&self) -> u64 {
        self.t0.elapsed().as_nanos() as u64
    }

    /// Take the queued next segment. Render-callback path: atomic swap, no lock.
    pub(crate) fn take_queued(&self) -> Option<Arc<SlotState>> {
        self.next.swap(None)
    }

    /// Read the playhead, taking a guard on the active slot to learn which
    /// generation the position has to belong to.
    fn active_slot(&self) -> Option<Arc<SlotState>> {
        let active = self.active.load(Ordering::Acquire) & 1;
        self.slots[active].load_full()
    }

    /// The playhead for a holder of the slot it was written for.
    ///
    /// A position tagged with a different generation is from an older publish
    /// (the caller read `active` just before the flip); it is discarded rather
    /// than used as an offset, because it may point past the end of this buffer.
    fn position_for(&self, slot: &SlotState, word: u64) -> u64 {
        let (tag, pos) = unpack_pos(word);
        // Clamp as well as reject: a slot whose buffer is shorter than the
        // position is a bug, not a reason to index out of bounds.
        if tag == slot.tag {
            pos.min(slot.clip.frames as u64)
        } else {
            slot.start.min(slot.clip.frames as u64)
        }
    }

    /// Fill a [`PlaybackSnapshot`] from one generation-consistent read.
    fn snapshot(&self, sample_rate: u32) -> PlaybackSnapshot {
        let slot = self.active_slot();
        let slot = slot.as_deref();
        let word = self.pos_tagged.load(Ordering::Acquire);
        let paused = self.paused.load(Ordering::Acquire);
        let finished = self.finished.load(Ordering::Acquire);
        let looping = self.looping.load(Ordering::Acquire);

        let (position_frames, duration_frames) = match slot {
            Some(slot) => (
                slot.window_start + self.position_for(slot, word),
                slot.total_frames,
            ),
            None => (0, 0),
        };
        let rate = sample_rate as f64;
        PlaybackSnapshot {
            position_s: position_frames as f64 / rate,
            duration_s: duration_frames as f64 / rate,
            // A paused clip is loaded and parked: still "playing" in the sense of
            // having a live slot, but the playhead must not advance and Space must
            // resume it rather than restart it.
            playing: duration_frames > 0 && !finished && !paused,
            paused: duration_frames > 0 && !finished && paused,
            finished,
            looping,
            loop_region_s: {
                let start = self.loop_start.load(Ordering::Acquire) as f64 / rate;
                let end = self.loop_end.load(Ordering::Acquire) as f64 / rate;
                (end > start).then_some((start, end))
            },
        }
    }

    /// Publish `slot` into the inactive play slot and make it active.
    ///
    /// The slot store carries the clip, its start offset, its window origin and
    /// its total in one `Arc`; the active flip is then the only thing a callback
    /// can observe. The position is written *before* the flip and tagged with the
    /// same tag, so a callback that loads the old active index reads a position
    /// whose tag no longer matches and falls back to that slot's start.
    ///
    /// The sequence is bumped *after* the slot store as well, so a callback can
    /// never measure this publish against the clip that came before it.
    fn publish_slot(&self, slot: SlotState) -> u64 {
        self.streaming_loop.store(false, Ordering::Release);
        let current = self.active.load(Ordering::Acquire);
        let next = 1 - current;
        self.pos_tagged
            .store(pack_pos(slot.tag, slot.start), Ordering::Release);
        self.slots[next].store(Some(Arc::new(slot)));
        let seq = self.pub_seq.fetch_add(1, Ordering::AcqRel) + 1;
        self.pub_ns.store(self.now_ns(), Ordering::Relaxed);
        self.finished.store(false, Ordering::Release);
        // A new publish always starts audible: leaving `paused` set would play a
        // silent clip, which is the one state that looks like a broken engine.
        self.paused.store(false, Ordering::Release);
        // A loop region belongs to the clip it was drawn on, so a different clip
        // loops whole. The flag survives — a user who turned looping on to audition
        // a one-shot expects the next row to loop too.
        self.loop_start.store(0, Ordering::Release);
        self.loop_end.store(0, Ordering::Release);
        self.active.store(next, Ordering::Release);
        seq
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

/// Convert a decoded window's source-frame origin to the device timeline used
/// by the render callback and playback monitor.
fn window_start_device_frames(clip: &DecodedAudio, device_rate: u32) -> u64 {
    let audible_source_frames = clip
        .window_start_frames
        .saturating_sub(clip.trimmed_leading_frames);
    (audible_source_frames as f64 * device_rate as f64 / clip.source_rate as f64).round() as u64
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

        // One generation-consistent read: the clip, its buffer length, its
        // window origin and the playhead all come from the same publish. The
        // position is tagged with the slot's generation, so a position written by
        // a publish the callback has not flipped to yet is rejected below instead
        // of being used as an index into a shorter buffer.
        let slot = shared.active_slot();
        let pos_word = shared.pos_tagged.load(Ordering::Acquire);
        let seq = shared.pub_seq.load(Ordering::Acquire);

        let Some(slot) = slot.as_deref() else {
            shared.silent_callbacks.fetch_add(1, Ordering::Relaxed);
            self.seen_seq = seq;
            self.scratch[..want_bytes].fill(0);
            return 0;
        };

        let clip = &slot.clip;
        // The offset this callback starts at. A stale tag means the position
        // belongs to a newer publish; the slot's own start is the right answer.
        let mut cursor = {
            let (tag, pos) = unpack_pos(pos_word);
            let pos = if tag == slot.tag {
                pos.min(clip.frames as u64)
            } else {
                slot.start
            };
            pos as usize
        };
        let mut tag = slot.tag;
        let paused = shared.paused.load(Ordering::Acquire);
        let clip_frame_bytes = clip.frame_bytes();
        let gain = f32::from_bits(shared.gain_bits.load(Ordering::Relaxed));

        let mut filled = 0usize;

        if paused {
            // Paused: leave the playhead where it is. The clip stays in the slot
            // so resuming is one flag flip, and the position the listener hears is
            // the position the playhead shows.
        } else if clip_frame_bytes == frame_bytes && cursor < clip.frames {
            let looping = shared.looping.load(Ordering::Acquire)
                && !shared.streaming_loop.load(Ordering::Acquire);
            // Clamp the region to the clip. An empty or inverted region — a
            // selection left over from a longer file — falls back to the whole
            // clip rather than playing silence nobody asked for.
            let frames = clip.frames;
            let (loop_from, loop_to) = if looping {
                let from = (shared.loop_start.load(Ordering::Acquire) as usize).min(frames);
                let raw_to = shared.loop_end.load(Ordering::Acquire) as usize;
                let to = if raw_to == 0 {
                    frames
                } else {
                    raw_to.min(frames)
                };
                if to > from { (from, to) } else { (0, frames) }
            } else {
                (0, frames)
            };

            // A playhead outside the region means the selection moved while this
            // clip was playing: enter at the region's start instead of playing
            // the part the user just deselected.
            if looping && (cursor < loop_from || cursor >= loop_to) {
                cursor = loop_from;
            }

            // Copied in segments because a callback can straddle the loop point.
            // The loop is bounded by `want_frames` and every iteration copies at
            // least one frame (the region was proven non-empty above), so this
            // cannot spin.
            let mut written = 0usize;
            let mut buffer_end = if looping { loop_to } else { frames };
            // Swapped only when a queued window takes over mid-callback.
            let mut handed_off_clip: Option<Arc<SlotState>> = None;
            while written < want_frames {
                let segment_end = if looping { loop_to } else { buffer_end };
                let n = (segment_end - cursor).min(want_frames - written);
                if n == 0 {
                    break;
                }
                let src = cursor * clip_frame_bytes;
                let dst = written * clip_frame_bytes;
                self.scratch[dst..dst + n * clip_frame_bytes]
                    .copy_from_slice(&clip.bytes[src..src + n * clip_frame_bytes]);
                written += n;
                cursor += n;
                if cursor >= segment_end {
                    if looping {
                        cursor = loop_from;
                    } else {
                        break;
                    }
                }
            }

            // Segment handoff. When the buffer ran out with room left in this
            // callback — or exactly at the boundary — and a next segment is
            // queued, swap it into the active slot and keep copying from its head
            // in the same callback. The queued slot carries its own coordinates,
            // so the swap publishes a complete state, not a clip the callback has
            // to pair with a separately stored length and origin.
            //
            // `seq` is read before the swap on purpose: the handoff publishes no
            // new sequence, so the latency report for the original publish is
            // still the one to measure.
            let mut handed_off = false;
            let streaming_loop = shared.streaming_loop.load(Ordering::Acquire);
            if (!looping || streaming_loop) && written < want_frames && cursor >= buffer_end {
                if let Some(next) = shared.take_queued() {
                    if next.clip.format == clip.format {
                        tag = next.tag;
                        let next_active = shared.active.load(Ordering::Acquire) & 1;
                        shared.pos_tagged.store(
                            pack_pos(next.tag, next.start),
                            Ordering::Release,
                        );
                        shared.slots[next_active].store(Some(Arc::clone(&next)));
                        cursor = next.start as usize;
                        buffer_end = next.clip.frames;
                        handed_off_clip = Some(next);
                        handed_off = true;
                    }
                }
            }
            let clip_after = handed_off_clip.as_deref().map(|s| &s.clip).unwrap_or(clip);

            // Continue filling from the handed-off clip. At most one handoff per
            // callback: the queued slot is now empty, so a second pass would find
            // nothing — and whatever is still missing is written as silence,
            // which is the correct response to a decode-ahead thread that has not
            // caught up.
            if handed_off && written < want_frames {
                while written < want_frames {
                    let n = (buffer_end - cursor).min(want_frames - written);
                    if n == 0 {
                        break;
                    }
                    let src = cursor * clip_frame_bytes;
                    let dst = written * clip_frame_bytes;
                    self.scratch[dst..dst + n * clip_frame_bytes]
                        .copy_from_slice(&clip_after.bytes[src..src + n * clip_frame_bytes]);
                    written += n;
                    cursor += n;
                    if cursor >= buffer_end {
                        break;
                    }
                }
                // The new segment's own end: finished only if it is also the
                // file's last segment (`complete`), otherwise the decode-ahead
                // thread is expected to queue the next one.
                if cursor >= buffer_end && !clip_after.complete {
                    shared.finished.store(false, Ordering::Release);
                }
            }


            // Unity gain is the common case and skipping the multiply keeps the
            // callback at one memcpy. Anything else scales in place, in the
            // device's own sample format, which is why this has to know the format
            // rather than working on `f32`. Applied once over the whole copied
            // region, so a looped callback scales the same way an unlooped one
            // does.
            if gain != 1.0 && written > 0 {
                apply_gain(
                    &mut self.scratch[..written * clip_frame_bytes],
                    clip_after.format.sample,
                    gain,
                );
            }
            filled = written * clip_frame_bytes;
            shared
                .pos_tagged
                .store(pack_pos(tag, cursor as u64), Ordering::Release);

            // The first callback able to write this publish is the moment the
            // click becomes audio, so that is what gets measured.
            if seq != self.seen_seq {
                let pub_ns = shared.pub_ns.load(Ordering::Relaxed);
                let now = shared.now_ns();
                let sr = clip_after.format.sample_rate as f64;
                let queued_ms = queued_frames as f64 / sr * 1000.0;
                let to_write_ms = now.saturating_sub(pub_ns) as f64 / 1e6;
                // A full queue drops the sample; acceptable for a diagnostic and
                // it must never block the callback.
                let _ = shared.measurements.push(LatencyReport {
                    seq,
                    to_write_ms,
                    queued_ms,
                    to_audible_ms: to_write_ms + queued_ms,
                });
            }

            // A looped clip is never finished: reaching the loop point is the
            // point of the feature, not the end of playback. A file whose window
            // chain owns the boundary is not finished at a window end either —
            // the chain queues the next window, or loops back to the first. Only
            // the buffer actually running out, and only when it was the whole
            // file (`complete`), ends playback: an incomplete window that ran out
            // is waiting for its successor from the chain.
            if !looping && !streaming_loop && !handed_off && cursor >= buffer_end {
                shared
                    .finished
                    .store(clip_after.complete, Ordering::Release);
            }
        }
        self.seen_seq = seq;

        // Silence for whatever the clip did not cover. Written in place, so the
        // callback never allocates.
        self.scratch[filled..want_bytes].fill(0);
        filled
    }
}

/// Scale interleaved device-format samples in place.
///
/// Runs on the render thread, so: no allocation, no branches per sample beyond
/// the format match, and no float conversion where the format does not need one.
/// `i16`/`i32` are scaled in their own integer domain with a saturating cast, so
/// a clip already at full scale stays at full scale instead of wrapping.
fn apply_gain(bytes: &mut [u8], sample: DeviceSample, gain: f32) {
    match sample {
        DeviceSample::F32 => {
            for chunk in bytes.chunks_exact_mut(4) {
                let value = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                chunk.copy_from_slice(&(value * gain).to_le_bytes());
            }
        }
        DeviceSample::I16 => {
            for chunk in bytes.chunks_exact_mut(2) {
                let value = i16::from_le_bytes([chunk[0], chunk[1]]);
                let scaled = (value as f32 * gain).round();
                let out = scaled.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                chunk.copy_from_slice(&out.to_le_bytes());
            }
        }
        DeviceSample::I32 => {
            for chunk in bytes.chunks_exact_mut(4) {
                let value = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let scaled = (value as f64 * gain as f64).round();
                let out = scaled.clamp(i32::MIN as f64, i32::MAX as f64) as i32;
                chunk.copy_from_slice(&out.to_le_bytes());
            }
        }
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
        self.shared.snapshot(self.format.sample_rate)
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

    /// The output gain the engine is applying, so the UI can show the value it
    /// actually got rather than the one it asked for.
    pub fn gain(&self) -> f32 {
        f32::from_bits(self.shared.gain_bits.load(Ordering::Acquire))
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
    /// An engine wired to an in-memory `Shared` with no device behind it.
    ///
    /// The publish/playhead state machine is all in `Shared`; the render thread
    /// only calls `Mixer::fill`. Tests drive `fill` directly, which is why this
    /// needs no audio device and can run in CI.
    #[cfg(test)]
    fn for_test(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            handle: None,
            format: OutputFormat {
                sample_rate: 48_000,
                channels: 2,
                sample: DeviceSample::F32,
            },
            device_name: String::new(),
            endpoint_frames: 480,
            backend: Backend::Wasapi,
            started: Instant::now(),
        }
    }

    /// Open the device and start the persistent render thread.
    ///
    /// The device format is discovered on the render thread (it owns the COM
    /// objects), so this blocks until the stream is live and the format known.
    /// That happens once, at startup.
    pub fn start(config: EngineConfig) -> Result<Self> {
        let shared = Arc::new(Shared {
            slots: [const { ArcSwapOption::const_empty() }; 2],
            next: ArcSwapOption::const_empty(),
            active: AtomicUsize::new(0),
            pub_ns: AtomicU64::new(0),
            pub_seq: AtomicU64::new(0),
            tag_seq: AtomicU64::new(1),
            pos_tagged: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            looping: AtomicBool::new(false),
            streaming_loop: AtomicBool::new(false),
            loop_start: AtomicU64::new(0),
            loop_end: AtomicU64::new(0),
            running: AtomicBool::new(true),
            measurements: ArrayQueue::new(4096),
            underruns: AtomicU64::new(0),
            silent_callbacks: AtomicU64::new(0),
            gain_bits: AtomicU32::new(1.0f32.to_bits()),
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
            .name("mio-render".into())
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
    ///
    /// `offset_frames` is relative to the clip's buffer. The window origin is
    /// stored separately so the monitor can add it exactly once.
    /// Positions beyond the buffer are clamped to its end, which a zero-length
    /// window decode (a seek past EOF) degrades to instead of an error.
    pub fn publish_from(&self, clip: Arc<DecodedAudio>, offset_frames: u64) -> u64 {
        let buffer_frames = clip.frames as u64;
        let total_frames = clip.total_frames.max(clip.frames) as u64;
        let offset = offset_frames.min(buffer_frames);
        // `pos` is the relative offset inside the buffer; the window origin is
        // stored separately so a seek is not counted twice in the playhead.
        let window_start = window_start_device_frames(&clip, self.format.sample_rate)
            .min(total_frames.saturating_sub(offset));
        let tag = self.shared.tag_seq.fetch_add(1, Ordering::AcqRel) as u32;

        // One state, one store, one flip. The clip, its offset, its window origin
        // and its total cannot be observed in a mixed state because they are all
        // inside the `SlotState` that the slot store publishes. The sequence and
        // timestamp are bumped inside that same step, so a callback can never
        // measure this publish against the clip that came before it.
        self.shared.publish_slot(SlotState {
            clip,
            start: offset,
            window_start,
            total_frames,
            tag,
        })
    }

    /// Queue the next segment of a long file for the render callback to pick up.
    ///
    /// Long-material playback publishes a bounded window, and this parks the
    /// following window next to the two play slots. The render callback takes it
    /// in the callback where the current buffer runs out — one atomic swap on
    /// the audio thread, no lock, no polling loop — so consecutive windows
    /// hand off without an audible seam. Storing a different clip replaces
    /// whatever was queued; `stop` clears it.
    ///
    /// `window_start_frames` is the window's origin in the file, in device
    /// frames. It is what the playhead adds to the buffer offset, so a handoff
    /// keeps reporting a file position instead of restarting at zero.
    pub fn queue_next(&self, clip: Arc<DecodedAudio>, window_start_frames: u64) {
        let tag = self.shared.tag_seq.fetch_add(1, Ordering::AcqRel) as u32;
        let total_frames = clip.total_frames.max(clip.frames) as u64;
        self.shared.next.store(Some(Arc::new(SlotState {
            clip,
            start: 0,
            window_start: window_start_frames,
            total_frames,
            tag,
        })));
    }

    /// Drop a queued segment without publishing it. Used when a newer play or a
    /// stop makes the queued window stale.
    pub fn clear_queued(&self) {
        self.shared.next.store(None);
    }

    /// The queued segment, if any, taking it out of the slot.
    ///
    /// Called from the render callback only (and by tests). The swap is atomic;
    /// the caller owns the clip afterwards.
    pub fn take_queued(&self) -> Option<Arc<DecodedAudio>> {
        self.shared
            .take_queued()
            .map(|slot| Arc::clone(&slot.clip))
    }

    /// Whether a segment is still waiting for the render callback to take it.
    /// Control threads use this to wait for a handoff without guessing from the
    /// playhead coordinate (which wraps to zero for a loop).
    pub fn has_queued(&self) -> bool {
        self.shared.next.load_full().is_some()
    }

    /// Stop playback now: clear both slots and park `finished`.
    ///
    /// This is the `stop` half of the IPC surface. It does not touch the stream,
    /// so the next `publish` costs one atomic store, same as any other switch.
    /// The endpoint keeps the audio it already queued (shared mode cannot drop
    /// it), which is why a stop is heard as a very short tail rather than a cut.
    pub fn stop(&self) {
        // Clear the playhead tag and both slots, then park. The order matters:
        // clearing a slot before the flip leaves a window where the active index
        // points at nothing, and a caller that raced it would read a live engine
        // as empty. Flip first, then drop the old slots.
        let active = self.shared.active.load(Ordering::Acquire);
        let next = 1 - active;
        self.shared.pos_tagged.store(0, Ordering::Release);
        self.shared.slots[next].store(None);
        self.shared.active.store(next, Ordering::Release);
        self.shared.slots[active].store(None);
        self.shared.next.store(None);
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
        let loaded = self
            .shared
            .active_slot()
            .is_some_and(|slot| slot.total_frames > 0);
        if loaded && !self.shared.finished.load(Ordering::Acquire) {
            self.shared.paused.store(false, Ordering::Release);
        }
    }

    /// Report the end of a window-chained file.
    ///
    /// A long file is played as a chain of windows, and the chain tells the
    /// render callback not to own the boundary (`streaming_loop`), so the
    /// callback never sets `finished` at a window end — including the last one.
    /// The chain calls this once the last window has been handed over, to park
    /// the transport at the end instead of leaving a playhead sitting at the
    /// duration with the transport still showing Play.
    pub fn stop_playback_at_end(&self) {
        self.shared.finished.store(true, Ordering::Release);
    }

    /// True while a clip is loaded and paused.
    pub fn paused(&self) -> bool {
        self.shared.paused.load(Ordering::Acquire)
    }

    /// Set the output gain. `1.0` is unity; `0.0` is silence.
    ///
    /// Applied by the render callback to whatever it copies, so it takes effect on
    /// the next callback rather than at the next publish — a slider move is heard
    /// on the clip that is already playing. Costs one atomic store and no
    /// re-decode: the clip bytes stay untouched, which is what keeps a volume
    /// change off the latency path.
    pub fn set_gain(&self, gain: f32) {
        let clamped = gain.clamp(0.0, 4.0);
        self.shared
            .gain_bits
            .store(clamped.to_bits(), Ordering::Release);
    }

    /// Turn looping on or off, optionally narrowing it to `[start_s, end_s)`.
    ///
    /// The range is converted to frames here, once, so the render callback never
    /// multiplies by a sample rate. `None` (or an empty range) means the whole
    /// clip, which is what a loop with nothing selected should do.
    ///
    /// Takes effect on the next callback without re-publishing: a loop that had
    /// to reload the clip would click every time the selection changed.
    pub fn set_loop(&self, enabled: bool, region: Option<(f64, f64)>) {
        let rate = self.format.sample_rate as f64;
        let (start, end) = match region {
            Some((from, to)) if to > from => (
                (from.max(0.0) * rate) as u64,
                (to.max(0.0) * rate).round() as u64,
            ),
            _ => (0, 0),
        };
        // Range first, flag last: the callback reads the flag before the range,
        // so writing it last is what makes a half-written range unobservable.
        self.shared.loop_start.store(start, Ordering::Release);
        self.shared.loop_end.store(end, Ordering::Release);
        self.shared.looping.store(enabled, Ordering::Release);

        // Turning the loop on after a clip ran to its end has to restart it: the
        // slot is still populated but `finished` parks the callback, so without
        // this the switch would appear to do nothing on the sound the user just
        // listened to. `fill` then moves a playhead outside the region to the
        // region's start, which is why nothing is seeked here.
        if enabled
            && self
                .shared
                .active_slot()
                .is_some_and(|slot| slot.total_frames > 0)
        {
            self.shared.finished.store(false, Ordering::Release);
        }
    }

    /// Let a long-file decode chain own segment boundaries while preserving the
    /// user's loop setting in `looping` for status reads.
    pub fn set_streaming_loop(&self, enabled: bool) {
        self.shared.streaming_loop.store(enabled, Ordering::Release);
        if enabled {
            self.shared.finished.store(false, Ordering::Release);
        }
    }

    /// Whether the render thread is looping, and the region in seconds.
    ///
    /// Read back by the UI so the switch mirrors the engine rather than its own
    /// state. `None` means the whole clip.
    pub fn loop_state(&self) -> (bool, Option<(f64, f64)>) {
        let looping = self.shared.looping.load(Ordering::Acquire);
        let rate = self.format.sample_rate as f64;
        let start = self.shared.loop_start.load(Ordering::Acquire) as f64 / rate;
        let end = self.shared.loop_end.load(Ordering::Acquire) as f64 / rate;
        if end > start {
            (looping, Some((start, end)))
        } else {
            (looping, None)
        }
    }

    /// The current output gain, as last set.
    pub fn gain(&self) -> f32 {
        f32::from_bits(self.shared.gain_bits.load(Ordering::Acquire))
    }

    /// Playback position of the active clip, in frames.
    ///
    /// This is where the render thread has *written* to, which runs ahead of what
    /// the listener hears by the endpoint queue depth (~12ms shared). Correct for
    /// drawing a playhead; not a sync reference.
    pub fn position_frames(&self) -> u64 {
        let Some(slot) = self.shared.active_slot() else {
            return 0;
        };
        let word = self.shared.pos_tagged.load(Ordering::Acquire);
        self.shared.position_for(&slot, word)
    }

    pub fn position_seconds(&self) -> f64 {
        let Some(slot) = self.shared.active_slot() else {
            return 0.0;
        };
        let word = self.shared.pos_tagged.load(Ordering::Acquire);
        let frames = slot.window_start + self.shared.position_for(&slot, word);
        frames as f64 / self.format.sample_rate as f64
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
#[cfg_attr(not(feature = "asio"), allow(unused_variables))]
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

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn format() -> OutputFormat {
        OutputFormat {
            sample_rate: RATE,
            channels: 2,
            sample: DeviceSample::F32,
        }
    }

    /// A clip whose every sample encodes its own frame index, so a copied range
    /// can be checked against where it claims to come from.
    fn clip(frames: usize, start_frame: usize, total_frames: usize) -> Arc<DecodedAudio> {
        let fmt = format();
        let mut bytes = Vec::with_capacity(frames * fmt.frame_bytes());
        for frame in start_frame..start_frame + frames {
            let value = frame as f32;
            for _ in 0..fmt.channels {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        Arc::new(DecodedAudio {
            bytes,
            frames,
            total_frames,
            format: fmt,
            source_rate: RATE,
            source_channels: 2,
            decode_ms: 0.0,
            resampled: false,
            path: String::new(),
            trimmed_leading_frames: 0,
            source: None,
            complete: true,
            total_frames_known: true,
            window_start_frames: start_frame,
        })
    }

    fn shared() -> Arc<Shared> {
        Shared::new()
    }

    fn first_frame(mixer: &Mixer) -> f32 {
        f32::from_le_bytes(mixer.scratch()[..4].try_into().unwrap())
    }

    #[test]
    fn a_publish_starts_at_its_offset() {
        let shared = shared();
        let engine = Engine::for_test(Arc::clone(&shared));
        engine.publish_from(clip(100, 0, 100), 40);

        let mut mixer = Mixer::new(64, format().frame_bytes());
        let filled = mixer.fill(&shared, 16, format().frame_bytes(), 0);
        assert_eq!(filled, 16 * format().frame_bytes());
        assert_eq!(first_frame(&mixer), 40.0);
        assert_eq!(engine.position_frames(), 56);
    }

    /// A1: a publish that lands between the callback's reads must never make the
    /// callback index past the clip it is holding. The old code stored the new
    /// offset before the new clip, so a callback could pair a one-frame clip with
    /// an offset near the end of a long one.
    #[test]
    fn a_publish_mid_callback_cannot_index_out_of_bounds() {
        let shared = shared();
        let engine = Engine::for_test(Arc::clone(&shared));
        let bytes = format().frame_bytes();

        engine.publish(clip(100_000, 0, 100_000), PublishSource::Initial);
        let mut mixer = Mixer::new(512, bytes);
        for _ in 0..10 {
            mixer.fill(&shared, 256, bytes, 0);
        }

        // Seek deep into a long clip, then immediately swap in a one-frame clip
        // and seek on it. Both are publishes; a callback racing them used to be
        // able to read the long clip with the short clip's offset.
        engine.publish_from(clip(100_000, 0, 100_000), 90_000);
        engine.publish_from(clip(1, 0, 1), 0);
        let _ = mixer.fill(&shared, 512, bytes, 0);
    }

    #[test]
    fn a_callback_racing_publishes_stays_in_bounds_and_monotonic() {
        use std::sync::atomic::AtomicBool;

        let shared = shared();
        let engine = Arc::new(Engine::for_test(Arc::clone(&shared)));
        let bytes = format().frame_bytes();
        let stop = Arc::new(AtomicBool::new(false));

        let writer = {
            let engine = Arc::clone(&engine);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut round = 0usize;
                while !stop.load(Ordering::Relaxed) {
                    // Alternate a long clip seeked far in with a one-frame clip,
                    // which is the pair that used to produce an out-of-range
                    // index.
                    if round % 2 == 0 {
                        engine.publish_from(clip(96_000, 0, 96_000), 95_000);
                    } else {
                        engine.publish_from(clip(1, 0, 1), 0);
                    }
                    round += 1;
                }
            })
        };

        let reader = {
            let shared = Arc::clone(&shared);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut mixer = Mixer::new(256, bytes);
                while !stop.load(Ordering::Relaxed) {
                    let filled = mixer.fill(&shared, 128, bytes, 0);
                    // The invariant the old code broke: whatever the callback
                    // copied, it copied from inside the clip it was holding, and
                    // the playhead never points past that clip.
                    assert!(filled <= 128 * bytes);
                    let Some(slot) = shared.active_slot() else {
                        continue;
                    };
                    let pos =
                        shared.position_for(&slot, shared.pos_tagged.load(Ordering::Acquire));
                    assert!(
                        pos <= slot.clip.frames as u64,
                        "pos {pos} past buffer {}",
                        slot.clip.frames
                    );
                }
            })
        };

        std::thread::sleep(Duration::from_millis(300));
        stop.store(true, Ordering::Relaxed);
        writer.join().unwrap();
        reader.join().unwrap();
    }

    #[test]
    fn a_queued_window_hands_off_with_its_own_coordinates() {
        let shared = shared();
        let engine = Engine::for_test(Arc::clone(&shared));
        let bytes = format().frame_bytes();

        engine.publish(clip(10, 0, 30), PublishSource::Initial);
        engine.queue_next(clip(20, 10, 30), 10);

        let mut mixer = Mixer::new(40, bytes);
        let filled = mixer.fill(&shared, 30, bytes, 0);
        assert_eq!(filled, 30 * bytes);
        // First 10 frames from the first clip, then 20 from the queued one.
        let frames: Vec<f32> = mixer
            .scratch()
            .chunks_exact(bytes)
            .map(|c| f32::from_le_bytes(c[..4].try_into().unwrap()))
            .collect();
        assert_eq!(frames[..10], (0..10).map(|f| f as f32).collect::<Vec<_>>());
        assert_eq!(
            frames[10..30],
            (10..30).map(|f| f as f32).collect::<Vec<_>>()
        );
        assert!(!engine.has_queued());
    }

    #[test]
    fn a_loop_region_wraps_inside_the_buffer() {
        let shared = shared();
        let engine = Engine::for_test(Arc::clone(&shared));
        let bytes = format().frame_bytes();

        engine.publish(clip(100, 0, 100), PublishSource::Initial);
        engine.set_loop(true, Some((20.0 / RATE as f64, 60.0 / RATE as f64)));

        let mut mixer = Mixer::new(64, bytes);
        let frames: Vec<f32> = (0..3_000)
            .flat_map(|_| {
                mixer.fill(&shared, 64, bytes, 0);
                mixer
                    .scratch()
                    .chunks_exact(bytes)
                    .map(|c| f32::from_le_bytes(c[..4].try_into().unwrap()))
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(frames.iter().all(|f| *f >= 20.0 && *f < 60.0), "loop escaped its region");
    }

    #[test]
    fn a_complete_clip_parks_finished_when_it_ends() {
        let shared = shared();
        let engine = Engine::for_test(Arc::clone(&shared));
        let bytes = format().frame_bytes();
        engine.publish(clip(32, 0, 32), PublishSource::Initial);
        let mut mixer = Mixer::new(64, bytes);
        for _ in 0..4 {
            mixer.fill(&shared, 16, bytes, 0);
        }
        assert!(engine.finished(), "a complete clip must park finished at its end");
        assert!(
            engine.snapshot().finished,
            "and the snapshot must report it, which is what the transport reads"
        );
    }

    #[test]
    fn stop_clears_the_slot_and_parks_finished() {
        let shared = shared();
        let engine = Engine::for_test(Arc::clone(&shared));
        engine.publish(clip(100, 0, 100), PublishSource::Initial);
        engine.stop();
        assert!(shared.active_slot().is_none());
        assert!(engine.finished());
        assert_eq!(engine.position_frames(), 0);
        assert_eq!(engine.snapshot().duration_s, 0.0);
    }
}
