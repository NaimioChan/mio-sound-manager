//! Embedded tags and cover identity for the details pass.
//!
//! Two jobs, both deliberately kept out of the basic index:
//!
//! * **Raw tag values**, as a list of `{key, value, source, searchable}` for the
//!   inspector, plus one concatenated search text for FTS.
//! * **Cover bytes and their identity**, so two files carrying the same picture
//!   compare equal without ever being decoded twice.
//!
//! Nothing here decodes audio and nothing here decodes an image. The cover is
//! hashed as bytes; thumbnails are the app's job (`crates/mio-app`), because the
//! index crate has no business depending on an image codec.
//!
//! **`searchable` is a promise.** A value marked searchable must appear in the
//! concatenated text the FTS column stores; a value this module refused to clean
//! is marked unsearchable and the inspector shows it as plain text. A tag the UI
//! invites a click on but the search index never contains would be a filter that
//! silently returns nothing.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use symphonia::core::meta::{
    MetadataOptions, StandardTagKey, StandardVisualKey, Tag, Value, Visual,
};

/// One tag value as stored in `sound_details.tags_json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataTag {
    /// Canonical `snake_case` for a recognised tag (`artist`, `bext.originator`,
    /// `ixml.project`); the raw four-character code or frame id otherwise.
    pub key: String,
    pub value: String,
    /// Where it came from: `id3`, `vorbis`, `m4a`, `bext`, `ixml`, `info`.
    pub source: String,
    /// Whether `sounds.tags` (and therefore FTS) contains this exact value.
    pub searchable: bool,
}

/// The picture an audio file carries, before any decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverBytes {
    /// The container's declared MIME type. Used as a hint, never trusted: the
    /// thumbnail encoder sniffs the actual bytes.
    pub media_type: String,
    pub data: Vec<u8>,
}

impl CoverBytes {
    /// 32 lowercase hex digits: `sha256(b"mio-cover\0" + image bytes)`, first 16
    /// bytes. A domain prefix so this can never collide with a hash of the file
    /// itself, and a fixed prefix so two different builds agree.
    pub fn hash(&self) -> String {
        cover_hash(&self.data)
    }
}

/// Hash of the picture bytes, not of the tag container: the same JPEG embedded in
/// an MP3 and a FLAC matches, a re-encoded version does not (accepted).
pub fn cover_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"mio-cover\0");
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// What one file's details pass produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EmbeddedDetails {
    pub tags: Vec<MetadataTag>,
    pub cover: Option<CoverBytes>,
    /// Some value or the whole JSON was cut at the documented size limits.
    pub truncated: bool,
    /// Whether `cover` was declared as a front cover. Ambiguous album art behind
    /// a declared front cover must not replace it.
    cover_front: bool,
}

impl EmbeddedDetails {
    /// The text `sounds.tags` stores and FTS tokenizes.
    ///
    /// Only values marked searchable are included, so the promise above holds by
    /// construction: there is one list and one derivation from it.
    pub fn search_text(&self) -> String {
        let mut text = String::new();
        for tag in &self.tags {
            if !tag.searchable {
                continue;
            }
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&tag.value);
            if text.len() >= MAX_SEARCH_TEXT_BYTES {
                break;
            }
        }
        truncate_utf8(&text, MAX_SEARCH_TEXT_BYTES)
    }
}

/// Upper bound on one tag value. Longer values are kept for display but never
/// enter the search text.
pub const MAX_VALUE_BYTES: usize = 4 * 1024;
/// Upper bound on the serialized tag JSON (the DTO is read per row, not per page,
/// but a pathological file must not produce a megabyte of inspector payload).
pub const MAX_JSON_BYTES: usize = 32 * 1024;
/// Upper bound on the FTS text in `sounds.tags`.
pub const MAX_SEARCH_TEXT_BYTES: usize = 8 * 1024;
/// Upper bound on one embedded picture. Album art over this is not art.
pub const MAX_COVER_BYTES: usize = 32 * 1024 * 1024;

fn truncate_utf8(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let cut = (0..=max)
        .rev()
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(0);
    text[..cut].to_string()
}

/// Tags in storage order, with the documented limits applied.
///
/// Duplicate `(key, value)` pairs are dropped: a WAV can carry the same comment
/// in `bext`, `LIST` and `ID3`, and three identical rows in the inspector is
/// noise, not information.
pub fn finalize(mut details: EmbeddedDetails) -> EmbeddedDetails {
    let mut seen = std::collections::HashSet::new();
    let mut kept = Vec::new();
    let mut truncated = details.truncated;
    let mut json_used = 2usize; // `[` and `]`
    for mut tag in details.tags.drain(..) {
        if tag.value.len() > MAX_VALUE_BYTES {
            tag.value = truncate_utf8(&tag.value, MAX_VALUE_BYTES);
            tag.searchable = false;
            truncated = true;
        }
        if tag.value.trim().is_empty() {
            continue;
        }
        let identity = (tag.key.clone(), tag.value.clone());
        if !seen.insert(identity) {
            continue;
        }
        // Quotes, field names, the source and the boolean: a little over the
        // value's own length plus a fixed 60 bytes. The exact number does not
        // matter; the bound does.
        let cost = tag.value.len() + tag.key.len() + tag.source.len() + 60;
        if json_used + cost > MAX_JSON_BYTES {
            truncated = true;
            break;
        }
        json_used += cost;
        kept.push(tag);
    }
    EmbeddedDetails {
        tags: kept,
        cover: details.cover,
        truncated,
        cover_front: details.cover_front,
    }
}

/// Canonical key for a standard tag, or `None` for one we do not name.
///
/// The list is what a sample library actually writes. Everything else keeps its
/// raw frame id: inventing a name for `TXXX:foo` would make the inspector claim a
/// meaning the file never gave.
pub fn canonical_tag_key(key: StandardTagKey) -> Option<&'static str> {
    Some(match key {
        StandardTagKey::Album => "album",
        StandardTagKey::AlbumArtist => "album_artist",
        StandardTagKey::Artist => "artist",
        StandardTagKey::Bpm => "bpm",
        StandardTagKey::Comment => "comment",
        StandardTagKey::Composer => "composer",
        StandardTagKey::Copyright => "copyright",
        StandardTagKey::Date => "date",
        StandardTagKey::Description => "description",
        StandardTagKey::DiscNumber => "disc",
        StandardTagKey::EncodedBy => "encoded_by",
        StandardTagKey::Encoder => "encoder",
        StandardTagKey::Genre => "genre",
        StandardTagKey::IdentCatalogNumber => "catalog_number",
        StandardTagKey::IdentIsrc => "isrc",
        StandardTagKey::Label => "label",
        StandardTagKey::Language => "language",
        StandardTagKey::License => "license",
        StandardTagKey::Lyrics => "lyrics",
        StandardTagKey::Mood => "mood",
        StandardTagKey::OriginalArtist => "original_artist",
        StandardTagKey::Producer => "producer",
        StandardTagKey::Rating => "rating",
        StandardTagKey::TrackNumber => "track",
        StandardTagKey::TrackTitle => "title",
        StandardTagKey::TrackTotal => "track_total",
        StandardTagKey::Version => "version",
        StandardTagKey::Writer => "writer",
        _ => return None,
    })
}

/// Tags and cover from one symphonia metadata revision.
pub fn from_revision(
    revision: &symphonia::core::meta::MetadataRevision,
    source: &str,
    out: &mut EmbeddedDetails,
) {
    for tag in revision.tags() {
        push_tag(
            out,
            source,
            tag.std_key.and_then(canonical_tag_key),
            &tag.key,
            tag,
        );
    }
    for visual in revision.visuals() {
        consider_visual(out, visual);
    }
}

/// A tag from an ID3 or Vorbis revision. `Value` variants other than text and
/// small numbers are binary payloads (embedded pictures, vendor blobs) and are not
/// tags.
fn push_tag(
    out: &mut EmbeddedDetails,
    source: &str,
    canonical: Option<&'static str>,
    raw_key: &str,
    tag: &Tag,
) {
    let text = match &tag.value {
        Value::String(value) => value.clone(),
        Value::UnsignedInt(value) => value.to_string(),
        Value::SignedInt(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        // Binary, Flag: the file carries no displayable text here.
        _ => return,
    };
    let Some((value, searchable)) = clean_value(&text) else {
        return;
    };
    let key = match canonical {
        Some(key) => key.to_string(),
        None => {
            let raw = raw_key.trim();
            if raw.is_empty() {
                return;
            }
            raw.to_string()
        }
    };
    out.tags.push(MetadataTag {
        key,
        value,
        source: source.to_string(),
        searchable,
    });
}

/// Push a tag read directly out of a container field (bext, iXML, LIST).
pub fn push_field(out: &mut EmbeddedDetails, source: &str, key: &str, value: &str) {
    let Some((value, searchable)) = clean_value(value) else {
        return;
    };
    out.tags.push(MetadataTag {
        key: key.to_string(),
        value,
        source: source.to_string(),
        searchable,
    });
}

/// Trim a raw string into a displayable value, and decide whether it may enter
/// the search text.
///
/// The same rules as the description cleaner, because the reason is the same:
/// sample packs smuggle GUIDs, encoder dumps and binary junk through text fields.
/// `None` means "not a value at all" (empty, or almost entirely non-printable).
pub fn clean_value(raw: &str) -> Option<(String, bool)> {
    let text = raw.trim_matches('\0').trim();
    if text.is_empty() {
        return None;
    }
    // Newlines and tabs are whitespace, not corruption: a coding history is
    // multi-line prose, and counting its line breaks as unprintable threw away
    // every one of them.
    let text = text
        .chars()
        .map(|c| {
            if c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else if c.is_control() {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect::<String>();
    if printable_ratio(&text) < 0.95 {
        return None;
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    if text.len() > MAX_VALUE_BYTES {
        // Kept for display (the inspector truncates in CSS), refused for search:
        // an 8KB "comment" is a dump, and FTS would tokenize all of it.
        return Some((truncate_utf8(&text, MAX_VALUE_BYTES), false));
    }
    let searchable = !looks_technical(&text);
    Some((text, searchable))
}

/// A value that is almost certainly a dump rather than a name.
fn looks_technical(text: &str) -> bool {
    // A GUID, an MD5, a base64 blob: long runs of hex or hex-ish digits.
    let long_hex = text.split(|c: char| !c.is_ascii_hexdigit()).any(|token| {
        token.len() >= 24
            && token.chars().any(|c| c.is_ascii_digit())
            && token.chars().any(|c| c.is_ascii_alphabetic())
    });
    if long_hex {
        return true;
    }
    // More than a third of the characters are digits, and there are at least a
    // dozen: sample rates, frame counts, timecode dumps. A tempo or a date is
    // short enough not to trip this.
    let digits = text.chars().filter(|c| c.is_ascii_digit()).count();
    let alnum = text.chars().filter(|c| c.is_alphanumeric()).count();
    digits >= 12 && digits * 3 > alnum
}

fn printable_ratio(text: &str) -> f64 {
    let total = text.chars().count();
    if total == 0 {
        return 0.0;
    }
    text.chars()
        .filter(|c| *c != '\u{fffd}' && !c.is_control())
        .count() as f64
        / total as f64
}

/// Choose the cover: the front cover if one is declared, else the first usable
/// picture. URL pictures (`-->` media type) and empty payloads are skipped.
fn consider_visual(out: &mut EmbeddedDetails, visual: &Visual) {
    if visual.data.is_empty() || visual.data.len() > MAX_COVER_BYTES {
        return;
    }
    let media_type = visual.media_type.trim();
    if media_type == "-->" || media_type.is_empty() {
        return;
    }
    let is_front = visual.usage == Some(StandardVisualKey::FrontCover);
    let already_front = out.cover_front;
    // The first usable picture is the fallback; a declared front cover replaces it
    // even when it comes later, and a second front cover does not.
    if out.cover.is_some() && (already_front || !is_front) {
        return;
    }
    out.cover = Some(CoverBytes {
        media_type: media_type.to_string(),
        data: visual.data.to_vec(),
    });
    out.cover_front = is_front;
}

/// Metadata options that let a real cover through while refusing an unbounded
/// allocation from a malformed file.
pub fn metadata_options() -> MetadataOptions {
    MetadataOptions {
        limit_metadata_bytes: symphonia::core::meta::Limit::Maximum(1024 * 1024),
        limit_visual_bytes: symphonia::core::meta::Limit::Maximum(MAX_COVER_BYTES),
    }
}

/// Serialize tags for `sound_details.tags_json`. Falls back to `[]` rather than
/// failing a pass over a value serde cannot represent (it cannot happen with
/// strings, and a pass must not stop for a theoretical one).
pub fn tags_json(details: &EmbeddedDetails) -> String {
    serde_json::to_string(&details.tags).unwrap_or_else(|_| "[]".into())
}

/// Common WAV `LIST/INFO` codes worth showing. Anything else keeps its code.
pub fn info_key(code: &[u8]) -> Option<&'static str> {
    Some(match code {
        b"IART" => "info.artist",
        b"ICMT" => "info.comment",
        b"ICOP" => "info.copyright",
        b"ICRD" => "info.date",
        b"IGNR" => "info.genre",
        b"INAM" => "info.title",
        b"IPRD" => "info.album",
        b"ISFT" => "info.software",
        b"ISRF" => "info.source",
        b"ITCH" => "info.technician",
        b"ICMS" => "info.commissioner",
        b"IENG" => "info.engineer",
        b"ISBJ" => "info.subject",
        b"IMED" => "info.medium",
        _ => return None,
    })
}

/// iXML fields worth keeping. iXML is a free-form tree; this is the common text
/// vocabulary, not a schema.
pub fn ixml_key(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_uppercase().as_str() {
        "PROJECT" => "ixml.project",
        "SCENE" => "ixml.scene",
        "TAKE" => "ixml.take",
        "TAPE" => "ixml.tape",
        "NOTE" => "ixml.note",
        "CIRCLED" => "ixml.circled",
        "CATEGORY" => "ixml.category",
        "SUBCATEGORY" => "ixml.subcategory",
        "SPEED" => "ixml.speed",
        "TRACK" => "ixml.track",
        "ROLL" => "ixml.roll",
        "FRAME_RATE" => "ixml.frame_rate",
        "TIMECODE_RATE" => "ixml.timecode_rate",
        "BWF_ORIGINATION_DATE" => "ixml.origination_date",
        "BWF_TIME_REFERENCE" => "ixml.time_reference",
        "BWF_ORIGINATION_TIME" => "ixml.origination_time",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn details_with(tags: Vec<MetadataTag>) -> EmbeddedDetails {
        EmbeddedDetails {
            tags,
            cover: None,
            truncated: false,
            cover_front: false,
        }
    }

    fn tag(key: &str, value: &str, searchable: bool) -> MetadataTag {
        MetadataTag {
            key: key.into(),
            value: value.into(),
            source: "id3".into(),
            searchable,
        }
    }

    #[test]
    fn a_cover_hash_is_stable_and_domain_separated() {
        let hash = cover_hash(b"jpeg bytes");
        assert_eq!(hash.len(), 32);
        assert!(
            hash.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_eq!(hash, cover_hash(b"jpeg bytes"));
        assert_ne!(hash, cover_hash(b"jpeg byteS"));
        // The domain prefix means hashing the bytes with no prefix is a different
        // value: a future consumer must not be able to confuse the two.
        let plain = Sha256::digest(b"jpeg bytes");
        let plain_hex: String = plain[..16].iter().map(|b| format!("{b:02x}")).collect();
        assert_ne!(hash, plain_hex);
    }

    #[test]
    fn search_text_contains_exactly_the_searchable_values() {
        let details = details_with(vec![
            tag("artist", "Naimio", true),
            tag("comment", "a GUID 0123456789abcdef0123456789abcdef", false),
            tag("title", "Door", true),
        ]);
        assert_eq!(details.search_text(), "Naimio Door");
    }

    #[test]
    fn a_long_hex_dump_is_kept_for_display_but_refused_for_search() {
        let raw = "0123456789abcdef0123456789abcdef";
        let (value, searchable) = clean_value(raw).expect("displayable");
        assert_eq!(value, raw);
        assert!(!searchable);

        let (name, searchable) = clean_value("  Door   Slam  ").expect("displayable");
        assert_eq!(name, "Door Slam");
        assert!(searchable);
    }

    #[test]
    fn a_value_longer_than_the_limit_is_truncated_and_unsearchable() {
        let long = "a".repeat(MAX_VALUE_BYTES + 10);
        let (value, searchable) = clean_value(&long).unwrap();
        assert_eq!(value.len(), MAX_VALUE_BYTES);
        assert!(!searchable);
    }

    #[test]
    fn finalize_drops_duplicates_and_reports_truncation() {
        let mut details = details_with(vec![
            tag("comment", "same", true),
            tag("comment", "same", true),
            tag("title", "other", true),
        ]);
        let out = finalize(std::mem::take(&mut details));
        assert_eq!(out.tags.len(), 2);
        assert!(!out.truncated);
    }

    #[test]
    fn finalize_stops_at_the_json_budget() {
        let big = "b".repeat(MAX_VALUE_BYTES);
        let tags = (0..20)
            .map(|i| tag(&format!("k{i}"), &big, true))
            .collect();
        let out = finalize(details_with(tags));
        assert!(out.truncated);
        assert!(out.tags.len() < 20);
    }

    #[test]
    fn json_round_trips_as_camel_case() {
        let json = tags_json(&details_with(vec![tag("artist", "x", true)]));
        assert!(json.contains("\"searchable\":true"));
        assert!(json.contains("\"key\":\"artist\""));
    }

    #[test]
    fn canonical_keys_cover_the_common_id3_frames() {
        assert_eq!(canonical_tag_key(StandardTagKey::Artist), Some("artist"));
        assert_eq!(
            canonical_tag_key(StandardTagKey::TrackTitle),
            Some("title")
        );
        assert_eq!(canonical_tag_key(StandardTagKey::Mood), Some("mood"));
        assert_eq!(canonical_tag_key(StandardTagKey::AcoustidId), None);
    }
}
