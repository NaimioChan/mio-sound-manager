//! File → device-format PCM.
//!
//! Everything expensive happens here, once, on a worker thread: container
//! probing, decoding, channel remapping, sample-rate conversion, and the final
//! conversion into the exact byte layout the audio device expects. The render
//! callback therefore only ever copies bytes.
//!
//! Decoding the whole file up front is a deliberate choice for P1: samples in a
//! sample manager are seconds long, and a fully resident clip is the simplest
//! thing that cannot underrun because of a slow disk. Streaming decode-ahead for
//! long files is a later concern; see `docs/architecture.md` section 4.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{Decoder as SymphoniaDecoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{
    FormatOptions, FormatReader, SeekMode as SymphoniaSeekMode, SeekTo,
};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Duration as Frames;
use symphonia::core::units::Time;

use crate::error::{EngineError, Result};

/// How the device wants its samples. Only the two formats shared-mode devices
/// actually hand out are supported; anything else is reported instead of guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSample {
    F32,
    I16,
    /// 32-bit integer, little-endian. ASIO drivers commonly hand out
    /// `ASIOSTInt32LSB`, which is a full-scale 32-bit sample rather than 24 bits
    /// in a 32-bit container, so the conversion scales by `i32::MAX`.
    I32,
}

impl DeviceSample {
    pub fn bytes(self) -> usize {
        match self {
            DeviceSample::F32 | DeviceSample::I32 => 4,
            DeviceSample::I16 => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DeviceSample::F32 => "f32",
            DeviceSample::I16 => "i16",
            DeviceSample::I32 => "i32",
        }
    }
}

/// The device's mix format: what every clip must be converted to at decode time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputFormat {
    pub sample_rate: u32,
    pub channels: usize,
    pub sample: DeviceSample,
}

impl OutputFormat {
    pub fn frame_bytes(&self) -> usize {
        self.channels * self.sample.bytes()
    }
}

/// A clip already converted to the device format, immutable and shareable.
///
/// `bytes` is interleaved, `channels` wide, at `sample_rate`, in `sample` format.
/// Length is always `frames * format.frame_bytes()`.
pub struct DecodedAudio {
    pub bytes: Vec<u8>,
    pub frames: usize,
    pub total_frames: usize,
    pub format: OutputFormat,
    /// Source properties, kept for reporting and for the UI ("44.1k → 48k").
    pub source_rate: u32,
    pub source_channels: usize,
    pub decode_ms: f64,
    pub resampled: bool,
    pub path: String,
    /// Container-declared priming frames that were removed from the head of the
    /// decode. Non-zero means the container reported encoder delay and we
    /// honoured it, so the clip starts on the first real sample.
    pub trimmed_leading_frames: usize,
    pub source: Option<Arc<SourceAudio>>,
    pub complete: bool,
    /// First frame of `bytes` measured from the head of the *audible* stream,
    /// in source frames. Zero for a clip decoded from the top; the seek target
    /// (converted to source frames) for a window decode. Source frames, not
    /// device frames: the waveform for a window clip is drawn at
    /// `window_start_frames`'s place in the source envelope.
    pub window_start_frames: usize,
}

impl DecodedAudio {
    pub fn duration_s(&self) -> f64 {
        self.frames as f64 / self.format.sample_rate as f64
    }

    pub fn frame_bytes(&self) -> usize {
        self.format.frame_bytes()
    }

    /// True when there is nothing left to play. A zero-length clip is legal
    /// (a corrupt tail decodes to nothing) and must not stall the render loop.
    pub fn is_empty(&self) -> bool {
        self.frames == 0
    }
}

/// Interleaved f32 at the file's own rate and channel count, as decoded.
///
/// This is the pre-conversion form of a clip: no resampling, no channel remap,
/// no byte packing. [`Decoder::decode`] converts it for the device; the peak
/// scanner reads it directly, so a waveform is computed from exactly the samples
/// that will be played (including the gapless trim).
#[derive(Debug, Clone)]
pub struct SourceAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: usize,
    pub trimmed_leading_frames: usize,
}

/// A bounded decode request: start at `start_s` and produce at most
/// `duration_s` of audio. `start_s == 0` with no duration is a full decode.
///
/// Used by long-material playback (>60s): the first publish is a window at the
/// seek target, and each later window chains from the end of the previous one,
/// so a 22-minute file never holds more than a few seconds of PCM.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecodeWindow {
    pub start_s: f64,
    pub duration_s: f64,
}

impl DecodeWindow {
    /// Window at `start_s` lasting `duration_s`.
    pub fn at(start_s: f64, duration_s: f64) -> Self {
        Self {
            start_s: start_s.max(0.0),
            duration_s: duration_s.max(0.0),
        }
    }
}

/// Decodes files into the device format. Cheap to construct and stateless
/// between files, so one instance can serve sequential and parallel decodes.
#[derive(Debug, Clone, Copy)]
pub struct Decoder {
    format: OutputFormat,
    timeout: Duration,
    max_duration_s: Option<f64>,
    window: Option<DecodeWindow>,
}

impl Decoder {
    pub fn new(format: OutputFormat) -> Self {
        Self {
            format,
            timeout: Duration::from_secs(30),
            max_duration_s: None,
            window: None,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_max_duration(mut self, seconds: f64) -> Self {
        self.max_duration_s = Some(seconds.max(0.0));
        self
    }

    /// Decode a bounded window starting `window.start_s` into the file.
    ///
    /// The demuxer seeks to the requested time (accurate mode lands on the
    /// packet boundary at or before it; the demuxer's own trim tracking makes
    /// the decoded output start at the requested position, not the packet
    /// head), then decodes forward for at most `window.duration_s`. The result
    /// carries `window_start_frames` so the engine can map buffer positions
    /// back to file positions.
    pub fn with_window(mut self, window: DecodeWindow) -> Self {
        self.window = Some(window);
        self
    }

    pub fn format(&self) -> OutputFormat {
        self.format
    }

    /// Decode `path` fully and convert it to the device format.
    pub fn decode<P: AsRef<Path>>(&self, path: P) -> Result<DecodedAudio> {
        self.decode_with_source(path).map(|(decoded, _)| decoded)
    }

    pub fn decode_with_source<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(DecodedAudio, Option<Arc<SourceAudio>>)> {
        let path = path.as_ref();
        let started = Instant::now();
        let display = path.display().to_string();

        let window = self.window;

        let (samples, source_rate, source_channels, trimmed, complete, declared_frames) =
            self.decode_to_f32(path, window)?;
        let source = Arc::new(SourceAudio {
            samples,
            sample_rate: source_rate,
            channels: source_channels,
            trimmed_leading_frames: trimmed,
        });
        let (src_rate, src_channels, trimmed) = (
            source.sample_rate,
            source.channels,
            source.trimmed_leading_frames,
        );

        let source_bytes = source.samples.len() * std::mem::size_of::<f32>();
        if source_bytes > 256 * 1024 * 1024 {
            return Err(EngineError::Decode {
                path: display,
                detail: "decoded PCM exceeds the 256MiB per-clip memory budget".into(),
            });
        }

        let mut resampled = false;
        let interleaved = if src_rate != self.format.sample_rate && !source.samples.is_empty() {
            resampled = true;
            resample(&source.samples, src_channels, src_rate, self.format.sample_rate)?
        } else {
            source.samples.clone()
        };

        let interleaved = remap_channels(&interleaved, src_channels, self.format.channels);
        let bytes = to_device_bytes(&interleaved, self.format.sample);
        let frames = bytes.len() / self.format.frame_bytes();
        if source_bytes.saturating_add(bytes.len()) > 256 * 1024 * 1024 {
            return Err(EngineError::Decode {
                path: display,
                detail: "source and device PCM together exceed the 256MiB per-clip memory budget".into(),
            });
        }
        let source_cache = Some(Arc::clone(&source));

        // The source-frame position of the first buffer frame within the audible
        // stream (i.e. after the gapless trim). For a window decode it is the
        // requested start converted to the source rate; the window decode itself
        // starts the output exactly at the requested position (the loop drops
        // and trims packets so no demuxer packet slop survives), so this is an
        // exact mapping and `window_start_frames - trimmed` converts it into the
        // audible-stream coordinates `total_frames` uses. For a full decode it
        // is `trimmed`, i.e. zero in audible coordinates.
        let window_start_frames = match window {
            Some(window) => {
                ((window.start_s * src_rate as f64).round() as usize).max(trimmed)
            }
            None => 0,
        };
        let audible_start = window_start_frames.saturating_sub(trimmed);

        let total_frames = if complete {
            audible_start + frames
        } else {
            declared_frames
                .map(|total| {
                    // The container's total is in raw trimmed-timeline source
                    // frames; the audible-stream length subtracts the trim. For
                    // a window decode the total is the whole file, not the
                    // window, so it is independent of `audible_start`.
                    total.saturating_sub(trimmed)
                })
                .unwrap_or(audible_start + frames)
                .max(audible_start + frames)
        };

        Ok((
            DecodedAudio {
                bytes,
                frames,
                total_frames,
                format: self.format,
                source_rate: src_rate,
                source_channels: src_channels,
                decode_ms: started.elapsed().as_secs_f64() * 1000.0,
                resampled,
                path: display,
                trimmed_leading_frames: trimmed,
                source: source_cache.clone(),
                complete,
                window_start_frames,
            },
            source_cache,
        ))
    }

    /// Decode `path` to interleaved f32 without touching the device format.
    ///
    /// Used by the waveform scanner. The device format is irrelevant to a peak
    /// envelope, and resampling a whole file just to measure its maxima would
    /// double the work for no visible difference.
    pub fn decode_source<P: AsRef<Path>>(&self, path: P) -> Result<SourceAudio> {
        let (samples, sample_rate, channels, trimmed_leading_frames, _, _) =
            self.decode_to_f32(path.as_ref(), self.window)?;
        Ok(SourceAudio {
            samples,
            sample_rate,
            channels,
            trimmed_leading_frames,
        })
    }

    /// Decode to interleaved f32 at the file's own rate and channel count.
    ///
    /// Channel count and rate come from the first decoded packet rather than the
    /// container header: symphonia's isomp4 demuxer reports 0 channels for M4A,
    /// so trusting the header would mis-handle a format the product must play.
    ///
    /// `window` seeks the demuxer to `start_s` before decoding and stops after
    /// `duration_s` of decoded output. All three demuxers the product plays
    /// (WAV/RIFF, MP3, MP4) land the seek on the packet at or before the
    /// requested time and stamp each packet with its own stream timestamp, so
    /// dropping packets whose audible range starts before the window keeps the
    /// output sample-aligned with a full decode from the top — a frame at file
    /// position T decodes to the same sample wherever it was decoded from.
    /// Packets that straddle the boundary (trim info) keep their start trim and
    /// lose only the frames before the window, so the first output frame is the
    /// frame at `start_s`, not the packet boundary.
    fn decode_to_f32(
        &self,
        path: &Path,
        window: Option<DecodeWindow>,
    ) -> Result<(Vec<f32>, u32, usize, usize, bool, Option<usize>)> {
        let display = path.display().to_string();
        let started = Instant::now();
        if started.elapsed() >= self.timeout {
            return Err(EngineError::DecodeTimeout {
                path: display,
                timeout_ms: self.timeout.as_millis().min(u64::MAX as u128) as u64,
            });
        }
        let file = File::open(path).map_err(|e| EngineError::Decode {
            path: display.clone(),
            detail: e.to_string(),
        })?;
        let stream = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        // Gapless must be on. Without it, an MP3 or M4A keeps the encoder's
        // priming samples at the head of the stream: measured 23–25ms of silence
        // before the first audible frame. In a sample manager that is latency
        // added to every trigger, and it is the same order as the whole device
        // path, so the trim information has to be honoured.
        let format_options = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };

        let mut format: Box<dyn FormatReader> = symphonia::default::get_probe()
            .format(&hint, stream, &format_options, &MetadataOptions::default())
            .map_err(|e| EngineError::Decode {
                path: display.clone(),
                detail: format!("probe: {e}"),
            })?
            .format;

        let track = format
            .default_track()
            .ok_or_else(|| EngineError::Decode {
                path: display.clone(),
                detail: "no default track".into(),
            })?
            .clone();
        let track_id = track.id;
        let declared_frames = track.codec_params.n_frames.map(|frames| frames as usize);

        let make_decoder = || -> Result<Box<dyn SymphoniaDecoder>> {
            symphonia::default::get_codecs()
                .make(&track.codec_params, &DecoderOptions::default())
                .map_err(|e| EngineError::Decode {
                    path: display.clone(),
                    detail: format!("codec: {e}"),
                })
        };
        let mut decoder = make_decoder()?;

        // A window decode seeks before touching any packet. Accurate mode
        // guarantees the demuxer lands at or before the requested position, and
        // every packet after that carries its own stream timestamp, so the loop
        // below can drop the audible-range-before-window part and keep the rest.
        // WAVE seeks are exact (PCM, no trim); MP3/MP4 may hand a packet with
        // trim info that straddles the boundary, which is handled per packet.
        let mut window_start_src_frames = 0u64; // in the demuxer's trimmed timeline
        let mut window_end_src_frames = u64::MAX;
        if let Some(window) = window {
            let time_base = track
                .codec_params
                .time_base
                .ok_or_else(|| EngineError::Decode {
                    path: display.clone(),
                    detail: "container does not report a time base; cannot seek".into(),
                })?;
            // `Time` is whole seconds plus a fractional part; split without
            // letting rounding push the fraction to exactly 1.
            let to_time = |seconds: f64| {
                let secs = seconds.max(0.0);
                let whole = secs.floor() as u64;
                let frac = secs - whole as f64;
                Time::new(whole, frac.min(0.999_999_999))
            };
            let requested_ts = time_base.calc_timestamp(to_time(window.start_s));
            // The seeked-to timestamp is not read directly: the packet loop
            // aligns the output to `requested_ts` and the demuxer's packet
            // timestamps after the seek do the rest. The call is what moves
            // the read position; an error here is a real failure to seek.
            format
                .seek(SymphoniaSeekMode::Accurate, SeekTo::Time {
                    time: to_time(window.start_s),
                    track_id: Some(track_id),
                })
                .map_err(|e| EngineError::Decode {
                    path: display.clone(),
                    detail: format!("seek to {:.2}s: {e}", window.start_s),
                })?;
            // The demuxer lands on a packet boundary at or before the requested
            // position (WAV rounds down to a 1152-frame packet; MP3 may back up
            // further for the bit reservoir; MP4 to the containing sample).
            // Timestamps after the seek are trimmed-timeline values (gapless
            // enabled), so the drop window below lives in that same timeline:
            // start at the *requested* position, not the packet head, so the
            // decoded output is frame-aligned with a full decode. Packets
            // before the requested position are dropped whole; a straddling
            // packet keeps its trim and loses only the frames before the
            // window, which keeps the output sample-exact.
            window_start_src_frames = requested_ts;
            if window.duration_s > 0.0 {
                window_end_src_frames = time_base
                    .calc_timestamp(to_time(window.start_s + window.duration_s))
                    .max(window_start_src_frames);
            }
        }

        let mut out: Vec<f32> = Vec::new();
        let mut src_rate = 0u32;
        let mut src_channels = 0usize;
        let mut scratch: Option<SampleBuffer<f32>> = None;
        let mut complete = true;

        loop {
            if started.elapsed() >= self.timeout {
                return Err(EngineError::DecodeTimeout {
                    path: display,
                    timeout_ms: self.timeout.as_millis().min(u64::MAX as u128) as u64,
                });
            }
            let packet = match format.next_packet() {
                Ok(p) => p,
                // End of stream is the ordinary exit. A truncated tail should not
                // discard audio already decoded, so mid-stream errors also break —
                // but only when something was decoded; otherwise it is a failure.
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(SymphoniaError::ResetRequired) => {
                    decoder = make_decoder()?;
                    continue;
                }
                Err(e) => {
                    if out.is_empty() {
                        return Err(EngineError::Decode {
                            path: display.clone(),
                            detail: format!("demux: {e}"),
                        });
                    }
                    break;
                }
            };

            if packet.track_id() != track_id {
                continue;
            }

            // Window alignment. `packet.ts` is the packet's first frame in the
            // trimmed (gapless) timeline — the same timeline the window bounds
            // were computed in. Three cases:
            //
            // 1. Packet entirely before the window: drop it without decoding.
            // 2. Packet straddling the window start: decode it, trim the head
            //    so the first output frame is the frame at the window start,
            //    and copy only the remainder.
            // 3. Packet at or after the window: copy as-is.
            //
            // A decoded frame at file position T therefore lands at the same
            // offset in `out` no matter where the decode started, which is what
            // makes `window_start_frames` (in device frames) an exact mapping.
            let mut window_skip_frames = 0usize;
            if packet.ts + packet.dur() <= window_start_src_frames && window_start_src_frames != 0 {
                continue;
            }
            if packet.ts < window_start_src_frames {
                window_skip_frames = (window_start_src_frames - packet.ts) as usize;
            }
            // Stop once the window's audible range is full. Packets are stamped
            // with their own timestamps, so this is a position check, not a
            // duration guess.
            if packet.ts >= window_end_src_frames {
                complete = false;
                break;
            }

            let decoded = match decoder.decode(&packet) {
                Ok(d) => d,
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(SymphoniaError::ResetRequired) => {
                    decoder = make_decoder()?;
                    continue;
                }
                Err(e) => {
                    if out.is_empty() {
                        return Err(EngineError::Decode {
                            path: display.clone(),
                            detail: format!("decode: {e}"),
                        });
                    }
                    break;
                }
            };

            const MAX_PCM_BYTES: usize = 256 * 1024 * 1024;
            let packet_bytes = decoded.capacity().saturating_mul(
                decoded.spec().channels.count().max(1) * std::mem::size_of::<f32>(),
            );
            if packet_bytes > MAX_PCM_BYTES || out.len().saturating_add(packet_bytes / 4) > MAX_PCM_BYTES / 4 {
                return Err(EngineError::Decode {
                    path: display.clone(),
                    detail: "decoded PCM exceeds the 256MiB per-clip memory budget".into(),
                });
            }

            if src_rate == 0 {
                src_rate = decoded.spec().rate;
                src_channels = decoded.spec().channels.count();
                scratch = Some(SampleBuffer::<f32>::new(
                    decoded.capacity() as Frames,
                    SignalSpec::new(src_rate, decoded.spec().channels),
                ));
            }

            // Reallocate only when a packet larger than any seen so far arrives.
            if let Some(buf) = scratch.as_ref() {
                if buf.capacity() < decoded.capacity() {
                    scratch = Some(SampleBuffer::<f32>::new(
                        decoded.capacity() as Frames,
                        SignalSpec::new(src_rate, decoded.spec().channels),
                    ));
                }
            }
            if let Some(buf) = scratch.as_mut() {
                buf.copy_interleaved_ref(decoded);
                let mut samples = buf.samples();
                // Trim the straddle head so `out` starts exactly at the window.
                if window_skip_frames > 0 {
                    let skip = (window_skip_frames * src_channels).min(samples.len());
                    samples = &samples[skip..];
                }
                out.extend_from_slice(samples);
            }
            if let Some(max_duration_s) = self.max_duration_s {
                let max_frames = (max_duration_s * src_rate as f64).ceil() as usize;
                if src_channels > 0 && out.len() / src_channels >= max_frames {
                    out.truncate(max_frames.saturating_mul(src_channels));
                    complete = false;
                    break;
                }
            }
            // Window length cap, measured in decoded output frames (window has
            // already dropped the head, so `out` starts at the window start).
            if let Some(window) = window {
                if window.duration_s > 0.0 {
                    let max_frames = (window.duration_s * src_rate as f64).ceil() as usize;
                    if src_channels > 0 && out.len() / src_channels >= max_frames {
                        out.truncate(max_frames.saturating_mul(src_channels));
                        complete = false;
                        break;
                    }
                }
            }
        }

        if src_rate == 0 {
            return Err(EngineError::Decode {
                path: display,
                detail: "stream produced no decodable packets".into(),
            });
        }

        // MP4/AAC carries AAC's 1024-sample encoder priming at the head of the
        // stream, declared only in the `elst` edit list. Symphonia parses `elst`
        // but does not surface it as `codec_params.delay`, so gapless trimming
        // never happens for M4A and the clip starts 1024 frames early. Measured
        // at 23.2ms on the reference machine — larger than the entire device
        // path, so a sample trigger would feel late by that much.
        //
        // Rather than hand-parse the atom, use the fact that an AAC stream's
        // first packet is always priming. This matches what ffprobe reports as
        // `initial_padding` for these files.
        let mut trimmed = 0usize;
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("m4a") || e.eq_ignore_ascii_case("mp4"))
            && src_channels > 0
            && out.len() > 1024 * src_channels
            && leading_is_priming(&out, src_channels, 1024)
        {
            out.drain(..1024 * src_channels);
            trimmed = 1024;
        }

        Ok((
            out,
            src_rate,
            src_channels,
            trimmed,
            complete,
            declared_frames,
        ))
    }
}

/// Whether the first `frames` frames are priming rather than music.
///
/// Guards the AAC priming trim so a file that genuinely starts with content is
/// never shortened. The test is deliberately loose: AAC priming decodes to
/// low-level swish rather than exact zeros (measured peak -42 dBFS on an FFmpeg
/// encode, because the filterbank fills the gap with the codec's own ringing),
/// while real musical content at the head of a sample sits far above -40 dBFS.
/// The 90th percentile is used rather than the maximum so that one stray
/// transient cannot defeat the check.
fn leading_is_priming(samples: &[f32], channels: usize, frames: usize) -> bool {
    let end = (frames * channels).min(samples.len());
    if end == 0 {
        return false;
    }
    let mut mags: Vec<f32> = samples[..end].iter().map(|s| s.abs()).collect();
    mags.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p90 = mags[(mags.len() - 1) * 9 / 10];
    // -40 dBFS
    p90 < 0.01
}

/// Resample interleaved f32 from `rate_in` to `rate_out`, whole clip at once.
fn resample(input: &[f32], channels: usize, rate_in: u32, rate_out: u32) -> Result<Vec<f32>> {
    use rubato::audioadapter_buffers::direct::InterleavedSlice;
    use rubato::{Fft, FixedSync, Resampler, WindowFunction};

    let frames = input.len() / channels;
    let mut rs = Fft::<f32>::new_custom(
        rate_in as usize,
        rate_out as usize,
        1024,
        1,
        channels,
        WindowFunction::BlackmanHarris2,
        FixedSync::Both,
    )
    .map_err(|e| EngineError::Unsupported(format!("resampler: {e}")))?;

    let src = InterleavedSlice::new(input, channels, frames)
        .map_err(|e| EngineError::Unsupported(format!("resampler input: {e}")))?;
    let out = rs
        .process_all(&src, frames, None)
        .map_err(|e| EngineError::Unsupported(format!("resample: {e}")))?;
    Ok(out.take_data())
}

/// Force the channel count to match the device.
///
/// The cases that actually occur: mono sample into a stereo device (duplicate),
/// and a stereo clip into a mono device (average). Wider layouts fall back to
/// copying the channels that exist and repeating the last one, which is what a
/// sample manager needs — not a surround downmix.
fn remap_channels(input: &[f32], in_ch: usize, out_ch: usize) -> Vec<f32> {
    if in_ch == out_ch || input.is_empty() {
        return input.to_vec();
    }
    let frames = input.len() / in_ch;
    let mut out = vec![0.0f32; frames * out_ch];

    if out_ch == 1 {
        for (f, chunk) in input.chunks_exact(in_ch).enumerate() {
            out[f] = chunk.iter().sum::<f32>() / in_ch as f32;
        }
        return out;
    }

    for f in 0..frames {
        for c in 0..out_ch {
            let src = if in_ch == 1 { 0 } else { c.min(in_ch - 1) };
            out[f * out_ch + c] = input[f * in_ch + src];
        }
    }
    out
}

/// Convert interleaved f32 to the device's sample format.
fn to_device_bytes(samples: &[f32], format: DeviceSample) -> Vec<u8> {
    match format {
        DeviceSample::F32 => {
            let mut out = Vec::with_capacity(samples.len() * 4);
            for s in samples {
                out.extend_from_slice(&s.to_le_bytes());
            }
            out
        }
        DeviceSample::I16 => {
            let mut out = Vec::with_capacity(samples.len() * 2);
            for s in samples {
                let clamped = s.clamp(-1.0, 1.0);
                let v = (clamped * i16::MAX as f32).round() as i16;
                out.extend_from_slice(&v.to_le_bytes());
            }
            out
        }
        DeviceSample::I32 => {
            let mut out = Vec::with_capacity(samples.len() * 4);
            for s in samples {
                let clamped = s.clamp(-1.0, 1.0);
                // `i32::MAX as f32` is not exactly representable, so the product
                // can land one ulp above i32::MAX and saturate on cast. Clamping
                // the product keeps full-scale input at full-scale output.
                let scaled = (clamped * i32::MAX as f32).round();
                let v = scaled.clamp(i32::MIN as f32, i32::MAX as f32) as i32;
                out.extend_from_slice(&v.to_le_bytes());
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_to_stereo_duplicates() {
        let out = remap_channels(&[0.25, -0.5], 1, 2);
        assert_eq!(out, vec![0.25, 0.25, -0.5, -0.5]);
    }

    #[test]
    fn stereo_to_mono_averages() {
        let out = remap_channels(&[1.0, 0.0, -1.0, 0.0], 2, 1);
        assert_eq!(out, vec![0.5, -0.5]);
    }

    #[test]
    fn matching_channels_is_a_copy() {
        let input = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(remap_channels(&input, 2, 2), input);
    }

    #[test]
    fn priming_guard_keeps_real_content_at_the_head() {
        let mut genuine_attack = vec![0.0; 1024];
        genuine_attack[..256].fill(0.5);
        assert!(!leading_is_priming(&genuine_attack, 1, 1024));

        let codec_ringing = vec![0.005; 1024];
        assert!(leading_is_priming(&codec_ringing, 1, 1024));
    }

    #[test]
    fn an_expired_decode_budget_returns_a_path_specific_timeout() {
        let path = std::env::temp_dir().join("mio-decode-timeout.wav");
        std::fs::write(&path, b"not decoded").unwrap();
        let decoder = Decoder::new(OutputFormat {
            sample_rate: 48_000,
            channels: 2,
            sample: DeviceSample::F32,
        })
        .with_timeout(Duration::ZERO);
        let error = decoder.decode_source(&path).unwrap_err();
        assert!(matches!(error, EngineError::DecodeTimeout { .. }));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn max_duration_returns_a_prefix_with_the_declared_total() {
        let path = std::env::temp_dir().join("mio-prefix-decode.wav");
        let frames = 48_000u32;
        let data_len = frames * 4;
        let mut wav = Vec::with_capacity((44 + data_len) as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&48_000u32.to_le_bytes());
        wav.extend_from_slice(&192_000u32.to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.resize((44 + data_len) as usize, 0);
        std::fs::write(&path, wav).unwrap();

        let decoder = Decoder::new(OutputFormat {
            sample_rate: 48_000,
            channels: 2,
            sample: DeviceSample::F32,
        })
        .with_max_duration(0.1);
        let (clip, _) = decoder.decode_with_source(&path).unwrap();
        assert!(!clip.complete);
        assert_eq!(clip.frames, 4_800);
        assert!(clip.total_frames >= frames as usize);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_window_decode_starts_at_the_requested_position_with_the_whole_file_total() {
        // 5 seconds of a marked waveform: frame f carries the value
        // (f % 1000) as f32 / 1000, so a decoded buffer can be aligned
        // against the file by its samples alone.
        let rate = 48_000u32;
        let frames = 5 * rate;
        let mut pcm = Vec::with_capacity(frames as usize);
        for f in 0..frames {
            pcm.push((f % 1000) as f32 / 1000.0);
        }
        let path = std::env::temp_dir().join("mio-window-decode.wav");
        write_pcm_wav(&path, rate, 1, &pcm);

        let decoder = Decoder::new(OutputFormat {
            sample_rate: rate,
            channels: 1,
            sample: DeviceSample::F32,
        });
        let (full, _) = decoder.decode_with_source(&path).unwrap();
        assert!(full.complete);
        assert_eq!(full.window_start_frames, 0);
        // The marked waveform: frame f carries (f % 1000)/1000, so alignment
        // is checked against the file position arithmetic directly.
        let _ = full;

        // Window at 2.0s, 0.5s long.
        let (window, _) = decoder
            .with_window(DecodeWindow::at(2.0, 0.5))
            .decode_with_source(&path)
            .unwrap();
        assert!(!window.complete, "a bounded window is not the whole file");
        assert_eq!(window.window_start_frames, 2 * rate as usize);
        assert!(
            (window.frames as f64 / rate as f64 - 0.5).abs() < 0.01,
            "window length {} frames",
            window.frames
        );
        assert_eq!(window.total_frames, full.total_frames, "total is the file's, not the window's");
        // Sample alignment: buffer frame 0 must equal file frame 96000.
        let full_samples: Vec<f32> = full
            .source
            .as_ref()
            .unwrap()
            .samples
            .chunks_exact(1)
            .map(|c| c[0])
            .collect();
        let window_samples = &window.source.as_ref().unwrap().samples;
        let file_frame = 2 * rate as usize;
        // Sample-exact: the packet loop drops and trims to the requested
        // position, so buffer frame 0 is file frame 96000 in this marked
        // waveform.
        for (i, sample) in window_samples.iter().take(48).enumerate() {
            let expected = ((file_frame + i) % 1000) as f32 / 1000.0;
            assert!(
                (sample - expected).abs() < 1e-6,
                "window sample {i} = {sample}, file sample = {expected}"
            );
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_window_past_the_end_of_the_file_decodes_nothing_and_stays_safe() {
        let rate = 48_000u32;
        let frames = rate; // one second
        let pcm = vec![0.25f32; frames as usize];
        let path = std::env::temp_dir().join("mio-window-past-end.wav");
        write_pcm_wav(&path, rate, 1, &pcm);

        let decoder = Decoder::new(OutputFormat {
            sample_rate: rate,
            channels: 1,
            sample: DeviceSample::F32,
        });
        let result = decoder.with_window(DecodeWindow::at(5.0, 1.0)).decode(&path);
        match result {
            // Either an explicit error or an empty clip is acceptable; an
            // empty clip must report itself empty so the render loop treats
            // it as finished rather than spinning.
            Ok(clip) => {
                assert_eq!(clip.frames, 0);
                assert!(clip.is_empty());
            }
            Err(EngineError::Decode { .. }) => {}
            Err(error) => panic!("unexpected error: {error}"),
        }
        let _ = std::fs::remove_file(path);
    }

    /// Write a mono f32 PCM WAV (the container the tests above assume).
    fn write_pcm_wav(path: &std::path::Path, rate: u32, channels: u16, pcm: &[f32]) {
        let data_len = (pcm.len() * 4) as u32;
        let mut wav = Vec::with_capacity(44 + data_len as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * u32::from(channels) * 4).to_le_bytes());
        wav.extend_from_slice(&(channels * 4).to_le_bytes());
        wav.extend_from_slice(&32u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for sample in pcm {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(path, wav).unwrap();
    }

    #[test]
    fn i16_conversion_clamps_and_scales() {
        let bytes = to_device_bytes(&[0.0, 1.0, -1.0, 2.0], DeviceSample::I16);
        let vals: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(vals, vec![0, i16::MAX, -i16::MAX, i16::MAX]);
    }

    #[test]
    fn resampling_preserves_duration() {
        // 441 frames at 44.1k is exactly 10ms, so 48k must give ~480 frames.
        let frames = 441usize;
        let input = vec![0.0f32; frames];
        let out = resample(&input, 1, 44100, 48000).expect("resample");
        let expected = 480usize;
        let delta = (out.len() as i64 - expected as i64).abs();
        assert!(delta <= 2, "got {} frames, wanted ~{expected}", out.len());
    }
}
