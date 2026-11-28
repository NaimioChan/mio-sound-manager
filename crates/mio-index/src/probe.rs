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
//!
//! [`read_details_checked`] is the slower second pass: it opens the tag containers
//! and the embedded pictures that [`probe_basic_with_meta`] deliberately skips. It
//! shares this module's RIFF walker and ID3 reader rather than growing a second
//! parser for the same chunks.

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

use crate::details::{self, EmbeddedDetails};

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
    /// Official UCS category/subcategory declared by embedded metadata, when present.
    pub ucs_category: Option<String>,
    pub ucs_subcategory: Option<String>,
}

/// Embedded fields read by the optional metadata pass. The basic index never
/// opens the metadata chunks, so a large library can become searchable first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EmbeddedMetadata {
    pub description: Option<String>,
    pub ucs_category: Option<String>,
    pub ucs_subcategory: Option<String>,
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
    probe_with_options(path, filesize, mtime_ns, true)
}

/// Probe only the fields required by the basic index.
pub fn probe_basic_with_meta(path: &Path, filesize: u64, mtime_ns: i64) -> AudioMeta {
    probe_with_options(path, filesize, mtime_ns, false)
}

fn probe_with_options(
    path: &Path,
    filesize: u64,
    mtime_ns: i64,
    read_embedded_metadata: bool,
) -> AudioMeta {
    let mut meta = AudioMeta {
        filesize,
        mtime_ns,
        ..Default::default()
    };

    // Metadata is independent of the audio demuxer's duration/channel probe.
    // Read it before opening Symphonia so WAV chunks after `data` and MP3 ID3
    // headers are still visible even when the demuxer stops early.
    if read_embedded_metadata {
        let metadata = read_metadata(path);
        meta.description = metadata.description;
        meta.ucs_category = metadata.ucs_category;
        meta.ucs_subcategory = metadata.ucs_subcategory;
    }

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

/// Read from a single open handle. `Ok(None)` means the file fingerprint changed
/// since the base index; leave that row pending for the next pass.
pub fn metadata_only_with_meta_checked(
    path: &Path,
    filesize: u64,
    expected_mtime_ns: i64,
) -> std::io::Result<Option<EmbeddedMetadata>> {
    let mut file = File::open(path)?;
    let matches = |meta: Metadata| meta.len() == filesize && mtime_ns(&meta) == expected_mtime_ns;
    if !matches(file.metadata()?) {
        return Ok(None);
    }
    let metadata = read_metadata_from(path, &mut file);
    Ok(matches(file.metadata()?).then(|| metadata.into()))
}

/// Raw tags and cover bytes for the details pass, from a single open handle.
///
/// Same fingerprint contract as [`metadata_only_with_meta_checked`]: `Ok(None)`
/// means the file changed under the pass and this row is deferred, not failed.
/// The fingerprint is re-checked after extraction because decoding a picture can
/// take long enough for the file to be replaced.
pub fn details_only_with_meta_checked(
    path: &Path,
    filesize: u64,
    expected_mtime_ns: i64,
) -> std::io::Result<Option<EmbeddedDetails>> {
    let mut file = File::open(path)?;
    let matches = |meta: Metadata| meta.len() == filesize && mtime_ns(&meta) == expected_mtime_ns;
    if !matches(file.metadata()?) {
        return Ok(None);
    }
    let details = read_details_from(path, &mut file);
    Ok(matches(file.metadata()?).then_some(details))
}

/// Lazy version for the inspector: no fingerprint, reads whatever is on disk now.
///
/// This is the "show the current file even though the pass has not reached it"
/// path. It deliberately does not write anything: the tags it returns are for
/// display, and a value it shows is not necessarily in the search index yet.
pub fn read_details_now(path: &Path) -> std::io::Result<EmbeddedDetails> {
    let mut file = File::open(path)?;
    Ok(read_details_from(path, &mut file))
}

fn read_details_from(path: &Path, file: &mut File) -> EmbeddedDetails {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let mut out = EmbeddedDetails::default();
    match ext.as_str() {
        "wav" | "wave" => read_wav_details(file, &mut out),
        "aif" | "aiff" => read_aiff_details(file, &mut out),
        _ => read_symphonia_details(path, &mut out),
    }
    details::finalize(out)
}

/// The RIFF walk WAV tags need.
///
/// One walker serves both passes: the metadata pass asks for the description
/// fields, this one asks for every tag and the picture. Copying the chunk walker
/// would mean two places to fix the `data`-chunk-past-EOF case the first one
/// already handles.
fn read_wav_details(file: &mut File, out: &mut EmbeddedDetails) {
    let mut header = [0u8; 12];
    if file.read_exact(&mut header).is_err() {
        return;
    }
    if &header[..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return;
    }
    loop {
        let mut chunk = [0u8; 8];
        if file.read_exact(&mut chunk).is_err() {
            break;
        }
        let size = u32::from_le_bytes(chunk[4..8].try_into().unwrap()) as u64;
        let interesting = matches!(&chunk[..4], b"bext" | b"iXML" | b"LIST" | b"ID3 " | b"_PMX");
        let body = if interesting {
            let read_size = size.min(MAX_DESCRIPTION_BYTES as u64) as usize;
            let mut body = vec![0u8; read_size];
            if file.read_exact(&mut body).is_err() {
                break;
            }
            if size > read_size as u64 {
                if file
                    .seek(SeekFrom::Current((size - read_size as u64) as i64))
                    .is_err()
                {
                    break;
                }
            }
            body
        } else {
            if file.seek(SeekFrom::Current(size as i64)).is_err() {
                break;
            }
            Vec::new()
        };
        if size & 1 == 1 {
            if file.seek(SeekFrom::Current(1)).is_err() {
                break;
            }
        }

        match &chunk[..4] {
            b"bext" => bext_tags(&body, out),
            b"iXML" => ixml_tags(&body, out),
            b"LIST" => info_tags(&body, out),
            b"ID3 " => id3_details(&body, out),
            b"_PMX" => {
                if let Some(value) = ascii_description(&body) {
                    details::push_field(out, "info", "info.comment", &value);
                }
            }
            _ => {}
        }
    }
}

/// AIFF tags: an optional embedded `ID3 ` chunk. Other metadata lives in the
/// format's own chunks and is not worth a second parser for the share of real
/// libraries that use it; the details pass still marks the file done.
fn read_aiff_details(file: &mut File, out: &mut EmbeddedDetails) {
    let mut header = [0u8; 12];
    if file.read_exact(&mut header).is_err() {
        return;
    }
    if &header[..4] != b"FORM" || &header[8..12] != b"AIFF" {
        return;
    }
    loop {
        let mut chunk = [0u8; 8];
        if file.read_exact(&mut chunk).is_err() {
            break;
        }
        let size = u32::from_be_bytes(chunk[4..8].try_into().unwrap()) as u64;
        if &chunk[..4] == b"ID3 " {
            let read_size = size.min(MAX_DESCRIPTION_BYTES as u64) as usize;
            let mut body = vec![0u8; read_size];
            if file.read_exact(&mut body).is_err() {
                break;
            }
            id3_details(&body, out);
        } else if file.seek(SeekFrom::Current(size as i64)).is_err() {
            break;
        }
        if size & 1 == 1 && file.seek(SeekFrom::Current(1)).is_err() {
            break;
        }
    }
}

/// Formats whose metadata symphonia already parses correctly (FLAC, OGG, M4A,
/// MP3). The reader is opened with the metadata limits the pass wants and every
/// revision in the log is read, because OGG stores a picture as its own revision.
fn read_symphonia_details(path: &Path, out: &mut EmbeddedDetails) {
    let Ok(file) = File::open(path) else {
        return;
    };
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut probed = match symphonia::default::get_probe().format(
        &hint,
        stream,
        &FormatOptions::default(),
        &details::metadata_options(),
    ) {
        Ok(probed) => probed,
        Err(_) => return,
    };
    let source = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("flac") => "vorbis",
        Some("ogg") | Some("opus") => "vorbis",
        Some("m4a") | Some("m4b") | Some("mp4") | Some("aac") => "m4a",
        _ => "id3",
    };
    let mut metadata = probed.format.metadata();
    // All revisions, oldest first. A tag written later usually supersedes an
    // earlier one, but the inspector shows what the file declares; duplicates are
    // collapsed by `finalize`.
    loop {
        if let Some(revision) = metadata.current() {
            details::from_revision(revision, source, out);
        }
        if metadata.pop().is_none() {
            break;
        }
    }
}

fn bext_tags(body: &[u8], out: &mut EmbeddedDetails) {
    // The BWF `bext` chunk: fixed offsets for description (0..256),
    // originator (256..288), originator reference (288..320), date (320..330),
    // time (330..338), time reference low (338..342), high (342..346), then a
    // version field and a free-form coding history.
    let field = |range: std::ops::Range<usize>| -> Option<String> {
        if body.len() < range.end {
            return None;
        }
        let text = String::from_utf8_lossy(&body[range]).to_string();
        let text = text.trim_matches('\0').trim().to_string();
        (!text.is_empty()).then_some(text)
    };
    let mut push = |key: &str, value: Option<String>| {
        if let Some(value) = value {
            details::push_field(out, "bext", key, &value);
        }
    };
    push("bext.description", field(0..256));
    push("bext.originator", field(256..288));
    push("bext.originator_reference", field(288..320));
    push("bext.origination_date", field(320..330));
    push("bext.origination_time", field(330..338));
    if body.len() >= 346 {
        let low = u32::from_le_bytes(body[338..342].try_into().unwrap()) as u64;
        let high = u32::from_le_bytes(body[342..346].try_into().unwrap()) as u64;
        let reference = (high << 32) | low;
        if reference > 0 {
            details::push_field(out, "bext", "bext.time_reference", &reference.to_string());
        }
    }
    // Coding history starts after the 602-byte fixed part (or 348 for version 0)
    // and runs to the end of the chunk. It is prose about the recording chain,
    // which is exactly the "where did this come from" note the inspector wants.
    let history_start = if body.len() > 602 { 602 } else { 348 };
    if body.len() > history_start {
        let history = String::from_utf8_lossy(&body[history_start..]).to_string();
        let history = history.trim_matches('\0').trim().to_string();
        if !history.is_empty() {
            details::push_field(out, "bext", "bext.coding_history", &history);
        }
    }
}

fn ixml_tags(body: &[u8], out: &mut EmbeddedDetails) {
    let text = String::from_utf8_lossy(body);
    let lower = text.to_ascii_lowercase();
    // `ATTR` blocks: `<ATTR><NAME>Foo</NAME><VALUE>Bar</VALUE></ATTR>`, which is
    // what Pro Tools and most recorders write.
    let mut at = 0;
    while let Some(relative) = lower[at..].find("<attr>") {
        let start = at + relative;
        let Some(end) = lower[start..].find("</attr>").map(|offset| start + offset) else {
            break;
        };
        let block = &text[start..end];
        let block_lower = &lower[start..end];
        if let (Some(name_start), Some(value_start)) = (block_lower.find("<name>"), block_lower.find("<value>")) {
            let name_start = name_start + "<name>".len();
            let value_start = value_start + "<value>".len();
            if let (Some(name_end), Some(value_end)) =
                (block_lower[name_start..].find("</name>"), block_lower[value_start..].find("</value>"))
            {
                let name = block[name_start..name_start + name_end].trim();
                let value = block[value_start..value_start + value_end].trim();
                if let Some(key) = details::ixml_key(name) {
                    details::push_field(out, "ixml", key, value);
                } else if !name.is_empty() {
                    details::push_field(out, "ixml", &format!("ixml.{}", name.to_ascii_lowercase()), value);
                }
            }
        }
        at = end + "</attr>".len();
    }
    // Unadorned elements: `<PROJECT>Foo</PROJECT>`. Text that is itself a dump is
    // refused downstream by `clean_value`, so no filtering happens here.
    for (raw_name, key) in [
        ("PROJECT", "ixml.project"),
        ("SCENE", "ixml.scene"),
        ("TAKE", "ixml.take"),
        ("TAPE", "ixml.tape"),
        ("NOTE", "ixml.note"),
        ("CIRCLED", "ixml.circled"),
        ("CATEGORY", "ixml.category"),
        ("SUBCATEGORY", "ixml.subcategory"),
    ] {
        let open = format!("<{}>", raw_name.to_ascii_lowercase());
        let close = format!("</{}>", raw_name.to_ascii_lowercase());
        let mut cursor = 0;
        while let Some(relative) = lower[cursor..].find(&open) {
            let value_start = cursor + relative + open.len();
            let Some(value_end) = lower[value_start..].find(&close).map(|offset| value_start + offset) else {
                break;
            };
            details::push_field(out, "ixml", key, text[value_start..value_end].trim());
            cursor = value_end + close.len();
        }
    }
}

fn info_tags(body: &[u8], out: &mut EmbeddedDetails) {
    if body.len() < 4 || &body[..4] != b"INFO" {
        return;
    }
    let mut at = 4usize;
    while at + 8 <= body.len() {
        let code = &body[at..at + 4];
        let size = u32::from_le_bytes(body[at + 4..at + 8].try_into().unwrap()) as usize;
        at += 8;
        let end = at.saturating_add(size).min(body.len());
        let value = String::from_utf8_lossy(&body[at..end]).to_string();
        let key = match details::info_key(code) {
            Some(key) => key,
            None => {
                let text = String::from_utf8_lossy(code).to_string();
                // An unknown code is still a field: show it under its own name.
                details::push_field(out, "info", &format!("info.{text}"), &value);
                at = end.saturating_add(size & 1);
                continue;
            }
        };
        details::push_field(out, "info", key, &value);
        at = end.saturating_add(size & 1);
    }
}

fn id3_details(body: &[u8], out: &mut EmbeddedDetails) {
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataBuilder;
    use symphonia_metadata::id3v2::read_id3v2;

    let mut stream = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(body.to_vec())),
        Default::default(),
    );
    let mut builder = MetadataBuilder::new();
    if read_id3v2(&mut stream, &mut builder).is_err() {
        return;
    }
    let revision = builder.metadata();
    details::from_revision(&revision, "id3", out);
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

#[derive(Debug, Default)]
struct FileMetadata {
    description: Option<String>,
    ucs_category: Option<String>,
    ucs_subcategory: Option<String>,
}

impl From<FileMetadata> for EmbeddedMetadata {
    fn from(metadata: FileMetadata) -> Self {
        Self {
            description: metadata.description,
            ucs_category: metadata.ucs_category,
            ucs_subcategory: metadata.ucs_subcategory,
        }
    }
}

fn read_metadata(path: &Path) -> FileMetadata {
    let Ok(mut file) = File::open(path) else {
        return FileMetadata::default();
    };
    read_metadata_from(path, &mut file)
}

fn read_metadata_from(path: &Path, file: &mut File) -> FileMetadata {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return FileMetadata::default();
    };
    let ext = ext.to_ascii_lowercase();
    match ext.as_str() {
        "wav" | "wave" => read_wav_metadata(file),
        "mp3" => FileMetadata {
            description: read_mp3_description(file),
            ..Default::default()
        },
        _ => FileMetadata::default(),
    }
}

fn read_wav_metadata(file: &mut File) -> FileMetadata {
    let mut header = [0u8; 12];
    if file.read_exact(&mut header).is_err() {
        return FileMetadata::default();
    }
    if &header[..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return FileMetadata::default();
    }

    let mut descriptions = Vec::new();
    let mut ucs = None;
    loop {
        let mut chunk = [0u8; 8];
        if file.read_exact(&mut chunk).is_err() {
            break;
        }
        let size = u32::from_le_bytes(chunk[4..8].try_into().unwrap()) as u64;
        let interesting = matches!(&chunk[..4], b"bext" | b"iXML" | b"LIST" | b"ID3 " | b"_PMX");
        let body = if interesting {
            let read_size = size.min(MAX_DESCRIPTION_BYTES as u64) as usize;
            let mut body = vec![0u8; read_size];
            if file.read_exact(&mut body).is_err() {
                break;
            }
            if size > read_size as u64 {
                if file
                    .seek(SeekFrom::Current((size - read_size as u64) as i64))
                    .is_err()
                {
                    break;
                }
            }
            body
        } else {
            if file.seek(SeekFrom::Current(size as i64)).is_err() {
                break;
            }
            Vec::new()
        };
        if size & 1 == 1 {
            if file.seek(SeekFrom::Current(1)).is_err() {
                break;
            }
        }

        match &chunk[..4] {
            b"bext" => {
                if let Some(value) = trim_metadata_text(&body[..body.len().min(256)]) {
                    descriptions.push(value);
                }
            }
            b"iXML" => {
                descriptions.extend(xml_descriptions(&body));
                ucs = xml_musical_category(&body).or(ucs);
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
    FileMetadata {
        description: choose_description(descriptions),
        ucs_category: ucs.as_ref().map(|value| value.0.clone()),
        ucs_subcategory: ucs.and_then(|value| value.1),
    }
}

fn read_mp3_description(file: &mut File) -> Option<String> {
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

fn xml_descriptions(body: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(body);
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(relative) = lower[at..].find("<description") {
        let start = at + relative;
        let Some(value_start) = text[start..].find('>').map(|offset| start + offset + 1) else {
            break;
        };
        let Some(value_end) = lower[value_start..]
            .find("</description>")
            .map(|offset| value_start + offset)
        else {
            break;
        };
        if let Some(value) = trim_metadata_text(text[value_start..value_end].as_bytes()) {
            out.push(value);
        }
        at = value_end + "</description>".len();
    }
    out
}

fn xml_musical_category(body: &[u8]) -> Option<(String, Option<String>)> {
    let text = String::from_utf8_lossy(body);
    let lower = text.to_ascii_lowercase();
    let mut at = 0;
    while let Some(relative) = lower[at..].find("<attr>") {
        let start = at + relative;
        let end = lower[start..]
            .find("</attr>")
            .map(|offset| start + offset)?;
        let block = &text[start..end];
        let block_lower = block.to_ascii_lowercase();
        let name_start = block_lower.find("<name>")? + "<name>".len();
        let name_end = block_lower[name_start..].find("</name>")? + name_start;
        if block[name_start..name_end]
            .trim()
            .eq_ignore_ascii_case("MusicalCategory")
        {
            let value_start = block_lower.find("<value>")? + "<value>".len();
            let value_end = block_lower[value_start..].find("</value>")? + value_start;
            let mut parts = block[value_start..value_end]
                .split('/')
                .map(str::trim)
                .filter(|part| !part.is_empty());
            let category = parts.next()?.to_ascii_uppercase();
            let subcategory = parts.next().map(|part| part.to_ascii_uppercase());
            return Some((category, subcategory));
        }
        at = end + "</attr>".len();
    }
    None
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

    // Metadata writers often put a useful phrase beside a technical dump in
    // the same field. Keep meaningful segments and discard lines/clauses that
    // only describe the container, encoder or recording equipment.
    let segments: Vec<String> = text
        .split(|c: char| matches!(c, '\r' | '\n' | '|' | ';' | ','))
        .filter_map(clean_metadata_segment)
        .collect();
    if segments.is_empty() {
        return None;
    }
    Some(segments.join(", "))
}

fn clean_metadata_segment(segment: &str) -> Option<String> {
    let text = segment
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = text.trim();
    if text.len() < 3 || !text.chars().any(|c| c.is_alphabetic()) {
        return None;
    }

    let lower = text.to_ascii_lowercase();
    let digits = text.chars().filter(|c| c.is_ascii_digit()).count();
    let technical = [
        "sample rate",
        "samplerate",
        "bit depth",
        "bitdepth",
        "bits per sample",
        "channel count",
        "channels",
        "stereo",
        "mono",
        "duration",
        "codec",
        "container",
        "bitrate",
        "kilobit",
        "sample format",
        "file format",
        "device",
        "serial number",
        "firmware",
        "guid",
        "uuid",
        "hash",
        "checksum",
        "md5",
        "sha1",
        "sha256",
    ];
    let has_technical_term = technical.iter().any(|term| lower.contains(term));
    let field_label = text
        .split_once(':')
        .map(|(label, _)| technical.iter().any(|term| label.trim().to_ascii_lowercase().contains(term)))
        .unwrap_or(false);
    let unit_dump = lower.contains(" hz")
        || lower.contains("khz")
        || lower.contains(" khz")
        || lower.contains(" bit")
        || lower.contains("bits")
        || lower.contains(" bps")
        || lower.contains(" kbps")
        || lower.contains(" mbps")
        || lower.contains(" sec")
        || lower.contains(" ms");

    // A long hexadecimal token is normally a GUID, hash, or binary tag that
    // happened to survive lossy UTF-8 conversion. Do not put it in FTS.
    if has_long_hex_token(text) {
        return None;
    }
    if field_label || has_technical_term || (unit_dump && digits > 0) {
        return None;
    }
    if digits >= 4 && digits * 3 > text.chars().filter(|c| c.is_alphanumeric()).count() {
        return None;
    }

    Some(text.to_string())
}

fn has_long_hex_token(text: &str) -> bool {
    text.split(|c: char| !c.is_ascii_hexdigit())
        .any(|token| token.len() >= 24 && token.chars().any(|c| c.is_ascii_digit()))
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
    fn basic_probe_does_not_read_embedded_description_or_ucs() {
        let mut bytes = wav_bytes(10);
        bytes.extend_from_slice(b"bext");
        bytes.extend_from_slice(&256u32.to_le_bytes());
        let mut description = [0u8; 256];
        description[..18].copy_from_slice(b"Metal, Impact, Hit");
        bytes.extend_from_slice(&description);
        let path = write_temp("basic-probe-skips-metadata.wav", &bytes);
        let meta = probe_basic_with_meta(&path, bytes.len() as u64, 7);
        assert_eq!(meta.duration_s, Some(10.0 / 44_100.0));
        assert_eq!(meta.description, None);
        assert_eq!(meta.ucs_category, None);
        assert_eq!(meta.ucs_subcategory, None);
    }

    #[test]
    fn reads_ucs_musical_category_from_ixml() {
        let mut bytes = wav_bytes(10);
        let xml = br#"<?xml version="1.0"?><BWFXML><STEINBERG><ATTR_LIST><ATTR><NAME>MusicalCategory</NAME><VALUE>AIRCRAFT / JET</VALUE></ATTR></ATTR_LIST></STEINBERG></BWFXML>"#;
        bytes.extend_from_slice(b"iXML");
        bytes.extend_from_slice(&(xml.len() as u32).to_le_bytes());
        bytes.extend_from_slice(xml);
        let path = write_temp("ucs-ixml.wav", &bytes);
        let meta = probe_file(&path);
        assert_eq!(meta.ucs_category.as_deref(), Some("AIRCRAFT"));
        assert_eq!(meta.ucs_subcategory.as_deref(), Some("JET"));
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

    #[test]
    fn keeps_meaningful_prose_and_drops_technical_metadata() {
        assert_eq!(
            trim_metadata_text(b"Metal impact, 44100 Hz, 24 bit, stereo"),
            Some("Metal impact".into())
        );
        assert_eq!(
            trim_metadata_text(b"Sample Rate: 44100 Hz\nBit Depth: 24\nChannels: 2"),
            None
        );
        assert_eq!(
            trim_metadata_text(b"A warm metal impact recorded in a small room"),
            Some("A warm metal impact recorded in a small room".into())
        );
    }

    #[test]
    fn drops_hex_hashes_and_applies_the_same_rule_to_metadata_sources() {
        assert_eq!(
            trim_metadata_text(b"8f4a9d11c2e7aa00ff1133557799abcd0011223344556677"),
            None
        );
        assert!(xml_descriptions(b"<description>Sample Rate: 48000 Hz</description>").is_empty());
        assert!(riff_info_descriptions(b"INFOICMT\x13\0\0\0Codec: PCM 24 bit").is_empty());
        assert_eq!(ascii_description(b"Metal impact\0\0"), Some("Metal impact".into()));
        assert_eq!(ascii_description(b"00112233445566778899aabbccddeeff"), None);
    }

    /// A WAV with one chunk appended after the audio data. `size` is the payload
    /// length, and the RIFF size is left wrong on purpose: the walker never trusts
    /// it, and real files with editors' junk chunks often have a stale one.
    fn wav_with_chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut bytes = wav_bytes(10);
        bytes.extend_from_slice(id);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        if payload.len() & 1 == 1 {
            bytes.push(0);
        }
        bytes
    }

    fn tag_value<'a>(details: &'a EmbeddedDetails, key: &str) -> Option<&'a str> {
        details
            .tags
            .iter()
            .find(|tag| tag.key == key)
            .map(|tag| tag.value.as_str())
    }

    #[test]
    fn bext_tags_keep_the_originator_and_the_recording_date() {
        let mut full = vec![0u8; 602];
        full[0..13].copy_from_slice(b"Door slam 03.");
        full[256..266].copy_from_slice(b"Naimio Rec");
        full[320..330].copy_from_slice(b"2026-11-28");
        full[330..338].copy_from_slice(b"14:03:22");
        full.extend_from_slice(b"A=96 kHz\r\nF=24 bit\r\n");
        let bytes = wav_with_chunk(b"bext", &full);
        let path = write_temp("details-bext.wav", &bytes);
        let details = read_details_now(&path).unwrap();
        assert_eq!(tag_value(&details, "bext.description"), Some("Door slam 03."));
        assert_eq!(tag_value(&details, "bext.originator"), Some("Naimio Rec"));
        assert_eq!(
            tag_value(&details, "bext.origination_date"),
            Some("2026-11-28")
        );
        assert!(tag_value(&details, "bext.coding_history").is_some());
    }

    #[test]
    fn ixml_tags_keep_attr_pairs_and_bare_elements() {
        let xml = br#"<BWFXML><PROJECT>Rain Pack</PROJECT><SCENE>Alley</SCENE><STEINBERG><ATTR_LIST><ATTR><NAME>TAKE</NAME><VALUE>7</VALUE></ATTR></ATTR_LIST></STEINBERG></BWFXML>"#;
        let bytes = wav_with_chunk(b"iXML", xml);
        let path = write_temp("details-ixml.wav", &bytes);
        let details = read_details_now(&path).unwrap();
        assert_eq!(tag_value(&details, "ixml.project"), Some("Rain Pack"));
        assert_eq!(tag_value(&details, "ixml.scene"), Some("Alley"));
        assert_eq!(tag_value(&details, "ixml.take"), Some("7"));
    }

    #[test]
    fn list_info_tags_use_their_known_codes_and_keep_unknown_ones() {
        let mut body = b"INFO".to_vec();
        let push = |body: &mut Vec<u8>, code: &[u8; 4], value: &[u8]| {
            body.extend_from_slice(code);
            body.extend_from_slice(&(value.len() as u32).to_le_bytes());
            body.extend_from_slice(value);
            // The RIFF pad byte is outside the declared size and only exists for
            // an odd payload: writing it unconditionally shifted every following
            // code by one byte, which is exactly the bug this test would hide.
            if value.len() & 1 == 1 {
                body.push(0);
            }
        };
        push(&mut body, b"IART", b"Naimio");
        push(&mut body, b"INAM", b"Door Slam");
        push(&mut body, b"ZZZZ", b"custom note");
        let bytes = wav_with_chunk(b"LIST", &body);
        let path = write_temp("details-info.wav", &bytes);
        let details = read_details_now(&path).unwrap();
        assert_eq!(tag_value(&details, "info.artist"), Some("Naimio"));
        assert_eq!(tag_value(&details, "info.title"), Some("Door Slam"));
        assert_eq!(tag_value(&details, "info.ZZZZ"), Some("custom note"));
        // Every one of those values is in the search text, because every one of
        // them is marked searchable.
        let text = details.search_text();
        for value in ["Naimio", "Door Slam", "custom note"] {
            assert!(text.contains(value), "{value} missing from {text:?}");
        }
    }

    #[test]
    fn a_details_read_refuses_a_value_it_cannot_search() {
        let mut body = b"INFO".to_vec();
        body.extend_from_slice(b"ICMT");
        let value = b"checksum 0123456789abcdef0123456789abcdef";
        body.extend_from_slice(&(value.len() as u32).to_le_bytes());
        body.extend_from_slice(value);
        let bytes = wav_with_chunk(b"LIST", &body);
        let path = write_temp("details-dump.wav", &bytes);
        let details = read_details_now(&path).unwrap();
        let comment = details
            .tags
            .iter()
            .find(|tag| tag.key == "info.comment")
            .expect("shown for display");
        assert!(!comment.searchable, "a GUID dump must not enter FTS");
        assert!(!details.search_text().contains("checksum"));
    }

    #[test]
    fn the_fingerprint_check_defers_a_changed_file() {
        let bytes = wav_with_chunk(b"iXML", br#"<BWFXML><PROJECT>Rain Pack</PROJECT></BWFXML>"#);
        let path = write_temp("details-fingerprint.wav", &bytes);
        let size = bytes.len() as u64;
        let meta = std::fs::metadata(&path).unwrap();
        let mtime = mtime_ns(&meta);
        let kept = details_only_with_meta_checked(&path, size, mtime).unwrap();
        assert!(kept.is_some(), "the unchanged file is read");
        let deferred = details_only_with_meta_checked(&path, size, mtime + 1).unwrap();
        assert!(deferred.is_none(), "a moved fingerprint is not written");
    }
}
