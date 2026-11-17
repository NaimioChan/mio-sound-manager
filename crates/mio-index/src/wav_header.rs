//! A minimal RIFF/WAVE header reader, used when symphonia refuses the file.
//!
//! symphonia's WAV demuxer accepts a `fmt ` chunk of exactly 16, 18 or 40 bytes
//! and returns `malformed stream: wav: malformed fmt_pcm chunk` for anything else
//! (see `symphonia-format-riff` `wave/chunks.rs::read_pcm_fmt`). A 20-byte
//! `fmt ` chunk is legal — it is `WAVEFORMATEX` with `cbSize = 2` — and appears in
//! real libraries: 284 files of a 280k-file library failed to probe for this
//! reason alone, all of them plain PCM that ffprobe reads without complaint.
//!
//! This reader does one thing: given the first bytes of a file, return duration,
//! channels and sample rate. It does not decode, does not validate the audio
//! data, and does not attempt to be a demuxer. Everything it reads is in the
//! fixed part of the header, which is why it can be this small.

/// What the header says. Each field is optional so a truncated header still
/// yields whatever was legible.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WavHeader {
    pub channels: Option<u32>,
    pub sample_rate: Option<u32>,
    /// Duration derived from the `data` chunk size and the byte rate.
    pub duration_s: Option<f64>,
    /// `bits_per_sample`, kept for the caller's diagnostics.
    pub bits: Option<u16>,
    /// True when the format tag said IEEE float rather than PCM.
    pub is_float: bool,
}

/// WAVE format tags this reader understands well enough to trust the byte rate.
const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
/// `WAVE_FORMAT_EXTENSIBLE`: the real tag lives in the extension's SubFormat GUID,
/// but its first two bytes are the same tag, and the fixed part of the chunk is
/// still laid out like PCM, so byte rate and duration are readable.
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Parse a RIFF/WAVE header from the first bytes of a file.
///
/// Returns `None` when the bytes are not a RIFF/WAVE header at all; returns a
/// partially-filled `WavHeader` when they are but a field could not be read.
pub fn parse_wav_header(bytes: &[u8]) -> Option<WavHeader> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }

    let mut header = WavHeader::default();
    let mut byte_rate: Option<u32> = None;
    let mut data_len: Option<u32> = None;
    // RIFF sizes are unsigned 32-bit; a WAV over 4 GB is out of scope (and is
    // usually RF64, which has a different header anyway).
    let mut offset = 12usize;

    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        // A header that ends mid-chunk is still a WAV; it just cannot tell us any
        // more. Returning the fields read so far is more useful than reporting
        // "not a wav", which would send the caller looking for another parser.
        let Some(size) = u32le(bytes, offset + 4).map(|s| s as usize) else {
            break;
        };
        let body = offset + 8;

        match id {
            b"fmt " => {
                // The fixed part is 16 bytes regardless of the chunk length; a
                // longer chunk only adds extension fields, which this reader
                // does not need. If the chunk itself was cut off, stop here with
                // whatever is legible rather than claiming this is not a WAV.
                let Some(tag) = u16le(bytes, body) else {
                    break;
                };
                header.channels = u16le(bytes, body + 2).map(u32::from);
                header.sample_rate = u32le(bytes, body + 4);
                byte_rate = u32le(bytes, body + 8);
                header.bits = u16le(bytes, body + 14);
                header.is_float = matches!(tag, WAVE_FORMAT_IEEE_FLOAT | WAVE_FORMAT_EXTENSIBLE)
                    && header.bits == Some(32)
                    && tag == WAVE_FORMAT_IEEE_FLOAT;
                if !matches!(
                    tag,
                    WAVE_FORMAT_PCM | WAVE_FORMAT_IEEE_FLOAT | WAVE_FORMAT_EXTENSIBLE
                ) {
                    // Compressed WAV (ADPCM, MP3-in-WAV, ...). The byte rate is an
                    // average, so duration from it would be approximate; leave it
                    // unset rather than report a wrong number.
                    return Some(header);
                }
            }
            b"data" => {
                // A zero size means the writer streamed the data and never
                // backfilled the length. Fall back to "the rest of the file".
                data_len = Some(if size == 0 {
                    (bytes.len() as u32).saturating_sub(body as u32)
                } else {
                    size as u32
                });
                break;
            }
            _ => {}
        }

        // Chunks are word-aligned: an odd size is followed by one pad byte.
        offset = body + size + (size & 1);
    }

    if let (Some(len), Some(rate)) = (data_len, byte_rate) {
        if rate > 0 {
            header.duration_s = Some(len as f64 / rate as f64);
        }
    }
    Some(header)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a WAV with a `fmt ` chunk of `fmt_len` bytes, the way the files that
    /// break symphonia are built: 20-byte `WAVEFORMATEX`.
    fn wav(fmt_len: usize, data_len: u32, channels: u16, rate: u32, bits: u16) -> Vec<u8> {
        let block_align = channels * bits / 8;
        let byte_rate = rate * u32::from(block_align);
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes()); // PCM
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&rate.to_le_bytes());
        fmt.extend_from_slice(&byte_rate.to_le_bytes());
        fmt.extend_from_slice(&block_align.to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        // Extension: cbSize = 2, no further data — this is the 20-byte case.
        while fmt.len() < fmt_len {
            fmt.push(0);
        }

        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(0u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&(fmt_len as u32).to_le_bytes());
        out.extend_from_slice(&fmt);
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.resize(out.len() + data_len as usize, 0);
        out
    }

    #[test]
    fn reads_a_standard_16_byte_fmt_chunk() {
        let bytes = wav(16, 44_100 * 4, 2, 44_100, 16);
        let h = parse_wav_header(&bytes).expect("is a wav");
        assert_eq!(h.channels, Some(2));
        assert_eq!(h.sample_rate, Some(44_100));
        assert_eq!(h.bits, Some(16));
        assert!(!h.is_float);
        let d = h.duration_s.expect("byte rate and data length are known");
        assert!((d - 1.0).abs() < 0.001, "duration {d}");
    }

    #[test]
    fn reads_the_20_byte_fmt_chunk_symphonia_rejects() {
        // The exact shape of the 284 files that failed to probe in the real
        // library: a legal WAVEFORMATEX that symphonia's reader refuses.
        let bytes = wav(20, 44_100 * 4, 2, 44_100, 16);
        let h = parse_wav_header(&bytes).expect("is a wav");
        assert_eq!(h.channels, Some(2));
        assert_eq!(h.sample_rate, Some(44_100));
        assert_eq!(h.duration_s.map(|d| d.round()), Some(1.0));
    }

    #[test]
    fn reads_a_40_byte_fmt_chunk_with_extension_data() {
        let bytes = wav(40, 48_000 * 2, 1, 48_000, 16);
        let h = parse_wav_header(&bytes).expect("is a wav");
        assert_eq!(h.channels, Some(1));
        assert_eq!(h.sample_rate, Some(48_000));
    }

    #[test]
    fn a_non_riff_file_is_not_a_wav() {
        assert!(parse_wav_header(b"this is not a wav file at all").is_none());
        assert!(parse_wav_header(b"RIFF").is_none());
    }

    #[test]
    fn a_truncated_header_yields_what_was_legible() {
        let bytes = wav(16, 1000, 2, 44_100, 16);
        // Cut inside the fmt chunk: nothing useful is readable yet, but the parse
        // must not panic or claim the file is not a wav.
        let h = parse_wav_header(&bytes[..20]);
        assert!(h.is_some());
    }

    #[test]
    fn a_zero_data_size_falls_back_to_the_bytes_present() {
        let mut bytes = wav(16, 0, 2, 44_100, 16);
        bytes.resize(bytes.len() + 8_820, 0);
        let h = parse_wav_header(&bytes).expect("is a wav");
        let d = h.duration_s.expect("duration from the bytes present");
        assert!(d > 0.0, "duration {d}");
    }

    #[test]
    fn a_compressed_wav_reports_no_duration() {
        // Format tag 2 = ADPCM: byte rate is an average, so duration from it would
        // be a guess. Channels and rate are still worth reporting.
        let mut bytes = wav(20, 10_000, 1, 22_050, 4);
        // Overwrite the format tag with ADPCM.
        let tag_at = 12 + 8;
        bytes[tag_at..tag_at + 2].copy_from_slice(&2u16.to_le_bytes());
        let h = parse_wav_header(&bytes).expect("is a wav");
        assert_eq!(h.channels, Some(1));
        assert_eq!(h.duration_s, None);
    }
}
