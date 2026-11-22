//! Tempo read from a file name.
//!
//! Sample packs put the tempo in the name (`KSHMR_Kick_128BPM_Cmin.wav`,
//! `Drum Loop 174 bpm.wav`, `BPM120_vox`), and a browser that can sort by it saves
//! opening the same loop in a DAW to find out. Nothing here reads audio: the only
//! source is the name, because the header of a WAV does not carry tempo and
//! guessing it from the waveform would be a different (and much slower) job.
//!
//! **A name without an explicit marker yields `None`.** A bare `120` in
//! `Snare_120.wav` is a take number as often as a tempo, and a wrong number that
//! sorts confidently is worse than an empty cell: the user asked for unmarked
//! files to stay unmarked. So the number has to sit next to the letters `bpm`.
//!
//! The plausible range is 20–300. Below 20 is a count of bars, above 300 is a
//! sample count or a frequency; neither is a tempo a music library contains.

/// Longest and shortest tempo accepted, in BPM.
const MIN_BPM: f64 = 20.0;
const MAX_BPM: f64 = 300.0;

/// Separators allowed between a number and `bpm`, and inside the number's
/// surroundings. `128BPM`, `128 BPM`, `128_bpm`, `128-BPM`, `(128 bpm)` all parse.
fn is_sep(c: char) -> bool {
    matches!(c, ' ' | '_' | '-' | '.' | '(' | ')' | '[' | ']' | '~' | '#')
}

/// Read a run of ASCII digits ending just before `end`, returning the value and
/// the index where the run starts.
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

/// Tempo encoded in a file name, or `None` when the name does not state one.
pub fn from_filename(name: &str) -> Option<f64> {
    let lower = name.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();

    let mut i = 0;
    while i + 3 <= chars.len() {
        if chars[i] == 'b' && chars[i + 1] == 'p' && chars[i + 2] == 'm' {
            // A letter glued to `bpm` is a word, not a marker (`bpms`, `abpm`).
            let glued_before = i > 0 && chars[i - 1].is_ascii_alphabetic();
            let glued_after = chars.get(i + 3).is_some_and(|c| c.is_ascii_alphabetic());

            if !glued_before && !glued_after {
                // `128bpm`: the number precedes the marker.
                let mut end = i;
                while end > 0 && is_sep(chars[end - 1]) {
                    end -= 1;
                }
                if let Some((value, start)) = digits_before(&chars, end) {
                    // `126-5 BPM` is one pack's way of writing a half tempo: the
                    // digit run next to the marker is a single decimal place whose
                    // whole part sits before the separator. A tempo *range* writes
                    // both ends in full (`120-128 BPM`), so its leading run is two
                    // or three digits and this branch does not touch it. The value
                    // is kept to one decimal rather than rounded: 126.5 is the
                    // number the name states, and the column renders it as such.
                    if end - start == 1 && start >= 2 {
                        let sep = chars[start - 1];
                        let whole_end = start - 1;
                        if matches!(sep, '-' | '.') {
                            if let Some((whole, whole_start)) = digits_before(&chars, whole_end) {
                                if (2..=3).contains(&(whole_end - whole_start)) {
                                    if let Some(bpm) = accept(whole) {
                                        return Some(bpm + f64::from(value) / 10.0);
                                    }
                                }
                            }
                        }
                    }
                    let len = end - start;
                    let preceded_by_digit = start > 0 && chars[start - 1].is_ascii_digit();
                    if (2..=3).contains(&len) && !preceded_by_digit {
                        if let Some(bpm) = accept(value) {
                            return Some(bpm);
                        }
                    }
                }

                // `bpm128` / `bpm 128`: the number follows.
                let mut start = i + 3;
                while start < chars.len() && is_sep(chars[start]) {
                    start += 1;
                }
                if let Some((value, end)) = digits_after(&chars, start) {
                    let len = end - start;
                    let followed_by_digit = chars.get(end).is_some_and(|c| c.is_ascii_digit());
                    if (2..=3).contains(&len) && !followed_by_digit {
                        if let Some(bpm) = accept(value) {
                            return Some(bpm);
                        }
                    }
                }
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    None
}

fn accept(value: u32) -> Option<f64> {
    let bpm = f64::from(value);
    if (MIN_BPM..=MAX_BPM).contains(&bpm) {
        Some(bpm)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_name_without_a_marker_stays_unmarked() {
        assert_eq!(from_filename("Snare_120.wav"), None);
        assert_eq!(from_filename("door_slam_02.wav"), None);
        assert_eq!(from_filename("Kick_808.wav"), None);
        assert_eq!(from_filename("hihat_44k_24bit.wav"), None);
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
        // One pack writes 126.5 as `126-5`. The value reported is the whole part:
        // 126.5 sorts identically to 126 in any practical comparison, and the
        // alternative is an empty cell next to a name that clearly states a tempo.
        assert_eq!(
            from_filename("VEC3 Special Sounds 12 Root E 126-5 BPM.wav"),
            Some(126.5)
        );
    }

    #[test]
    fn a_tempo_range_is_not_mistaken_for_a_decimal() {
        // Both ends written in full: the run beside the marker is three digits, so
        // it is read as its own number rather than as a decimal place.
        assert_eq!(from_filename("Loop 120-128 BPM.wav"), Some(128.0));
    }

    #[test]
    fn a_letter_glued_to_the_marker_is_a_word() {
        assert_eq!(from_filename("bpms_list_120.wav"), None);
    }
}
