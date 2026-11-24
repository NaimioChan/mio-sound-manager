//! Header-only metadata probing.
//!
//! The index needs three numbers per file — duration, channel count, sample
//! rate — plus size and mtime for incremental reindexing. All of it comes from
//! the container header; nothing here decodes audio, except for one documented
//! exception (M4A channel count).
//!
//! Cost measured on this machine: 0.09–0.45 ms per file depending on format, so
//! 8 threads probe ~20k files/s. That is fast enough that the walk and the
//! SQLite write are now the comparable costs, which is why the indexer times all
//! three stages separately.

use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const MAX_DESCRIPTION_BYTES: usize = 4 * 1024 * 1024;

/// Extensions the indexer treats as audio. Lowercase, no dot.
///
/// `aac` and `opus` are accepted because the reference implementation's probe
/// handled them and real libraries contain them; the playback path supports the
/// six formats in `docs/architecture.md`, so a file can be indexed and still
/// fail to play. Indexing it is still right: the UI shows it and says why.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "aac", "opus",
];

/// What a header probe can learn. Every field is optional: a file whose header
/// is truncated or in an unexpected variant still gets indexed, with the fields
/// it could not supply left null. Losing a duration is better than losing the
/// file from the library.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioMeta {
    pub duration_s: Option<f64>,
    pub channels: Option<u32>,
    pub sample_rate: Option<u32>,
    pub filesize: u64,
    /// Modification time in nanoseconds since the Unix epoch, the half of the
    /// incremental-reindex key that catches edits which keep the size.
    pub mtime_ns: i64,
    /// True when the file had to be opened with a decoder to fill in a field the
    /// header omitted (currently only M4A channels). Diagnostic only.
    pub decoded_head: bool,
    /// The first useful textual description found in the file's metadata.
    /// Embedded artwork and binary tags are deliberately excluded.
    pub description: Option<String>,
}

/// Is this extension one the indexer keeps?
pub fn is_audio_path(path: &Path) -> bool {
    // macOS leaves `._name.wav` resource forks next to copied files. Office and
    // export tools likewise leave `~$name.wav` and `*.wav.partial` placeholders.
    // They carry an audio extension but are not user audio; a real library copied
    // from a Mac held 9,455 AppleDouble files.
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.starts_with("._") || name.starts_with("~$") {
            return false;
        }
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some(extension) if matches!(extension.to_ascii_lowercase().as_str(), "tmp" | "partial") => {
            false
        }
        Some(extension) => AUDIO_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()),
        None => false,
    }
}

fn mtime_ns(meta: &Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// One file found by the walk, carrying the metadata the directory scan already
/// returned.
///
/// On Windows `std::fs::DirEntry::metadata()` is served from the `FindNextFileW`
/// data, so size and mtime cost nothing here. That matters: probing 280k files
/// spent 41% of its time in `stat` alone when the same numbers were re-fetched
/// two more times (once by `probe_file`, once by the incremental check).
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub filesize: u64,
    /// Nanoseconds since the Unix epoch. The incremental-reindex key.
    pub mtime_ns: i64,
}

/// A directory the walk could not enumerate. Its subtree is unknown, not gone,
/// so a full index pass must preserve existing rows below this path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedDir {
    pub path: String,
    pub detail: String,
}

#[derive(Debug, Default)]
pub struct WalkReport {
    pub files: usize,
    pub skipped_dirs: Vec<SkippedDir>,
}

/// Probe one file. Never fails: an unreadable or unrecognised file yields
/// metadata with everything but size/mtime empty, which the UI renders as "—".
pub fn probe_file(path: &Path) -> AudioMeta {
    let (filesize, mtime) = match std::fs::metadata(path) {
        Ok(m) => (m.len(), mtime_ns(&m)),
        Err(_) => (0, 0),
    };
    probe_with_meta(path, filesize, mtime)
}

/// Probe a file whose size and mtime the caller already has, skipping the `stat`.
pub fn probe_with_meta(path: &Path, filesize: u64, mtime_ns: i64) -> AudioMeta {
    let mut meta = AudioMeta {
        filesize,
        mtime_ns,
        ..Default::default()
    };

    // Metadata is independent of the audio demuxer's duration/channel probe.
    // Read it before opening Symphonia so WAV chunks after `data` and MP3 ID3
    // headers are still visible even when the demuxer stops early.
    meta.description = read_description(path);

    let Ok(file) = File::open(path) else {
        return meta;
    };
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    // The hint is what lets symphonia pick the right demuxer for headerless
    // containers. Cheap, and without it some MP3/AAC files probe as unknown.
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        stream,
        &FormatOptions {
            // Gapless matters here too: with it off, an MP3's frame count
            // includes the encoder delay and the indexed duration is ~25ms long.
            enable_gapless: true,
            ..Default::default()
        },
        &MetadataOptions::default(),
    );

    let mut probed = match probed {
        Ok(p) => p,
        Err(_) => {
            // symphonia gives up on WAV files whose `fmt ` chunk is 20 bytes (a
            // legal WAVEFORMATEX it does not accept) and on any other header
            // variant it does not recognise. Those files are perfectly playable,
            // so fall back to reading the RIFF header directly rather than
            // indexing them with no metadata at all.
            apply_wav_fallback(path, &mut meta);
            return meta;
        }
    };

    let Some(track) = probed.format.default_track().cloned() else {
        return meta;
    };

    meta.sample_rate = track.codec_params.sample_rate;
    meta.channels = track.codec_params.channels.map(|c| c.count() as u32);
    if let (Some(frames), Some(rate)) = (track.codec_params.n_frames, meta.sample_rate) {
        if rate > 0 {
            meta.duration_s = Some(frames as f64 / rate as f64);
        }
    }

    // M4A's channel count is the one field the header does not carry: symphonia's
    // isomp4 demuxer leaves `codec_params.channels` empty until the first packet
    // is decoded, because the value lives in the sample description, not the
    // track header. Decoding one packet costs ~0.3ms and only happens for files
    // that need it.
    if meta.channels.is_none() && needs_decoded_head(path) {
        if let Some((channels, rate)) = decode_head(&mut probed, track.id) {
            meta.channels = Some(channels);
            if meta.sample_rate.is_none() {
                meta.sample_rate = Some(rate);
            }
            if meta.duration_s.is_none() {
                if let (Some(frames), true) = (track.codec_params.n_frames, rate > 0) {
                    meta.duration_s = Some(frames as f64 / rate as f64);
                }
            }
            meta.decoded_head = true;
        }
    }

    meta
}

/// Fill in what the RIFF header says when symphonia would not open the file.
///
/// Only WAV benefits: the other formats' headers are what symphonia already
/// handles, and a second parser for each would be more code than the problem
/// justifies. A file that is not RIFF/WAVE is left alone, so a genuinely broken
/// file still ends up with empty metadata.
fn apply_wav_fallback(path: &Path, meta: &mut AudioMeta) {
    use std::io::Read;

    let is_wav = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "wav" | "wave"))
        .unwrap_or(false);
    if !is_wav {
        return;
    }

    let Ok(mut file) = File::open(path) else {
        return;
    };
    // The fixed part of every chunk this parser needs lives in the first 4 KB,
    // even when the `fmt ` chunk carries a long extension (WAVE_FORMAT_EXTENSIBLE
    // adds 24 bytes of SubFormat GUID).
    let mut head = [0u8; 4096];
    let Ok(read) = file.read(&mut head) else {
        return;
    };

    let Some(header) = crate::wav_header::parse_wav_header(&head[..read]) else {
        return;
    };

    if meta.channels.is_none() {
        meta.channels = header.channels;
    }
    if meta.sample_rate.is_none() {
        meta.sample_rate = header.sample_rate;
    }
    if meta.duration_s.is_none() {
        meta.duration_s = header.duration_s;
    }
    meta.decoded_head = true;
}

fn read_description(path: &Path) -> Option<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "wav" | "wave" => read_wav_description(path),
        "mp3" => read_mp3_description(path),
        _ => None,
    }
}

fn read_wav_description(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut header = [0u8; 12];
    file.read_exact(&mut header).ok()?;
    if &header[..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return None;
    }

    let mut descriptions = Vec::new();
    loop {
        let mut chunk = [0u8; 8];
        if file.read_exact(&mut chunk).is_err() {
            break;
        }
        let size = u32::from_le_bytes(chunk[4..8].try_into().ok()?) as u64;
        let interesting = matches!(&chunk[..4], b"bext" | b"iXML" | b"LIST" | b"ID3 " | b"_PMX");
        let body = if interesting {
            let read_size = size.min(MAX_DESCRIPTION_BYTES as u64) as usize;
            let mut body = vec![0u8; read_size];
            if file.read_exact(&mut body).is_err() {
                break;
            }
            if size > read_size as u64 {
                file.seek(SeekFrom::Current((size - read_size as u64) as i64))
                    .ok()?;
            }
            body
        } else {
            file.seek(SeekFrom::Current(size as i64)).ok()?;
            Vec::new()
        };
        if size & 1 == 1 {
            file.seek(SeekFrom::Current(1)).ok()?;
        }

        match &chunk[..4] {
            b"bext" => {
                if let Some(value) = trim_metadata_text(&body[..body.len().min(256)]) {
                    descriptions.push(value);
                }
            }
            b"iXML" => {
                if let Some(value) = xml_description(&body) {
                    descriptions.push(value);
                }
            }
            b"LIST" => descriptions.extend(riff_info_descriptions(&body)),
            b"ID3 " => descriptions.extend(id3_descriptions(&body)),
            b"_PMX" => {
                if let Some(value) = ascii_description(&body) {
                    descriptions.push(value);
                }
            }
            _ => {}
        }
    }
    choose_description(descriptions)
}

fn read_mp3_description(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut header = [0u8; 10];
    file.read_exact(&mut header).ok()?;
    if &header[..3] != b"ID3" {
        return None;
    }
    let size = header[6..10].iter().fold(0usize, |value, byte| {
        (value << 7) | usize::from(byte & 0x7f)
    });
    let total = (10 + size).min(MAX_DESCRIPTION_BYTES);
    let mut bytes = vec![0u8; total];
    bytes[..10].copy_from_slice(&header);
    file.read_exact(&mut bytes[10..]).ok()?;
    choose_description(id3_descriptions(&bytes))
}

fn riff_info_descriptions(body: &[u8]) -> Vec<String> {
    if body.len() < 4 || &body[..4] != b"INFO" {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = 4usize;
    while at + 8 <= body.len() {
        let id = &body[at..at + 4];
        let size = u32::from_le_bytes(body[at + 4..at + 8].try_into().unwrap()) as usize;
        at += 8;
        let end = at.saturating_add(size).min(body.len());
        if matches!(id, b"ICMT" | b"INAM" | b"IART" | b"IPRD" | b"ISRF") {
            if let Some(value) = trim_metadata_text(&body[at..end]) {
                out.push(value);
            }
        }
        at = end.saturating_add(size & 1);
    }
    out
}

fn id3_descriptions(body: &[u8]) -> Vec<String> {
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::{MetadataBuilder, MetadataRevision, StandardTagKey, Value};
    use symphonia_metadata::id3v2::read_id3v2;

    let mut stream = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(body.to_vec())),
        Default::default(),
    );
    let mut builder = MetadataBuilder::new();
    // Keep this helper local to the parser; ID3 is an optional enhancement and
    // malformed tags must never make a valid audio file disappear from the index.
    if read_id3v2(&mut stream, &mut builder).is_err() {
        return Vec::new();
    }
    let revision: MetadataRevision = builder.metadata();
    revision
        .tags()
        .iter()
        .filter(|tag| {
            matches!(
                tag.std_key,
                Some(StandardTagKey::Comment | StandardTagKey::Description)
            )
        })
        .filter_map(|tag| match &tag.value {
            Value::String(value) => trim_metadata_text(value.as_bytes()),
            _ => None,
        })
        .collect()
}

fn xml_description(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    let lower = text.to_ascii_lowercase();
    let start = lower.find("<description")?;
    let value_start = text[start..].find('>')? + start + 1;
    let value_end = lower[value_start..].find("</description>")? + value_start;
    trim_metadata_text(text[value_start..value_end].as_bytes())
}

fn ascii_description(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    let mut words = Vec::new();
    for part in text.split(|c: char| !c.is_ascii_graphic() && c != ' ') {
        if let Some(value) = trim_metadata_text(part.as_bytes()) {
            words.push(value);
        }
    }
    choose_description(words)
}

fn trim_metadata_text(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes)
        .trim_matches('\0')
        .trim()
        .to_string();
    if text.len() < 3 || printable_ratio(&text) < 0.95 {
        return None;
    }
    Some(text)
}

fn printable_ratio(text: &str) -> f64 {
    let total = text.chars().count();
    if total == 0 {
        return 0.0;
    }
    text.chars()
        .filter(|c| {
            *c != '\u{fffd}' && (c.is_ascii_graphic() || c.is_ascii_whitespace() || !c.is_control())
        })
        .count() as f64
        / total as f64
}

fn choose_description(values: Vec<String>) -> Option<String> {
    let mut unique = Vec::new();
    for value in values {
        if !unique
            .iter()
            .any(|existing: &String| metadata_equivalent(existing, &value))
        {
            unique.push(value);
        }
    }
    unique.into_iter().next()
}

fn metadata_equivalent(left: &str, right: &str) -> bool {
    normalise_metadata(left) == normalise_metadata(right)
}

fn normalise_metadata(value: &str) -> String {
    value
        .split(|c: char| matches!(c, '\u{1a}' | ',' | '-'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Only MP4-family containers need the decode-the-first-packet fallback. Gating
/// on the extension keeps a genuinely channel-less file in another format from
/// paying for a decode that would not help.
fn needs_decoded_head(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "m4a" | "m4b" | "mp4" | "aac"
            )
        })
        .unwrap_or(false)
}

fn decode_head(
    probed: &mut symphonia::core::probe::ProbeResult,
    track_id: u32,
) -> Option<(u32, u32)> {
    let track = probed.format.tracks().iter().find(|t| t.id == track_id)?;
    let params = track.codec_params.clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .ok()?;

    for _ in 0..8 {
        let packet = match probed.format.next_packet() {
            Ok(p) => p,
            // End of stream or a truncated file: nothing to learn.
            Err(SymphoniaError::IoError(_)) => return None,
            Err(_) => continue,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => return None,
        };
        let spec = decoded.spec();
        return Some((spec.channels.count() as u32, spec.rate));
    }
    None
}

/// Every audio file under `root`, sorted so two runs produce the same row order
/// (which keeps the batch boundaries — and therefore the timings — comparable).
///
/// Symlinked directories are not followed: a library that links back to itself
/// would otherwise walk forever, and the reference implementation did not follow
/// them either.
pub fn walk_audio_files(root: &Path) -> crate::error::Result<Vec<PathBuf>> {
    Ok(walk_audio_entries(root)?
        .into_iter()
        .map(|entry| entry.path)
        .collect())
}

/// Same walk, but keeping the size and mtime the directory scan already returned.
///
/// The indexer wants both for the incremental check and for the row, so getting
/// them here removes two `stat` calls per file. Measured on 280k files: `stat`
/// alone was 8.2 s of a 95 s pass.
pub fn walk_audio_entries(root: &Path) -> crate::error::Result<Vec<FileEntry>> {
    let mut files = Vec::new();
    walk_audio_entries_into(root, |entry| {
        files.push(entry);
        Ok(())
    })?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Walk every directory or fail; callers that prune must never use a partial scan.
pub fn try_walk_audio_entries(root: &Path) -> crate::error::Result<Vec<FileEntry>> {
    walk_audio_entries(root)
}

pub fn walk_audio_entries_into<F>(root: &Path, visit: F) -> crate::error::Result<usize>
where
    F: FnMut(FileEntry) -> crate::error::Result<()>,
{
    Ok(walk_audio_entries_with_report(root, visit)?.files)
}

pub fn walk_audio_entries_with_report<F>(
    root: &Path,
    mut visit: F,
) -> crate::error::Result<WalkReport>
where
    F: FnMut(FileEntry) -> crate::error::Result<()>,
{
    let root_handle = std::fs::read_dir(root).map_err(|e| crate::error::IndexError::Walk {
        path: root.display().to_string(),
        detail: e.to_string(),
    })?;
    drop(root_handle);

    let mut report = WalkReport::default();
    for result in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                let path = error.path().unwrap_or(root);
                if path == root {
                    return Err(crate::error::IndexError::Walk {
                        path: path.display().to_string(),
                        detail: error.to_string(),
                    });
                }
                report.skipped_dirs.push(SkippedDir {
                    path: crate::folders::store_path(path),
                    detail: error.to_string(),
                });
                continue;
            }
        };
        if !entry.file_type().is_file() || !is_audio_path(entry.path()) {
            continue;
        }
        // `DirEntry::metadata` on Windows comes from the directory scan
        // (`FindNextFileW`), not a second open. On other platforms it is a
        // stat, which is still no worse than doing it later.
        let meta = entry.metadata().ok();
        visit(FileEntry {
            path: entry.into_path(),
            filesize: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            mtime_ns: meta.as_ref().map(mtime_ns).unwrap_or(0),
        })?;
        report.files += 1;
    }
    report.skipped_dirs.sort_by(|a, b| a.path.cmp(&b.path));
    report.skipped_dirs.dedup_by(|a, b| a.path == b.path);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join("mio-probe-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut f = File::create(&path).unwrap();
        f.write_all(bytes).unwrap();
        path
    }

    /// A minimal 44.1k / stereo / 16-bit WAV with `frames` samples of silence.
    fn wav_bytes(frames: u32) -> Vec<u8> {
        let data_len = frames * 4;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&44_100u32.to_le_bytes());
        out.extend_from_slice(&176_400u32.to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.resize(44 + data_len as usize, 0);
        out
    }

    #[test]
    fn reads_duration_channels_and_rate_from_a_wav_header() {
        let path = write_temp("one-second.wav", &wav_bytes(44_100));
        let meta = probe_file(&path);
        assert_eq!(meta.sample_rate, Some(44_100));
        assert_eq!(meta.channels, Some(2));
        let duration = meta.duration_s.expect("header carries a frame count");
        assert!((duration - 1.0).abs() < 0.001, "duration was {duration}");
        assert_eq!(meta.filesize, 44 + 44_100 * 4);
        assert!(meta.mtime_ns > 0);
        assert!(!meta.decoded_head);
    }

    #[test]
    fn a_directory_disappearing_during_walk_is_an_error() {
        let root = std::env::temp_dir().join(format!(
            "mio-walk-race-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("nested/gone.wav"), wav_bytes(10)).unwrap();
        let mut visited = 0;
        let error = walk_audio_entries_into(&root, |_| {
            visited += 1;
            Err(crate::error::IndexError::Walk {
                path: root.join("nested").display().to_string(),
                detail: "mock directory disappeared".into(),
            })
        })
        .unwrap_err();

        assert_eq!(visited, 1);
        assert!(error.to_string().contains("mock directory disappeared"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unreadable_file_still_yields_size_and_mtime() {
        let path = write_temp("not-audio.wav", b"this is not a riff file at all");
        let meta = probe_file(&path);
        assert_eq!(meta.duration_s, None);
        assert_eq!(meta.channels, None);
        assert_eq!(meta.sample_rate, None);
        assert_eq!(
            meta.filesize,
            b"this is not a riff file at all".len() as u64
        );
        assert!(meta.mtime_ns > 0);
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let meta = probe_file(Path::new("C:/definitely/not/here.wav"));
        assert_eq!(meta.filesize, 0);
        assert_eq!(meta.duration_s, None);
    }

    #[test]
    fn extension_filter_is_case_insensitive_and_excludes_non_audio() {
        assert!(is_audio_path(Path::new("a/B.WAV")));
        assert!(is_audio_path(Path::new("b.flac")));
        assert!(is_audio_path(Path::new(".hidden.wav")));
        assert!(!is_audio_path(Path::new("c.txt")));
        assert!(!is_audio_path(Path::new("noextension")));
        assert!(!is_audio_path(Path::new("scratch.tmp")));
        assert!(!is_audio_path(Path::new("render.wav.partial")));
        assert!(!is_audio_path(Path::new("~$draft.wav")));
        assert!(!is_audio_path(Path::new("._real.wav")));
    }

    #[test]
    fn reads_a_description_from_a_chunk_after_data() {
        let mut bytes = wav_bytes(10);
        bytes.extend_from_slice(b"bext");
        bytes.extend_from_slice(&256u32.to_le_bytes());
        let mut description = [0u8; 256];
        description[..18].copy_from_slice(b"Metal, Impact, Hit");
        bytes.extend_from_slice(&description);
        let path = write_temp("description-after-data.wav", &bytes);
        let meta = probe_file(&path);
        assert_eq!(meta.description.as_deref(), Some("Metal, Impact, Hit"));
    }

    #[test]
    fn rejects_binary_bext_garbage() {
        let mut bytes = wav_bytes(10);
        bytes.extend_from_slice(b"bext");
        bytes.extend_from_slice(&256u32.to_le_bytes());
        bytes.extend_from_slice(&[0xff; 256]);
        let path = write_temp("description-garbage.wav", &bytes);
        assert_eq!(probe_file(&path).description, None);
    }
}
