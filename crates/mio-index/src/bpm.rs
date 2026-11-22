//! Tempo from the names a library already has.
//!
//! Sample packs state the tempo somewhere in the path — `KSHMR_Kick_128BPM_Cmin.wav`,
//! `Drum Loop 174 bpm.wav`, `.../Kit Angels - Gm 126BPM/Arp_C.wav`, `Vocal 01 (90, Em).wav`
//! — and a browser that can sort by it saves opening the same loop in a DAW. Nothing
//! here reads audio: a WAV header carries no tempo, and estimating one from the
//! waveform would mean decoding every file during the index pass.
//!
//! Four tiers, first match wins:
//!
//! 1. An explicit `bpm` marker in the file name — `128BPM`, `174 bpm`, `BPM120`, and
//!    the `126-5 BPM` / `BPM_138.4` spellings of a fractional tempo.
//! 2. The Splice convention `(90, Em)`: a tempo and a key in parentheses.
//! 3. The same markers in an ancestor folder name, nearest first. A folder that names
//!    a *range* (`83-98 BPM`) is skipped: it says the pack spans that range, not that
//!    this file sits at one end of it.
//! 4. A bare number in a loop-ish file name, when the file's length agrees with it.
//!
//! Tier 4 is the only one that guesses from a bare number, so both of its conditions
//! carry weight. **The name has to say "loop"**: in `Snare_120.wav`, `Kick_808.wav`
//! and `NIG065.wav` the number is an index or a pack id, and the same bar-length test
//! that validates loop names happily accepts `NIG065.wav` as 65 BPM over 33 bars.
//! **The length has to be a whole number of bars**: a trimmed loop ends on a bar line
//! to within 25 ms, and on 21,812 rows whose tempo is known from an explicit marker a
//! real tempo clears that bar 90% of the time while a random number clears it 3%.
//!
//! The bar test is a test about loops, so it only ever vetoes a loop: tier 4 requires
//! a loop name to begin with, and tier 3 runs it only when the name says loop. A
//! one-shot is a note plus a tail and its length counts nothing musical, so the
//! folder's tempo stands however long it is. Gating tier 3 on every row emptied 174
//! rows in a 271k library and was wrong 74% of the time against the file's own
//! marker — long vocal phrases were the bulk of it.
//!
//! A wrong number that sorts confidently is worse than an empty cell, so a tier that
//! fails its check yields `None` rather than a weaker guess.

/// Longest and shortest tempo accepted, in BPM.
const MIN_BPM: f64 = 20.0;
const MAX_BPM: f64 = 300.0;

/// Window a bare number in a loop name may fall in. Narrower than the marker range
/// on purpose: a two-digit number at or below 59 in a loop name is a bar count far
/// more often than a tempo (one pack ships `Loop 32` for a 32-bar file), and the
/// durations that would corroborate a sub-60 tempo are half-time spellings of the
/// same loop.
const BARE_MIN: u32 = 60;
const BARE_MAX: u32 = 200;

/// How far a file's length may sit from a whole bar and still corroborate a tempo.
/// A bar at 128 BPM is 1.875 s; 25 ms is a little over one sample of jitter per bar
/// at 44.1 kHz, and wide enough for the rounding WAV headers carry.
const BAR_TOLERANCE_S: f64 = 0.025;

/// Separators allowed between a number and `bpm`, and around a tempo in `(90, Em)`.
fn is_sep(c: char) -> bool {
    matches!(
        c,
        ' ' | '_' | '-' | '.' | '(' | ')' | '[' | ']' | '~' | '#' | '&' | '+' | ',' | '\'' | '"' | '@'
            | '=' | ':'
            | ';'
            | '!'
            | '{'
            | '}'
    )
}

/// Read a run of ASCII digits ending just before `end`, returning the value and the
/// index where the run starts.
fn digits_before(chars: &[char], end: usize) -> Option<(u32, usize)> {
    let mut start = end;
    while start > 0 && chars[start - 1].is_ascii_digit() {
        start -= 1;
    }
    if start == end {
        return None;
    }
    let text: String = chars[start..end].iter().collect();
    text.parse().ok().map(|v| (v, start))
}

/// Read a run of ASCII digits starting at `start`.
fn digits_after(chars: &[char], start: usize) -> Option<(u32, usize)> {
    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_digit() {
        end += 1;
    }
    if start == end {
        return None;
    }
    let text: String = chars[start..end].iter().collect();
    text.parse().ok().map(|v| (v, end))
}

fn accept(value: u32) -> Option<f64> {
    let bpm = f64::from(value);
    (MIN_BPM..=MAX_BPM).contains(&bpm).then_some(bpm)
}

/// Whether the number starting at `start` is the right-hand end of a dashed range,
/// so `100-109 BPM`, `090 - 119Bpm` and `020 - 136 BPM` are all recognised. Only
/// spaces and underscores are skipped while looking back for the dash, because a
/// dash next to a whole number is otherwise the `140-BPM` separator.
fn ends_a_range(chars: &[char], start: usize) -> bool {
    let mut i = start;
    while i > 0 && matches!(chars[i - 1], ' ' | '_') {
        i -= 1;
    }
    if i == 0 || !matches!(chars[i - 1], '-' | '~' | '\u{2013}') {
        return false;
    }
    // Step over the dash, then any padding before the range's left end.
    let mut j = i - 1;
    while j > 0 && matches!(chars[j - 1], ' ' | '_') {
        j -= 1;
    }
    j > 0 && chars[j - 1].is_ascii_digit()
}

/// The tempo written just left of a `bpm` marker, whether the number is glued to a
/// word (`SubBass150 BPM`), and where the number starts.
///
/// A glued number is weaker evidence than a free-standing one: `Vocal24_BPM_138.4`
/// and `DubstepRiddim_Bass_Loop_140bpm_23_G#` both put a take number next to the
/// marker. The caller prefers the other side of the marker when this one is glued.
fn value_before(chars: &[char], marker_at: usize) -> Option<(f64, bool, usize)> {
    let mut end = marker_at;
    while end > 0 && is_sep(chars[end - 1]) {
        end -= 1;
    }
    let (value, start) = digits_before(chars, end)?;

    // `126-5 BPM` is one pack's way of writing a half tempo: a single digit after a
    // dash or a dot is a decimal place of the number in front of it. A tempo *range*
    // writes both ends in full (`120-128 BPM`), so its run is two or three digits and
    // this branch does not touch it.
    if end - start == 1 && start >= 2 && matches!(chars[start - 1], '-' | '.') {
        if let Some((whole, whole_start)) = digits_before(chars, start - 1)
            && (2..=3).contains(&(start - 1 - whole_start))
            && let Some(bpm) = accept(whole)
        {
            return Some((bpm + f64::from(value) / 10.0, false, whole_start));
        }
    }

    if !(2..=3).contains(&(end - start)) || (start > 0 && chars[start - 1].is_ascii_digit()) {
        return None;
    }
    let glued = start > 0 && chars[start - 1].is_ascii_alphabetic();
    accept(value).map(|bpm| (bpm, glued, start))
}

/// The tempo written just right of a `bpm` marker, plus a one-digit decimal tail so
/// `BPM_138.4` reads as 138.4.
fn value_after(chars: &[char], marker_end: usize) -> Option<f64> {
    let mut start = marker_end;
    while start < chars.len() && is_sep(chars[start]) {
        start += 1;
    }
    let (value, end) = digits_after(chars, start)?;
    if !(2..=3).contains(&(end - start)) {
        return None;
    }
    if chars.get(end).is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    let bpm = accept(value)?;
    if let (Some(&sep), Some(&digit)) = (chars.get(end), chars.get(end + 1))
        && matches!(sep, '.' | ',')
        && digit.is_ascii_digit()
        && !chars.get(end + 2).is_some_and(|c| c.is_ascii_digit())
    {
        return Some(bpm + f64::from(digit.to_digit(10).unwrap_or(0)) / 10.0);
    }
    Some(bpm)
}

/// The first tempo marker in `text`, and whether that marker states a range.
fn marker(text: &str) -> Option<(f64, bool)> {
    let lower = text.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();

    let mut i = 0;
    while i + 3 <= chars.len() {
        if chars[i] == 'b' && chars[i + 1] == 'p' && chars[i + 2] == 'm' {
            // A letter glued to `bpm` is a word, not a marker (`bpms`, `rebelbpm`).
            let glued_before = i > 0 && chars[i - 1].is_ascii_alphabetic();
            let glued_after = chars.get(i + 3).is_some_and(|c| c.is_ascii_alphabetic());
            if !glued_before && !glued_after {
                let before = value_before(&chars, i);
                let after = value_after(&chars, i + 3);
                if let Some((bpm, glued, start)) = before {
                    // A free-standing number on the left is the reading with the
                    // fewest assumptions, so it wins even when one follows too:
                    // `DubstepRiddim_Bass_Loop_140bpm_23_G#` is 140, not 23. A glued
                    // number is a take number as often as a tempo, so the other side
                    // of the marker is read first when it has a number of its own.
                    if !glued {
                        return Some((bpm, ends_a_range(&chars, start)));
                    }
                    return Some((after.unwrap_or(bpm), false));
                }
                if let Some(bpm) = after {
                    return Some((bpm, false));
                }
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    None
}

/// Tempo stated by an explicit `bpm` marker in a file name.
pub fn from_filename(name: &str) -> Option<f64> {
    marker(name).map(|(bpm, _)| bpm)
}

/// Note names and the mode words packs write beside a tempo. `phyrgian` is included
/// because one pack spells it that way; a word outside this set ends the key, which
/// is what keeps `(69, Atonal)`, `(100, Gtr)` and `(69, C to Fm)` from being read as
/// a key and `69` from being taken on faith.
const KEY_WORDS: &[&str] = &[
    "m",
    "min",
    "minor",
    "maj",
    "major",
    "phrygian",
    "phyrgian",
    "arabic",
    "dorian",
    "lydian",
    "mixolydian",
    "aeolian",
    "locrian",
];

/// End index of the key token starting at `start`, or `None` when what follows is not
/// a key. Accepts `Em`, `F#m`, `D`, `C Maj`, `E Phrygian`, `F Phyrgian`.
fn key_len(chars: &[char], start: usize) -> Option<usize> {
    let first = *chars.get(start)?;
    if !matches!(first, 'a'..='g') {
        return None;
    }
    let mut i = start + 1;
    // A sharp or flat sits flush against the note, and the quality may sit flush
    // against that: `F#m`, `Bbm`, `F#`. A word with no separator in front of it is
    // only a key word here, where the note has already been read.
    if chars.get(i).is_some_and(|c| matches!(c, '#' | 'b')) {
        i += 1;
    }
    let run_start = i;
    while i < chars.len() && chars[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i > run_start {
        let word: String = chars[run_start..i].iter().collect();
        if !KEY_WORDS.contains(&word.as_str()) {
            return None;
        }
        return Some(i);
    }
    // Up to two more words, each introduced by a separator: `C Maj`, `E Phrygian`,
    // `C Maj Phrygian` never occurs but costs nothing. A bare `D` in `(87, D)` has no
    // separator worth reading, so the key ends at the note instead of swallowing
    // whatever follows it.
    let mut words = 0;
    while words < 2 && i < chars.len() && matches!(chars[i], ' ' | '_' | '-') {
        let mut j = i;
        while j < chars.len() && matches!(chars[j], ' ' | '_' | '-') {
            j += 1;
        }
        let run_start = j;
        while j < chars.len() && chars[j].is_ascii_alphabetic() {
            j += 1;
        }
        if j == run_start {
            break;
        }
        let word: String = chars[run_start..j].iter().collect();
        if !KEY_WORDS.contains(&word.as_str()) {
            return None;
        }
        i = j;
        words += 1;
    }
    Some(i)
}

/// Tempo and key written as `(90, Em)` or `(128, F#m)`.
fn splice(text: &str) -> Option<f64> {
    let lower = text.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '(' {
            let mut j = i + 1;
            while j < chars.len() && matches!(chars[j], ' ' | '_') {
                j += 1;
            }
            if let Some((value, after)) = digits_after(&chars, j) {
                if (2..=3).contains(&(after - j)) {
                    let mut k = after;
                    while k < chars.len() && matches!(chars[k], ' ' | '_') {
                        k += 1;
                    }
                    if chars.get(k) == Some(&',') {
                        k += 1;
                        while k < chars.len() && matches!(chars[k], ' ' | '_') {
                            k += 1;
                        }
                        if let Some(key_end) = key_len(&chars, k) {
                            let mut close = key_end;
                            while close < chars.len() && matches!(chars[close], ' ' | '_') {
                                close += 1;
                            }
                            if chars.get(close) == Some(&')') {
                                if let Some(bpm) = accept(value) {
                                    return Some(bpm);
                                }
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Words that make a name a loop. Compounds (`drumloop`, `houseloops`, `shakerbeats`)
/// are matched by suffix, which is how packs actually build the noun.
const LOOP_WORDS: &[&str] = &[
    "loop",
    "loops",
    "groove",
    "grooves",
    "break",
    "breaks",
    "breakbeat",
    "breakbeats",
    "topline",
    "toplines",
    "melody",
    "melodies",
    "riff",
    "riffs",
    "arp",
    "arps",
    "progression",
    "progressions",
    "chord",
    "chords",
    "bassline",
    "basslines",
    "stem",
    "stems",
];

const LOOP_HEADS: &[&str] = &["loop", "loops", "beat", "beats", "groove", "grooves", "break", "breaks"];

/// Whether `text` names a loop. Only loop-ish names are allowed to state a tempo as a
/// bare number; see the module note.
fn says_loop(text: &str) -> bool {
    let lower = text.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_alphabetic() {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        if LOOP_WORDS.contains(&word.as_str()) {
            return true;
        }
        // `drumloop`, `houseloops`, `shakerbeats`: a bare `beat` is a word too, so
        // only compounds longer than the head count here.
        if word.len() > 4 && LOOP_HEADS.iter().any(|head| word.ends_with(head)) {
            return true;
        }
    }
    false
}

/// Last segment of a relative folder, which is the one a pack names its tempo in.
fn last_folder(folder: &str) -> &str {
    folder.rsplit(['\\', '/']).next().unwrap_or("")
}

/// Tempo named by an ancestor folder, nearest first, skipping segments that state a
/// range.
fn folder_tempo(folder: &str) -> Option<f64> {
    for segment in folder.rsplit(['\\', '/']) {
        if let Some((bpm, is_range)) = marker(segment) {
            // `83-98 BPM` names the span the pack covers. Taking one end would tag
            // every file in it with a tempo the folder never claims.
            if !is_range {
                return Some(bpm);
            }
            continue;
        }
        if let Some(bpm) = splice(segment) {
            return Some(bpm);
        }
    }
    None
}

/// The only number in `filename` inside the bare window, or `None` when the name
/// states none or several. Two *different* candidates mean the name is not saying
/// which is the tempo (`Vocal 01 120 140`), and picking one would be a guess; the same
/// number stated twice (`DS_DSP_150_synth_loop_pad_rage_150_Dm`) is one candidate.
fn unique_bare(filename: &str) -> Option<f64> {
    let chars: Vec<char> = filename.chars().collect();
    let mut found: Option<f64> = None;
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let (value, end) = digits_after(&chars, i)?;
        if (BARE_MIN..=BARE_MAX).contains(&value) {
            let candidate = f64::from(value);
            if found.is_some_and(|seen| (seen - candidate).abs() > f64::EPSILON) {
                return None;
            }
            found = Some(candidate);
        }
        i = end;
    }
    found
}

/// Whether the file is a whole number of bars long at `bpm`.
///
/// A loop is cut on a bar line, so its duration is a multiple of `240 / bpm`. Files
/// shorter than one bar say nothing either way and are left to the caller's policy.
fn whole_bars(duration_s: Option<f64>, bpm: f64) -> Option<bool> {
    let duration = duration_s.filter(|d| d.is_finite() && *d > 0.0)?;
    if bpm <= 0.0 {
        return None;
    }
    let bar = 240.0 / bpm;
    let bars = (duration / bar).round();
    if bars < 1.0 {
        return None;
    }
    Some((duration - bars * bar).abs() <= BAR_TOLERANCE_S)
}

/// Tempo for one indexed file, from its name, its ancestor folders and its length.
///
/// `folder` is the path relative to the library root, as the `folder` column stores
/// it. `duration_s` is only ever used to confirm a guess, never to produce one.
pub fn tempo(filename: &str, folder: &str, duration_s: Option<f64>) -> Option<f64> {
    if let Some(bpm) = from_filename(filename) {
        return Some(bpm);
    }
    if let Some(bpm) = splice(filename) {
        return Some(bpm);
    }
    let loop_named = says_loop(filename) || says_loop(last_folder(folder));
    if let Some(bpm) = folder_tempo(folder) {
        // The folder is shared by everything under it, so its tempo is the least
        // specific evidence here; a length that contradicts it means the folder is
        // describing a parent pack or a sibling kit.
        //
        // That check only applies to loops. A loop is cut on a bar line, so its
        // length is evidence; a one-shot is a hit plus a tail, and `Bars` counts
        // nothing musical about it. Applying the test there emptied 174 rows in a
        // 271k library and was wrong 74% of the time by the file's own marker,
        // because a 42 s vocal phrase simply does not end on a bar line at 100 BPM.
        let contradicted = loop_named && whole_bars(duration_s, bpm) == Some(false);
        if !contradicted {
            return Some(bpm);
        }
    }
    let bare = unique_bare(filename)?;
    if !loop_named {
        return None;
    }
    (whole_bars(duration_s, bare) == Some(true)).then_some(bare)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> Option<f64> {
        tempo(name, "", None)
    }

    #[test]
    fn a_marker_glued_to_the_number_parses() {
        assert_eq!(from_filename("KSHMR_Kick_128BPM_Cmin.wav"), Some(128.0));
        assert_eq!(from_filename("loop_90bpm.wav"), Some(90.0));
    }

    #[test]
    fn a_marker_separated_from_the_number_parses() {
        assert_eq!(from_filename("Drum Loop 174 bpm.wav"), Some(174.0));
        assert_eq!(from_filename("Bass 140-BPM.wav"), Some(140.0));
        assert_eq!(from_filename("vox_120_bpm_take2.wav"), Some(120.0));
    }

    #[test]
    fn the_marker_may_come_first() {
        assert_eq!(from_filename("BPM120_vox.wav"), Some(120.0));
        assert_eq!(from_filename("bpm 100 pad.wav"), Some(100.0));
    }

    #[test]
    fn a_name_without_a_marker_and_without_a_loop_stays_unmarked() {
        assert_eq!(named("Snare_120.wav"), None);
        assert_eq!(named("door_slam_02.wav"), None);
        assert_eq!(named("Kick_808.wav"), None);
        assert_eq!(named("hihat_44k_24bit.wav"), None);
        // A pack id with a plausible number: 65 BPM over 33 bars is arithmetically
        // tidy, which is exactly why the loop word has to gate this tier.
        assert_eq!(tempo("NIG065.wav", "", Some(122.307)), None);
    }

    #[test]
    fn a_number_outside_the_plausible_range_is_not_a_tempo() {
        assert_eq!(from_filename("bpm_8.wav"), None);
        assert_eq!(from_filename("Kick_808BPM.wav"), None);
        assert_eq!(from_filename("snare_12000bpm.wav"), None);
    }

    #[test]
    fn the_first_valid_marker_wins() {
        // Packs that name a tempo per section: the leading one is the loop's own.
        assert_eq!(from_filename("loop_100bpm_to_128bpm.wav"), Some(100.0));
    }

    #[test]
    fn a_half_tempo_written_with_a_dash_parses_as_the_whole_part() {
        assert_eq!(
            from_filename("VEC3 Special Sounds 12 Root E 126-5 BPM.wav"),
            Some(126.5)
        );
    }

    #[test]
    fn a_tempo_range_in_a_file_name_reads_its_right_end() {
        // Both ends written in full: the run beside the marker is three digits, so
        // it is read as its own number rather than as a decimal place.
        assert_eq!(from_filename("Loop 120-128 BPM.wav"), Some(128.0));
        assert_eq!(from_filename("ESM Loop 20 - 105bpm (Emaj).wav"), Some(105.0));
    }

    #[test]
    fn a_letter_glued_to_the_marker_is_a_word() {
        assert_eq!(from_filename("bpms_list_120.wav"), None);
        assert_eq!(from_filename("DS_MTS_150_drum_top_loop_rebelbpm.wav"), None);
    }

    #[test]
    fn a_decimal_after_the_marker_is_kept() {
        // Two rows in a 271k library write the tempo this way, and both used to be
        // read as the take number in front of the marker.
        assert_eq!(from_filename("RIORT_Vocal_Vocal24_BPM_138.4_E.wav"), Some(138.4));
        assert_eq!(from_filename("RIORT_Vocal_Vocal29_BPM_129.2_G#.wav"), Some(129.2));
    }

    #[test]
    fn a_clean_number_beats_a_take_number_beside_the_marker() {
        assert_eq!(from_filename("DubstepRiddim_Bass_Loop_140bpm_23_G#.wav"), Some(140.0));
        assert_eq!(from_filename("Foley Loop_128BPM (39).wav"), Some(128.0));
        assert_eq!(from_filename("Bass_F_150bpm_23.wav"), Some(150.0));
    }

    #[test]
    fn a_number_glued_to_a_word_is_still_read_when_it_is_all_there_is() {
        assert_eq!(from_filename("HNS4 Bass 26 F SubBass150 BPM.wav"), Some(150.0));
    }

    #[test]
    fn the_splice_convention_reads_a_tempo_and_a_key() {
        assert_eq!(named("KSHMR Tabla Loop 01 (87, D).wav"), Some(87.0));
        assert_eq!(named("KSHMR Synth Arp 01 (128, F#m).wav"), Some(128.0));
        assert_eq!(named("Vocal Chant (95, E Phrygian).wav"), Some(95.0));
        assert_eq!(named("Vocal Chant (95, F Phyrgian).wav"), Some(95.0));
    }

    #[test]
    fn a_parenthesis_that_names_a_key_and_a_tempo_only() {
        // A word that is not a key, or a missing key, means the group is not the
        // Splice convention and its number is not a tempo.
        assert_eq!(named("Vocal (95, Atonal).wav"), None);
        assert_eq!(named("Guitar (100, Gtr).wav"), None);
        assert_eq!(named("Vocal (95, C to Fm).wav"), None);
        assert_eq!(named("Take (95).wav"), None);
        // A track number before the group is not a tempo either.
        assert_eq!(named("KSHMR Arp 01 (abc).wav"), None);
    }

    #[test]
    fn a_folder_states_the_tempo_when_the_name_does_not() {
        assert_eq!(
            tempo("Arp_C.wav", "Bought\\Kit Angels - Gm 126BPM", Some(0.25)),
            Some(126.0)
        );
        assert_eq!(
            tempo("Stab_C.wav", "Bought\\Ghosthack\\Kit Control 135BPM", Some(0.383)),
            Some(135.0)
        );
    }

    #[test]
    fn the_nearest_folder_with_a_tempo_wins() {
        // 5.143 s is exactly 3 bars at 140 and 2.143 at 100: the outer folder is
        // ruled out by the length, so the inner one is the only reading left.
        assert_eq!(
            tempo("a.wav", "Pack\\Outer 100BPM\\Inner 140BPM", Some(5.142857)),
            Some(140.0)
        );
    }

    #[test]
    fn a_folder_that_names_a_range_is_skipped() {
        // `83-98 BPM` says the pack spans 83 to 98; no single file in it is 98.
        assert_eq!(
            tempo("break.wav", "Mark Fletcher\\83-98 BPM", Some(4.8)),
            None
        );
        // 4.8 s is 2 bars at 100 BPM, so the older folder's tempo is consistent.
        assert_eq!(
            tempo("break.wav", "Pack 100BPM\\Short Fills 120 - 130Bpm", Some(4.8)),
            Some(100.0)
        );
    }

    #[test]
    fn a_file_name_outranks_its_folder() {
        assert_eq!(
            tempo("Loop_140BPM.wav", "Pack 128BPM", Some(7.2)),
            Some(140.0)
        );
    }

    #[test]
    fn a_bare_number_in_a_loop_name_needs_a_whole_number_of_bars() {
        // 8.000 s at 120 BPM is exactly 4 bars.
        assert_eq!(tempo("Vocal Stack Loop 120.wav", "", Some(8.0)), Some(120.0));
        // 1.875 s at 120 BPM is 0.94 bars: the number is not this file's tempo.
        assert_eq!(tempo("Vocal Stack Loop 120.wav", "", Some(1.875)), None);
        // No duration to check against, so no claim.
        assert_eq!(tempo("Vocal Stack Loop 120.wav", "", None), None);
    }

    #[test]
    fn a_bare_number_in_a_loop_name_is_read_without_a_marker() {
        assert_eq!(
            tempo("BOS_DAV_Bm_120_Vocal_Stack_Loop_Void_Dry.wav", "", Some(8.0)),
            Some(120.0)
        );
        assert_eq!(
            tempo("CHIME_synth_loop_lazer_150_Fmin.wav", "Bought\\Synth Loops", Some(6.4)),
            Some(150.0)
        );
    }

    #[test]
    fn a_name_with_two_bare_candidates_is_not_guessed() {
        assert_eq!(tempo("Vocal Loop 120 140.wav", "", Some(8.0)), None);
        // 120 and 140 are both plausible, so neither is the tempo.
    }

    #[test]
    fn the_same_bare_number_twice_is_one_candidate() {
        // Packs that repeat the tempo in the name state it once, twice: refusing these
        // would refuse a file whose own name agrees with itself.
        assert_eq!(
            tempo(
                "DS_DSP_150_synth_loop_pad_rage_150_Dm.wav",
                "Pack\\loops\\synth_loops",
                Some(6.4)
            ),
            Some(150.0)
        );
        assert_eq!(
            tempo(
                "FLAVAD_128_melody_loop_organ_chords128_Dmin.wav",
                "Pack\\FLAVAD_melody_loops",
                Some(7.5)
            ),
            Some(128.0)
        );
    }

    #[test]
    fn a_one_shot_keeps_its_folder_tempo_however_long_it_is() {
        // A 42 s vocal phrase in `100BPM - Cinematic Style` is 17.5 bars at 100, and
        // the bar test says nothing about a phrase that was never cut to a grid.
        // Applying it here emptied 174 rows in a 271k library, 74% of them wrongly.
        assert_eq!(
            tempo(
                "BOS_KVDVL_Lullaby_26_Female_Solo_E_100.wav",
                "Bought\\Divine Vocal Lullabies\\100BPM - Lullaby",
                Some(42.0)
            ),
            Some(100.0)
        );
        // ...but a loop whose length contradicts the folder is still emptied.
        assert_eq!(
            tempo("Loop 22 Amin.wav", "Pack\\VEH4 128BPM", Some(7.619)),
            None
        );
    }

    #[test]
    fn a_bare_number_in_a_one_shot_is_still_refused() {
        // The folder tier no longer gates one-shots, but the bare-number tier still
        // needs the name to say loop: `Fill - 60` is the 60th fill, not 60 BPM.
        assert_eq!(
            tempo("VEH1 Fill - 60.wav", "Pack\\VEH1 Fills", Some(2.0)),
            None
        );
    }

    #[test]
    fn a_pack_index_number_does_not_pass_as_a_tempo() {
        // `VEDM Cymbal-Clap Loop 063` is the 63rd file, not a 63 BPM loop: at 63 BPM
        // its 3.75 s is 0.98 bars, which fails the bar check.
        assert_eq!(tempo("VEDM Cymbal-Clap Loop 063.wav", "", Some(3.75)), None);
        assert_eq!(tempo("Cymbal-Clap Loop 061.wav", "", Some(3.75)), None);
        // 128 BPM over 3.75 s is exactly 2 bars, so a genuine 128 passes.
        assert_eq!(tempo("Cymbal-Clap Loop 128.wav", "", Some(3.75)), Some(128.0));
    }

    #[test]
    fn a_folder_saying_loop_lets_a_bare_number_through() {
        assert_eq!(
            tempo(
                "IndianPerc_Mridangam_Ens_4_D_120_19.wav",
                "Drum - Percussion Loops",
                Some(8.0)
            ),
            Some(120.0)
        );
    }

    #[test]
    fn a_files_own_marker_outranks_a_folder_that_contradicts_it() {
        // The folder says 100 BPM, the name says 95, and the length agrees with 95
        // (10.105 s is 4.0 bars at 95 but 4.21 at 100). Tier 1 wins before the folder
        // is even consulted; the folder tier is tested on its own below.
        assert_eq!(
            tempo(
                "ESM_Guitar_Loop_Almost_One_Shot_95bpm_Bm_Dry_1.wav",
                "100bpm_Almost_One_Note_Dry",
                Some(10.105)
            ),
            Some(95.0)
        );
    }

    #[test]
    fn the_bar_tolerance_is_an_absolute_25ms() {
        // At 120 BPM a bar is 2 s, so 1 bar + 25 ms passes and + 26 ms does not.
        assert_eq!(tempo("Drum Loop 120.wav", "", Some(2.025)), Some(120.0));
        assert_eq!(tempo("Drum Loop 120.wav", "", Some(2.026)), None);
        // The window does not grow with the number of bars: 16 bars at 120 BPM is
        // 32 s, and 32.026 s is still refused. A relative tolerance used to pass
        // here (0.02 × 32 s = 640 ms), which let an unrelated number through on
        // every long file.
        assert_eq!(tempo("Drum Loop 120.wav", "", Some(32.025)), Some(120.0));
        assert_eq!(tempo("Drum Loop 120.wav", "", Some(32.026)), None);
    }

    #[test]
    fn a_short_one_shot_keeps_the_folder_tempo() {
        // Too short to argue either way, so the folder's claim stands.
        assert_eq!(
            tempo("Ghosthack-AC25_Kit_Angels_Arp_C.wav", "Kits\\Angels 126BPM", Some(0.25)),
            Some(126.0)
        );
    }

    #[test]
    fn a_sub_60_bare_number_is_not_read_as_a_tempo() {
        // Below the bare window: these are bar counts, and 32 bars is not 32 BPM.
        assert_eq!(tempo("Loop 32.wav", "", Some(38.4)), None);
    }
}
