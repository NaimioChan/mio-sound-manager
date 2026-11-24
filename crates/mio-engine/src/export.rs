//! Writing a region of a file back out as a WAV.
//!
//! Exists for one feature: dragging a selection out of the waveform dock into a
//! DAW. A DAW cannot be handed "frames 4000–11000 of this mp3" — it takes a file
//! — so the selection is rendered to a real WAV first and that path is what gets
//! dropped.
//!
//! Written as 32-bit IEEE float at the file's *own* rate and channel count, not
//! the device format. An export is a copy of the material, not a monitoring path:
//! resampling it to whatever the interface happens to be running would quietly
//! change the file the user drags into their project.

use std::path::Path;

use crate::decode::SourceAudio;
use crate::error::Result;

/// Sample format written by [`write_wav`]. Always float: it is the form the
/// decoder already produces, so nothing is quantised on the way out.
const WAV_FLOAT_TAG: u16 = 3;

/// Write interleaved `samples` as a canonical 32-bit float WAV.
///
/// `samples` is interleaved f32, `channels` wide, at `sample_rate`. A length that
/// is not a whole number of frames is truncated rather than padded: a half frame
/// at the end of a selection is not audio.
pub fn write_wav(
    path: impl AsRef<Path>,
    samples: &[f32],
    channels: usize,
    sample_rate: u32,
) -> Result<()> {
    let channels = channels.max(1);
    let frames = samples.len() / channels;
    let bits: u16 = 32;
    let block_align = channels as u16 * (bits / 8);
    let byte_rate = sample_rate * block_align as u32;
    let data_len = (frames * channels * 4) as u32;

    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&WAV_FLOAT_TAG.to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in &samples[..frames * channels] {
        out.extend_from_slice(&sample.to_le_bytes());
    }

    std::fs::write(path, &out).map_err(crate::EngineError::from)
}

/// Slice `[start_s, end_s)` out of `source` and write it as a WAV.
///
/// The range is clamped to the clip, so a selection left over from a longer file
/// exports what exists instead of failing. Returns the number of frames written.
pub fn write_wav_region(
    path: impl AsRef<Path>,
    source: &SourceAudio,
    start_s: f64,
    end_s: f64,
) -> Result<usize> {
    let channels = source.channels.max(1);
    let frames = source.samples.len() / channels;
    let rate = source.sample_rate.max(1) as f64;

    let from = ((start_s.max(0.0) * rate) as usize).min(frames);
    let to = ((end_s.max(0.0) * rate) as usize).min(frames);
    let to = to.max(from);

    let slice = &source.samples[from * channels..to * channels];
    write_wav(path, slice, channels, source.sample_rate)?;
    Ok(to - from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(frames: usize) -> SourceAudio {
        let samples = (0..frames * 2).map(|i| i as f32 / 1000.0).collect();
        SourceAudio {
            samples,
            sample_rate: 48_000,
            channels: 2,
            trimmed_leading_frames: 0,
            window_start_frames: 0,
        }
    }

    #[test]
    fn a_region_keeps_only_its_own_frames() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("region.wav");
        // 4800 frames at 48kHz is 0.1s, so [0.02, 0.04) is frames 960..1920.
        let written = write_wav_region(&out, &source(4800), 0.02, 0.04).unwrap();
        assert_eq!(written, 960);

        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        // Header is 44 bytes, then 960 frames * 2 channels * 4 bytes.
        assert_eq!(bytes.len(), 44 + 960 * 2 * 4);
        // The first exported sample is the 960th frame's left channel.
        let first = f32::from_le_bytes([bytes[44], bytes[45], bytes[46], bytes[47]]);
        assert!((first - (960.0 * 2.0) / 1000.0).abs() < 1e-6, "{first}");
    }

    #[test]
    fn a_region_past_the_end_is_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("clamped.wav");
        let written = write_wav_region(&out, &source(4800), 0.0, 60.0).unwrap();
        assert_eq!(written, 4800);
    }

    #[test]
    fn an_inverted_region_writes_an_empty_but_valid_wav() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("empty.wav");
        let written = write_wav_region(&out, &source(4800), 0.05, 0.02).unwrap();
        assert_eq!(written, 0);
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(bytes.len(), 44);
        assert_eq!(&bytes[0..4], b"RIFF");
    }
}
