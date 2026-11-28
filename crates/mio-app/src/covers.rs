//! Cover thumbnails and the inspector's larger picture.
//!
//! Two sizes, two lifetimes:
//!
//! * **48px thumbnails** go into `covers.thumb` once per unique picture hash, and
//!   the table draws them at 24px. Only a hash the library has not stored before
//!   is decoded, so a pack that reuses one album cover across 400 files costs one
//!   decode.
//! * **512px inspector renditions** are lazy, never written to the database, and
//!   kept in a small LRU in [`crate::state::AppState`]. Walking a list with the
//!   arrow keys would otherwise re-decode the same cover on every key press.
//!
//! Decoding is bounded before it starts: a 40000×40000 PNG is 6.4 GB of pixels and
//! a malformed header can claim that, so the image crate is given explicit
//! dimension and allocation limits and any failure is "no picture", never an error
//! the pass has to stop for.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{GenericImageView, ImageEncoder, ImageReader, Limits};

use mio_index::{CoverBytes, EmbeddedDetails};

/// Thumbnail long edge. The table draws 24px; 48px covers a 2× display.
const THUMB_EDGE: u32 = 48;
/// Inspector long edge, matching the panel's designed 1:1 area.
const PREVIEW_EDGE: u32 = 512;
/// Refuse to decode anything whose declared dimensions exceed this.
const MAX_EDGE: u32 = 16_384;
/// Refuse to allocate more than this for one decode.
const MAX_ALLOC: u64 = 256 * 1024 * 1024;

/// Sidecar file stems, in priority order: a pack's `cover.jpg` beats `art.jpg`.
const SIDECAR_STEMS: &[&str] = &["cover", "folder", "front", "album", "artwork", "art"];

/// Image extensions a sidecar may have.
const SIDECAR_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif"];

/// An encoded thumbnail, ready for `covers`.
#[derive(Debug, Clone, PartialEq)]
pub struct Thumb {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

/// Decode whatever picture the file carries (or its sidecar) and shrink it.
///
/// `Ok(None)` means "this file has no usable picture", which is the normal case
/// for most of a sample library and is not a failure.
pub fn thumbnail(cover: &CoverBytes) -> Result<Thumb, String> {
    let (image, has_alpha) = decode(&cover.data)?;
    // A picture smaller than the thumbnail is upscaled by no more than the display
    // needs; `resize` keeps the aspect and never distorts.
    let (w, h) = image.dimensions();
    let scale = (THUMB_EDGE as f32 / w.max(h) as f32).min(1.0);
    let target_w = ((w as f32 * scale).round() as u32).max(1);
    let target_h = ((h as f32 * scale).round() as u32).max(1);
    let resized = image.resize_exact(target_w, target_h, FilterType::Triangle);
    encode(resized, has_alpha)
}

/// The inspector's 512px rendition, as a data URL. `None` when the bytes are not
/// a decodable image.
pub fn preview_data_url(cover: &CoverBytes) -> Option<String> {
    let (image, has_alpha) = decode(&cover.data).ok()?;
    let (w, h) = image.dimensions();
    let scale = (PREVIEW_EDGE as f32 / w.max(h) as f32).min(1.0);
    let target_w = ((w as f32 * scale).round() as u32).max(1);
    let target_h = ((h as f32 * scale).round() as u32).max(1);
    let resized = image.resize_exact(target_w, target_h, FilterType::Lanczos3);
    let thumb = encode(resized, has_alpha).ok()?;
    Some(format!(
        "data:{};base64,{}",
        thumb.mime,
        base64(&thumb.bytes)
    ))
}

fn decode(bytes: &[u8]) -> Result<(image::DynamicImage, bool), String> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(MAX_ALLOC);
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("unreadable picture: {e}"))?;
    reader.limits(limits);
    let image = reader.decode().map_err(|e| format!("undecodable picture: {e}"))?;
    // "Has alpha" for the output format means "some pixel is not opaque", not
    // "the container has an alpha channel": a fully opaque PNG is a JPEG's worth
    // of picture and should not be re-encoded as a PNG. The scan only happens for
    // an image that actually declares an alpha channel.
    let has_alpha = image.color().has_alpha()
        && image
            .to_rgba8()
            .pixels()
            .any(|pixel| pixel[3] != u8::MAX);
    Ok((image, has_alpha))
}

fn encode(image: image::DynamicImage, has_alpha: bool) -> Result<Thumb, String> {
    let mut bytes = Vec::new();
    if has_alpha {
        let rgba = image.to_rgba8();
        PngEncoder::new(&mut bytes)
            .write_image(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
            .map_err(|e| format!("PNG encode failed: {e}"))?;
        Ok(Thumb { bytes, mime: "image/png" })
    } else {
        let rgb = image.to_rgb8();
        // Quality 80 at 48px: measured at ~1.9KB per thumb, and the visible
        // difference from quality 95 at this size is nil.
        JpegEncoder::new_with_quality(&mut bytes, 80)
            .write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
            .map_err(|e| format!("JPEG encode failed: {e}"))?;
        Ok(Thumb { bytes, mime: "image/jpeg" })
    }
}

/// Base64 without pulling in a crate for it.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

/// A sidecar picture found next to (or above) an audio file.
#[derive(Debug, Clone, PartialEq)]
pub struct Sidecar {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub mtime_ns: i64,
}

/// Parse a directory once and answer sidecar questions from the result.
///
/// The details pass walks a library in path order, so a directory's files arrive
/// in a run; caching one directory entry list per worker thread turns 282k
/// `read_dir` calls into one per directory. The cache is keyed by directory and
/// only the most recent one is kept, because the candidates are sorted by path.
#[derive(Debug, Default)]
pub struct SidecarCache {
    dir: Option<PathBuf>,
    /// Candidate sidecar files in the directory, best first.
    files: Vec<PathBuf>,
    /// Directories above this one, nearest first, with their candidate lists.
    ancestors: Vec<(PathBuf, Vec<PathBuf>)>,
    /// Decoded sidecar for this directory, once found.
    resolved: Option<Option<Sidecar>>,
}

impl SidecarCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The sidecar picture `audio` should fall back to, or `None`.
    ///
    /// Rules, in order: a named cover in the same directory, then the directory's
    /// only picture, then the same in each ancestor up to `root`.
    pub fn sidecar_for(&mut self, audio: &Path, root: &Path) -> Option<Sidecar> {
        let dir = audio.parent()?;
        if self.dir.as_deref() != Some(dir) {
            self.load(dir, root);
        }
        if let Some(resolved) = &self.resolved {
            return resolved.clone();
        }
        let mut found = None;
        if let Some(first) = self.files.first() {
            found = read_sidecar(first);
        }
        if found.is_none() {
            for (_, files) in &self.ancestors {
                if let Some(first) = files.first() {
                    found = read_sidecar(first);
                    if found.is_some() {
                        break;
                    }
                }
            }
        }
        self.resolved = Some(found.clone());
        found
    }

    fn load(&mut self, dir: &Path, root: &Path) {
        self.dir = Some(dir.to_path_buf());
        self.resolved = None;
        self.files = candidate_pictures(dir);
        self.ancestors.clear();
        let mut current = dir.parent().map(Path::to_path_buf);
        while let Some(ancestor) = current {
            if !is_inside(&ancestor, root) {
                break;
            }
            let pictures = candidate_pictures(&ancestor);
            if !pictures.is_empty() {
                self.ancestors.push((ancestor.clone(), pictures));
            }
            if ancestor == root {
                break;
            }
            current = ancestor.parent().map(Path::to_path_buf);
        }
    }
}

fn is_inside(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn read_sidecar(path: &Path) -> Option<Sidecar> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > mio_index::details::MAX_COVER_BYTES as u64 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let mtime_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    Some(Sidecar { path: path.to_path_buf(), bytes, mtime_ns })
}

/// Candidate pictures in `dir`, best first.
///
/// A `cover.jpg` outranks `cover.png` only because the list order says so; a
/// directory with several pictures and none of the reserved stems falls back to
/// "the only picture", and a directory with three random pictures is ambiguous and
/// yields nothing. Guessing among them would attach the wrong artwork to a file
/// and the hash would then claim it is the same cover as everything else in the
/// folder.
fn candidate_pictures(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut named: Vec<(usize, PathBuf)> = Vec::new();
    let mut other: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Hidden and temporary files: macOS `._cover.jpg`, Office `~$cover.png`,
        // a half-written download.
        if name.starts_with('.') || name.starts_with("~$") {
            continue;
        }
        let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !SIDECAR_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()) {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match SIDECAR_STEMS.iter().position(|candidate| *candidate == stem) {
            Some(rank) => named.push((rank, path)),
            None => other.push(path),
        }
    }
    named.sort_by_key(|(rank, _)| *rank);
    let mut out: Vec<PathBuf> = named.into_iter().map(|(_, path)| path).collect();
    if out.is_empty() && other.len() == 1 {
        out.push(other.remove(0));
    }
    out
}

/// The embedded picture, or the sidecar one. Embedded always wins; a file that
/// declares its own artwork has already answered the question.
pub fn cover_for(
    details: &EmbeddedDetails,
    cache: &mut SidecarCache,
    audio: &Path,
    root: &Path,
) -> Option<(CoverBytes, Option<Sidecar>)> {
    if let Some(cover) = &details.cover {
        return Some((cover.clone(), None));
    }
    let sidecar = cache.sidecar_for(audio, root)?;
    let media_type = sidecar
        .path
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| match ext.to_ascii_lowercase().as_str() {
            "png" => "image/png".to_string(),
            "webp" => "image/webp".to_string(),
            "bmp" => "image/bmp".to_string(),
            "gif" => "image/gif".to_string(),
            _ => "image/jpeg".to_string(),
        })
        .unwrap_or_else(|| "image/jpeg".to_string());
    Some((
        CoverBytes {
            media_type,
            data: sidecar.bytes.clone(),
        },
        Some(sidecar),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny valid PNG, 2×2, opaque red. Small enough to spell out and stable
    /// across `image` versions because the encoder is the same crate that decodes
    /// it.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let rgba = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 0, 0, 255]));
        PngEncoder::new(&mut bytes)
            .write_image(rgba.as_raw(), width, height, image::ExtendedColorType::Rgba8)
            .unwrap();
        bytes
    }

    #[test]
    fn base64_matches_the_canonical_encoding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_thumbnail_is_a_jpeg_no_larger_than_the_short_edge() {
        let cover = CoverBytes {
            media_type: "image/png".into(),
            data: png(300, 200),
        };
        let thumb = thumbnail(&cover).unwrap();
        assert_eq!(thumb.mime, "image/jpeg");
        assert!(thumb.bytes.starts_with(&[0xff, 0xd8]), "not a JPEG");
        let decoded = image::load_from_memory(&thumb.bytes).unwrap();
        assert_eq!(decoded.width(), THUMB_EDGE);
        assert_eq!(decoded.height(), 32);
    }

    #[test]
    fn an_alpha_picture_keeps_a_png_container() {
        let mut bytes = Vec::new();
        let rgba = image::RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 0]));
        PngEncoder::new(&mut bytes)
            .write_image(rgba.as_raw(), 64, 64, image::ExtendedColorType::Rgba8)
            .unwrap();
        let thumb = thumbnail(&CoverBytes {
            media_type: "image/png".into(),
            data: bytes,
        })
        .unwrap();
        assert_eq!(thumb.mime, "image/png");
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let cover = CoverBytes {
            media_type: "image/jpeg".into(),
            data: b"not a picture at all".to_vec(),
        };
        assert!(thumbnail(&cover).is_err());
        assert!(preview_data_url(&cover).is_none());
    }

    #[test]
    fn a_sidecar_named_cover_beats_a_random_picture() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("IMG_0001.jpg"), png(2, 2)).unwrap();
        std::fs::write(dir.path().join("cover.jpg"), png(2, 2)).unwrap();
        let audio = dir.path().join("a.wav");
        std::fs::write(&audio, b"RIFF").unwrap();
        let mut cache = SidecarCache::new();
        let found = cache.sidecar_for(&audio, dir.path()).unwrap();
        assert!(found.path.ends_with("cover.jpg"));
    }

    #[test]
    fn a_directory_with_several_random_pictures_has_no_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), png(2, 2)).unwrap();
        std::fs::write(dir.path().join("b.jpg"), png(2, 2)).unwrap();
        let audio = dir.path().join("a.wav");
        std::fs::write(&audio, b"RIFF").unwrap();
        let mut cache = SidecarCache::new();
        assert!(cache.sidecar_for(&audio, dir.path()).is_none());
    }

    #[test]
    fn an_ancestor_cover_is_found_from_a_nested_folder() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("Pack/Discs")).unwrap();
        std::fs::write(root.path().join("Pack/folder.png"), png(2, 2)).unwrap();
        let audio = root.path().join("Pack/Discs/a.wav");
        std::fs::write(&audio, b"RIFF").unwrap();
        let mut cache = SidecarCache::new();
        let found = cache.sidecar_for(&audio, root.path()).unwrap();
        assert!(found.path.ends_with("folder.png"));
    }

    #[test]
    fn the_preview_is_a_base64_data_url() {
        let cover = CoverBytes {
            media_type: "image/png".into(),
            data: png(600, 400),
        };
        let url = preview_data_url(&cover).unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,"));
        assert!(!url.contains(' '));
    }
}
