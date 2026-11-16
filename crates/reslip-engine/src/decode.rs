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
use std::time::Instant;

use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{Decoder as SymphoniaDecoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Duration as Frames;

use crate::error::{EngineError, Result};

/// How the device wants its samples. Only the two formats shared-mode devices
/// actually hand out are supported; anything else is reported instead of guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSample {
    F32,
    I16,
}

impl DeviceSample {
    pub fn bytes(self) -> usize {
        match self {
            DeviceSample::F32 => 4,
            DeviceSample::I16 => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DeviceSample::F32 => "f32",
            DeviceSample::I16 => "i16",
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

/// Decodes files into the device format. Cheap to construct and stateless
/// between files, so one instance can serve sequential and parallel decodes.
#[derive(Debug, Clone, Copy)]
pub struct Decoder {
    format: OutputFormat,
}

impl Decoder {
    pub fn new(format: OutputFormat) -> Self {
        Self { format }
    }

    pub fn format(&self) -> OutputFormat {
        self.format
    }

    /// Decode `path` fully and convert it to the device format.
    pub fn decode<P: AsRef<Path>>(&self, path: P) -> Result<DecodedAudio> {
        let path = path.as_ref();
        let started = Instant::now();
        let display = path.display().to_string();

        let (interleaved, src_rate, src_channels, trimmed) = self.decode_to_f32(path)?;

        let mut resampled = false;
        let interleaved = if src_rate != self.format.sample_rate && !interleaved.is_empty() {
            resampled = true;
            resample(
                &interleaved,
                src_channels,
                src_rate,
                self.format.sample_rate,
            )?
        } else {
            interleaved
        };

        let interleaved = remap_channels(&interleaved, src_channels, self.format.channels);
        let bytes = to_device_bytes(&interleaved, self.format.sample);
        let frames = bytes.len() / self.format.frame_bytes();

        Ok(DecodedAudio {
            bytes,
            frames,
            format: self.format,
            source_rate: src_rate,
            source_channels: src_channels,
            decode_ms: started.elapsed().as_secs_f64() * 1000.0,
            resampled,
            path: display,
            trimmed_leading_frames: trimmed,
        })
    }

    /// Decode to interleaved f32 at the file's own rate and channel count.
    ///
    /// Channel count and rate come from the first decoded packet rather than the
    /// container header: symphonia's isomp4 demuxer reports 0 channels for M4A,
    /// so trusting the header would mis-handle a format the product must play.
    fn decode_to_f32(&self, path: &Path) -> Result<(Vec<f32>, u32, usize, usize)> {
        let display = path.display().to_string();
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
            .format(
                &hint,
                stream,
                &format_options,
                &MetadataOptions::default(),
            )
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

        let make_decoder = || -> Result<Box<dyn SymphoniaDecoder>> {
            symphonia::default::get_codecs()
                .make(&track.codec_params, &DecoderOptions::default())
                .map_err(|e| EngineError::Decode {
                    path: display.clone(),
                    detail: format!("codec: {e}"),
                })
        };
        let mut decoder = make_decoder()?;

        let mut out: Vec<f32> = Vec::new();
        let mut src_rate = 0u32;
        let mut src_channels = 0usize;
        let mut scratch: Option<SampleBuffer<f32>> = None;

        loop {
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
                out.extend_from_slice(buf.samples());
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

        Ok((out, src_rate, src_channels, trimmed))
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
fn resample(
    input: &[f32],
    channels: usize,
    rate_in: u32,
    rate_out: u32,
) -> Result<Vec<f32>> {
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
            let src = if in_ch == 1 {
                0
            } else {
                c.min(in_ch - 1)
            };
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
