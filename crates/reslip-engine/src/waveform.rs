//! Peak envelopes for the waveform display.
//!
//! The renderer never sees PCM. It gets a fixed small number of floats, one per
//! horizontal pixel bucket, and draws bars from them. That keeps the IPC payload
//! at 4KB regardless of whether the file is three seconds or three minutes, and
//! it means the webview does no audio work at all.
//!
//! Peaks are computed from the same decode the engine plays, so a waveform can
//! never disagree with what comes out of the speakers — including the gapless
//! trim, which would otherwise shift every peak by 23–25ms against the sound.

use std::path::Path;

use crate::decode::{Decoder, SourceAudio};
use crate::error::Result;

/// How many floats a waveform carries. Matches the reference implementation;
/// a wider panel stretches bars, it does not need more numbers.
pub const DEFAULT_BUCKETS: usize = 1000;

/// A peak envelope plus enough metadata to draw it.
#[derive(Debug, Clone)]
pub struct Waveform {
    /// Max absolute amplitude per bucket, in `0.0..=1.0`, across all channels.
    pub peaks: Vec<f32>,
    /// Source duration in seconds, before any resampling.
    pub duration_s: f64,
    pub sample_rate: u32,
    pub channels: usize,
    /// Time spent decoding. Reported so the UI can show the cost of a cold
    /// waveform instead of appearing to hang on a long file.
    pub decode_ms: f64,
}

impl Waveform {
    /// Decode `path` and reduce it to `buckets` peaks.
    pub fn scan<P: AsRef<Path>>(decoder: &Decoder, path: P, buckets: usize) -> Result<Self> {
        let source = decoder.decode_source(path)?;
        Ok(Self::from_source(&source, buckets))
    }

    /// Reduce an already-decoded clip to peaks.
    pub fn from_source(source: &SourceAudio, buckets: usize) -> Self {
        let frames = if source.channels == 0 {
            0
        } else {
            source.samples.len() / source.channels
        };
        Self {
            peaks: bucket_peaks(&source.samples, source.channels, buckets),
            duration_s: frames as f64 / source.sample_rate.max(1) as f64,
            sample_rate: source.sample_rate,
            channels: source.channels,
            decode_ms: 0.0,
        }
    }
}

/// Max absolute amplitude per bucket, across all channels.
///
/// Exact rather than approximated: bucket `b` covers frames
/// `[b*frames/buckets, (b+1)*frames/buckets)`, and every frame in that range is
/// visited. The reference implementation took maxima over 256-frame blocks and
/// then merged blocks into buckets, which quantises bucket edges to 256 frames;
/// on a 1000-bucket display of a 2-second file that is a visible off-by-one at
/// every bar boundary when the file has a sharp transient near it.
///
/// Clamped to 1.0 because a clipped source can decode slightly above full scale
/// and a bar taller than the panel is just a layout bug.
pub fn bucket_peaks(samples: &[f32], channels: usize, buckets: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; buckets];
    if channels == 0 || buckets == 0 {
        return out;
    }
    let frames = samples.len() / channels;
    if frames == 0 {
        return out;
    }

    for (b, slot) in out.iter_mut().enumerate() {
        let start = b * frames / buckets;
        let end = (((b + 1) * frames / buckets).max(start + 1)).min(frames);
        let mut peak = 0.0f32;
        for frame in start..end {
            let base = frame * channels;
            for &sample in &samples[base..base + channels] {
                let magnitude = sample.abs();
                if magnitude > peak {
                    peak = magnitude;
                }
            }
        }
        *slot = peak.min(1.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_flat_zero() {
        let peaks = bucket_peaks(&vec![0.0f32; 4000], 2, 100);
        assert_eq!(peaks.len(), 100);
        assert!(peaks.iter().all(|&p| p == 0.0));
    }

    #[test]
    fn empty_input_still_returns_the_right_shape() {
        let peaks = bucket_peaks(&[], 2, 50);
        assert_eq!(peaks.len(), 50);
        assert!(peaks.iter().all(|&p| p == 0.0));
    }

    #[test]
    fn full_scale_buckets_report_one() {
        let peaks = bucket_peaks(&vec![1.0f32; 3200], 2, 64);
        assert!(peaks.iter().all(|&p| (p - 1.0).abs() < f32::EPSILON));
    }

    #[test]
    fn the_loudest_channel_wins() {
        // Frame 0: left quiet, right loud. The bucket must show the right channel,
        // otherwise a hard-panned sample would draw as near-silence.
        let peaks = bucket_peaks(&[0.1, 0.9, 0.1, 0.9], 2, 2);
        assert!((peaks[0] - 0.9).abs() < 1e-6);
        assert!((peaks[1] - 0.9).abs() < 1e-6);
    }

    #[test]
    fn a_single_transient_lands_in_exactly_one_bucket() {
        // 1000 frames, 10 buckets, so bucket b owns frames [100b, 100b+100).
        let mut samples = vec![0.0f32; 1000];
        samples[350] = 0.75;
        let peaks = bucket_peaks(&samples, 1, 10);
        assert!((peaks[3] - 0.75).abs() < 1e-6, "bucket 3 = {}", peaks[3]);
        let others: f32 = peaks
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 3)
            .map(|(_, p)| *p)
            .sum();
        assert_eq!(others, 0.0);
    }

    #[test]
    fn bucket_edges_are_exact_not_quantised() {
        // 700 frames into 7 buckets: bucket 1 is [100,200). A transient at frame
        // 100 must be in bucket 1 and must not leak into bucket 0.
        let mut samples = vec![0.0f32; 700];
        samples[100] = 0.5;
        let peaks = bucket_peaks(&samples, 1, 7);
        assert_eq!(peaks[0], 0.0);
        assert!((peaks[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn more_buckets_than_frames_does_not_panic() {
        let peaks = bucket_peaks(&[0.2, 0.4, 0.6], 1, 10);
        assert_eq!(peaks.len(), 10);
        // Every frame must still be represented somewhere.
        assert!(peaks.iter().any(|&p| (p - 0.6).abs() < 1e-6));
    }

    #[test]
    fn samples_above_full_scale_are_clamped() {
        let peaks = bucket_peaks(&[2.5, -3.0], 2, 4);
        assert!(peaks.iter().all(|&p| p <= 1.0));
    }
}
