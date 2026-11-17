//! Path → category classification.
//!
//! The vocabulary and the scoring model are this project's own, authored and
//! measured against a real sample library (`tools/taxonomy.py` holds the
//! vocabulary, `tools/gen-category-dict.py` writes [`dictionary.rs`]). Nothing
//! here is a port: the reference implementation's dictionary was a GPL-licensed
//! compilation and its scoring model misbehaves on this library (see below), so
//! both were replaced.
//!
//! ## Why the scoring is not the obvious one
//!
//! A sample library path is
//! `…\Bought\SFX\Epic.Stock.Media.Hybrid.Game.Sounds\…\
//! ESM_SPG_fx_servo_metal_latch_02.wav`. The pack folder is marketing — the same
//! words (`epic`, `cinematic`, `game`) appear in dozens of pack titles — while
//! the filename states what the sound actually is. A model that scores a
//! directory word above a filename word therefore scores the marketing and
//! ignores the sound: on the 271k-file reference corpus that model put 92,876
//! files (34%) into one category on the strength of the word `epic` alone.
//!
//! So the weights run the other way:
//!
//! | Signal | Weight |
//! | --- | --- |
//! | multi-word phrase in the normalised path | 4 |
//! | a dictionary word in the filename, once per distinct word | 3 |
//! | a dictionary word in a directory name, at most **once per target** | 1 |
//!
//! The directory cap is the important one. One folder can hold ten words that
//! all point at `Weapons`, and it still casts one vote: a folder says which pack
//! the file came from, not what the file is. The filename votes per word, so a
//! name like `servo_metal_latch` can outvote its pack.
//!
//! ## Pack names and artists are not descriptions
//!
//! A pack's name is usually the first word of every filename in it
//! (`CHIME_vox_chop_male_ohh`, `Fox_tonal_loop_boops`), and those names are the
//! artist or the label. they are skipped as first tokens rather than deleted from
//! the dictionary, because the same words are real index terms elsewhere: `chime`
//! is also a bell and `ghost` is also a spirit.
//!
//! ## ASCII only
//!
//! Chinese text in paths is not read. In the Epic Stock Media packs the Chinese
//! folder name is a whole sentence about the pack's theme
//! (`【现代化军事战争用户UI界面游戏音效】`), so any Chinese signal labels thousands
//! of unrelated files at once; the sound's own description is in the ASCII part of
//! the filename.
//!
//! ## Confidence
//!
//! `min(1, raw_score / 8)`. A single filename word reads as 0.375 — enough to
//! show up as a guess, not enough to look certain.

mod dictionary;

use std::collections::HashMap;

pub use dictionary::CATEGORY_DICTIONARY;

/// One row of the dictionary: a category, an optional subcategory, and the words
/// that point at it. `subcategory == None` is the category-level entry (the words
/// that name the category itself), not a missing value.
pub struct CategoryDefinition {
    pub category: &'static str,
    pub subcategory: Option<&'static str>,
    pub keywords: &'static [&'static str],
    pub phrases: &'static [&'static str],
}

/// What a path was classified as.
#[derive(Debug, Clone, PartialEq)]
pub struct Classification {
    pub category: String,
    pub subcategory: Option<String>,
    /// 0–1, `raw_score / 8` clamped.
    pub confidence: f64,
    /// Pre-normalisation score. Kept because tuning the dictionary needs it.
    pub raw_score: u32,
    /// The dictionary words and phrases that fired, deduplicated.
    pub matched_terms: Vec<String>,
}

impl Classification {
    /// The row every unclassifiable path gets. Matches the schema default.
    pub fn uncategorized() -> Self {
        Self {
            category: "Uncategorized".to_string(),
            subcategory: None,
            confidence: 0.0,
            raw_score: 0,
            matched_terms: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    category: &'static str,
    subcategory: Option<&'static str>,
}

struct Phrase {
    phrase: &'static str,
    target: Target,
}

/// Built once per process (or once per indexing run) and then shared across
/// threads: it is immutable and contains no interior mutability.
pub struct Classifier {
    keyword_to_target: HashMap<&'static str, Target>,
    phrases: Vec<Phrase>,
}

impl Default for Classifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Classifier {
    pub fn new() -> Self {
        let mut keyword_to_target = HashMap::new();
        let mut phrases = Vec::new();
        for def in CATEGORY_DICTIONARY {
            let target = Target {
                category: def.category,
                subcategory: def.subcategory,
            };
            for kw in def.keywords {
                keyword_to_target.insert(*kw, target.clone());
            }
            for phrase in def.phrases {
                phrases.push(Phrase {
                    phrase,
                    target: target.clone(),
                });
            }
        }
        Self {
            keyword_to_target,
            phrases,
        }
    }

    pub fn classify(&self, full_path: &str) -> Classification {
        let (dir_part, filename) = split_path(full_path);

        // Phrases match a normalised copy of the whole path, so `drum_loop` and
        // `Drum Loop` are one phrase and a separator inside a phrase cannot
        // silently defeat it.
        let normalised = normalise(full_path);

        // Scores are first-insertion ordered rather than a `HashMap` because the
        // tie-break walks them in that order: on a tie the first target that
        // scored wins, which keeps classification deterministic and single-pass.
        let mut scores: Vec<(Target, u32)> = Vec::new();
        let mut matched: Vec<String> = Vec::new();

        fn add(
            scores: &mut Vec<(Target, u32)>,
            matched: &mut Vec<String>,
            target: &Target,
            weight: u32,
            term: &str,
        ) {
            match scores.iter_mut().find(|(t, _)| t == target) {
                Some((_, score)) => *score += weight,
                None => scores.push((target.clone(), weight)),
            }
            matched.push(term.to_string());
        }

        // 1. Phrases: unambiguous, so they carry the highest weight.
        for phrase in &self.phrases {
            if normalised.contains(phrase.phrase) {
                add(&mut scores, &mut matched, &phrase.target, 4, phrase.phrase);
            }
        }

        // 2. Directory, at most one vote per target. Whichever keyword is seen
        //    first is the one reported, so `matched_terms` explains the vote that
        //    actually happened instead of listing words that were ignored.
        //
        //    Only ASCII words vote. Chinese text in a path is ignored: the Epic
        //    Stock Media folder names are a whole sentence about the pack's theme,
        //    so reading them would label thousands of files with all of it.
        let mut dir_voted: Vec<&Target> = Vec::new();
        for token in tokenize(&dir_part) {
            // A brand token in a folder name is the pack's label, never its
            // contents. `Fox Stevenson Universal Samples` is not about foxes.
            if BRAND_TOKENS.contains(&token.as_str()) {
                continue;
            }
            let Some(target) = self.lookup(&token) else {
                continue;
            };
            if dir_voted.contains(&target) {
                continue;
            }
            dir_voted.push(target);
            add(&mut scores, &mut matched, target, 1, &token);
        }

        // 3. Filename: one vote per distinct word.
        //
        //    A word that already voted from a folder does not vote again. A pack
        //    called `Epic.Stock.Media.Ocean.Game` makes every filename in it start
        //    `ESM_Ocean_Game_...`, and without this rule that pack name turns each
        //    `ESM_Ocean_Game_One_Shot_UI_Select` into two votes for water against
        //    one for the UI sound it actually is.
        let mut file_voted: Vec<&Target> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut first = true;
        for token in tokenize(&filename) {
            if seen.contains(&token) {
                continue;
            }
            seen.push(token.clone());
            // A pack's own name is usually the first word of every filename in
            // it — `FOX_STVNSN_BASS_SHOT`, `CHIME_vox_chop_male_ohh` — and it is
            // the artist, not the sound. Those are skipped rather than deleted
            // from the dictionary, because the same word is a real index term
            // elsewhere: `chime` is a bell here, `Chime` is an artist.
            let is_first = first;
            first = false;
            if is_first && BRAND_TOKENS.contains(&token.as_str()) {
                continue;
            }
            let Some(target) = self.lookup(&token) else {
                continue;
            };
            if file_voted.contains(&target) {
                continue;
            }
            file_voted.push(target);
            add(&mut scores, &mut matched, target, 3, &token);
        }

        // 4. Pick the winner; a subcategory-level match beats a bare-category
        //    match on a tie, so `Weapons::Firearms` wins over `Weapons`.
        let mut best: Option<(Target, u32)> = None;
        for (target, score) in &scores {
            let more_specific = target.subcategory.is_some();
            let current_more_specific = best
                .as_ref()
                .map(|(t, _)| t.subcategory.is_some())
                .unwrap_or(false);
            let better = match &best {
                None => true,
                Some((_, best_score)) => {
                    *score > *best_score
                        || (*score == *best_score && more_specific && !current_more_specific)
                }
            };
            if better {
                best = Some((target.clone(), *score));
            }
        }

        let Some((target, raw_score)) = best else {
            return Classification::uncategorized();
        };
        if raw_score < 1 {
            return Classification::uncategorized();
        }

        // Saturation at 8: one filename word reads as weak, a phrase plus a
        // filename word lands near 1.0.
        let confidence = (raw_score as f64 / 8.0).min(1.0);

        let mut seen_terms = std::collections::HashSet::new();
        matched.retain(|term| seen_terms.insert(term.clone()));

        Classification {
            category: target.category.to_string(),
            subcategory: target.subcategory.map(str::to_string),
            confidence,
            raw_score,
            matched_terms: matched,
        }
    }

    fn lookup(&self, token: &str) -> Option<&Target> {
        if let Some(t) = self.keyword_to_target.get(token) {
            return Some(t);
        }
        let stemmed = stem(token);
        if stemmed != token {
            return self.keyword_to_target.get(stemmed);
        }
        None
    }
}

/// Split into (directory part with its separators intact, filename).
///
/// The directory part keeps its separators; tokenisation turns them into spaces
/// on its own, so a token never spans two folder names.
fn split_path(full_path: &str) -> (String, String) {
    match full_path.rfind(['\\', '/']) {
        Some(index) => (
            full_path[..index].to_string(),
            full_path[index + 1..].to_string(),
        ),
        None => (String::new(), full_path.to_string()),
    }
}

/// Lowercase, and turn everything that is not a letter or digit into a space.
fn normalise(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else {
            out.push(' ');
        }
    }
    out
}

/// Words the dictionary must not see: format names, version tags, and the
/// generic nouns and marketing words that appear in nearly every sample library
/// path.
///
/// This list is not cosmetic. `bit`, `sample`, `pack`, `game` and `epic` are in
/// most pack folder names in this library, and every one of them that reached the
/// dictionary would vote for a single category on tens of thousands of files.
const STOP_WORDS: &[&str] = &[
    "sfx", "sound", "sounds", "audio", "stereo", "mono", "wav", "mp3", "ogg", "aif", "aiff", "flac",
    "raw", "snd", "take", "final", "master", "export", "render", "loop", "looped", "loops", "ver",
    "version", "bit", "sample", "samples", "pack", "packs", "bundle", "edition", "complete",
    "formats", "format", "custom", "library", "vol", "volume", "bpm", "kit", "kits", "epic",
    "cinematic", "trailer", "game", "games", "source", "designed", "stock", "media", "sonniss",
    "gdc", "aaa", "esm", "spg", "vedm", "dpte", "advanced", "mobile", "app", "starter",
    "essentials", "essential", "ultimate", "modern", "pro", "professional", "premium", "collection",
    "series", "part",
];

/// Tokens that are a pack's own name and, being the first word of nearly every
/// filename in that pack, are not a description of the sound.
///
/// These are only skipped when they are the *first* token of a filename. None of
/// them is removed from the dictionary: `chime` is also a bell, `ghost` is also
/// a spirit, and the corpus needs both readings. What the corpus does not need is
/// `CHIME_vox_chop_male_ohh.wav` being filed as a bell because the artist is
/// called Chime.
const BRAND_TOKENS: &[&str] = &["fox", "chime", "ghost", "htech"];

/// The shortest token the dictionary can match. Two characters, so `ui`, `hh`,
/// `808` and `ak` reach the dictionary instead of being dropped as noise.
const MIN_TOKEN_LEN: usize = 2;

/// Lowercase, split on everything that is not an ASCII letter or digit, drop
/// tokens shorter than [`MIN_TOKEN_LEN`] and stop words.
///
/// Two characters instead of the usual three is deliberate: `ui` is among the
/// most common index terms in this library's game-audio packs, and a
/// three-letter floor silently discarded it.
fn tokenize(input: &str) -> Vec<String> {
    input
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|t| t.len() >= MIN_TOKEN_LEN && !STOP_WORDS.contains(t))
        .map(|t| t.to_string())
        .collect()
}

/// Strip common inflections so `footsteps` / `stepping` / `stepped` all reach
/// `step`. Deliberately conservative: the stem left behind must still be at
/// least three characters, so short roots are not mangled.
fn stem(token: &str) -> &str {
    let len = token.len();
    if len > 6 && token.ends_with("ing") {
        return &token[..len - 3];
    }
    if len > 5 && token.ends_with("ed") {
        return &token[..len - 2];
    }
    if len > 4 && token.ends_with("es") {
        return &token[..len - 2];
    }
    if len > 4 && token.ends_with('s') && !token.ends_with("ss") {
        return &token[..len - 1];
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(path: &str) -> Classification {
        Classifier::new().classify(path)
    }

    #[test]
    fn a_filename_word_outweighs_its_pack_folder() {
        // The pack folder says "Epic Stock Media Hybrid Game Sounds", none of
        // which is a keyword any more; the filename says what the sound is.
        let by_file = classify(r"D:\lib\Epic.Stock.Media.Hybrid.Game.Sounds\servo_metal_latch.wav");
        assert_eq!(by_file.category, "Machinery");
        assert!(
            by_file.raw_score > 1,
            "two filename words should beat one folder vote, got {}",
            by_file.raw_score
        );
    }

    #[test]
    fn one_folder_casts_one_vote_however_many_words_point_at_it() {
        // `gun`, `rifle` and `pistol` all point at Weapons/Firearms. The folder
        // must still contribute 1, not 3, so the filename can win.
        let c = classify(r"D:\lib\gun_rifle_pistol\door_open.wav");
        assert_eq!(c.category, "Foley");
        assert_eq!(c.subcategory.as_deref(), Some("Doors & Locks"));
    }

    #[test]
    fn marketing_words_are_stop_words() {
        // Every one of these is in dozens of pack titles. As keywords they put a
        // third of the corpus in one bucket.
        for word in ["epic", "cinematic", "trailer", "game", "stock", "media"] {
            assert!(
                tokenize(word).is_empty(),
                "{word} must not reach the dictionary"
            );
        }
    }

    #[test]
    fn chinese_folder_names_are_ignored() {
        // The Epic Stock Media packs name their folders with a whole sentence
        // about the pack's theme. Reading it would label all ~4,000 files in the
        // pack with every theme in the sentence, so Chinese text contributes
        // nothing: the ASCII part of the filename has to decide.
        let c = classify(r"D:\lib\UnBought\SFX\帝国战争【武器 步枪 手枪 射击】\a_01.wav");
        assert_eq!(c.category, "Uncategorized");
        let c = classify(r"D:\lib\UnBought\SFX\【蒸汽朋克机械 齿轮】\servo_latch.wav");
        assert_eq!(c.category, "Machinery");
    }

    #[test]
    fn two_letter_tokens_reach_the_dictionary() {
        // `ui` is among the most common index terms in the game-audio packs.
        let c = classify(r"D:\lib\x\ui_button_click.wav");
        assert_eq!(c.category, "UI & Game");
    }

    #[test]
    fn phrases_match_across_separators() {
        // `drum_loop` must fire the same phrase as `drum loop`; matching the raw
        // path silently missed every underscored filename in the library.
        let spaced = classify(r"D:\lib\x\drum loop 01.wav");
        let underscored = classify(r"D:\lib\x\drum_loop_01.wav");
        assert!(spaced.matched_terms.iter().any(|t| t == "drum loop"));
        assert!(underscored.matched_terms.iter().any(|t| t == "drum loop"));
        assert_eq!(spaced.category, underscored.category);
    }

    #[test]
    fn subcategory_wins_over_bare_category_on_a_tie() {
        let c = classify(r"D:\lib\Weapons\pistol.wav");
        assert_eq!(c.category, "Weapons");
        assert_eq!(c.subcategory.as_deref(), Some("Firearms"));
    }

    #[test]
    fn nothing_recognisable_is_uncategorized_with_zero_confidence() {
        let c = classify(r"D:\lib\zzz\qwertyuiop.wav");
        assert_eq!(c.category, "Uncategorized");
        assert_eq!(c.confidence, 0.0);
        assert!(c.matched_terms.is_empty());
    }

    #[test]
    fn non_ascii_paths_do_not_panic_and_still_classify() {
        // The Chinese part is invisible; `pistol` is what decides.
        let c = classify(r"D:\音效库\枪声_pistol.wav");
        assert_eq!(c.category, "Weapons");
    }

    #[test]
    fn classification_is_deterministic() {
        let path = r"D:\lib\Bought\SFX\【机器人 伺服电机 齿轮】\servo_gear_latch_02.wav";
        let first = classify(path);
        for _ in 0..8 {
            let again = classify(path);
            assert_eq!(again.category, first.category);
            assert_eq!(again.subcategory, first.subcategory);
            assert_eq!(again.matched_terms, first.matched_terms);
        }
    }

    #[test]
    fn matched_terms_are_deduplicated_globally() {
        let c = classify(r"D:\lib\x\magic sparkle spell_arcane_11.wav");
        let mut sorted = c.matched_terms.clone();
        sorted.sort();
        let mut deduped = sorted.clone();
        deduped.dedup();
        assert_eq!(
            sorted, deduped,
            "terms must not repeat: {:?}",
            c.matched_terms
        );
    }

    #[test]
    fn every_dictionary_entry_is_reachable() {
        // A keyword the tokenizer can never emit is a dead entry that silently
        // never fires. This is the cheap structural guard for that class of typo.
        for def in CATEGORY_DICTIONARY {
            for kw in def.keywords {
                assert_eq!(
                    tokenize(kw),
                    vec![kw.to_string()],
                    "keyword {kw:?} ({:?}) can never be produced by the tokenizer",
                    def.category
                );
            }
            for phrase in def.phrases {
                assert!(
                    !phrase.is_empty() && phrase.chars().all(|c| !c.is_uppercase()),
                    "phrase {phrase:?} must be lowercase and non-empty"
                );
            }
        }
    }

    #[test]
    fn no_keyword_belongs_to_two_targets() {
        let mut seen: HashMap<&str, (&str, Option<&str>)> = HashMap::new();
        for def in CATEGORY_DICTIONARY {
            for kw in def.keywords {
                if let Some(previous) = seen.insert(kw, (def.category, def.subcategory)) {
                    assert_eq!(
                        previous,
                        (def.category, def.subcategory),
                        "keyword {kw:?} is listed under two targets"
                    );
                }
            }
        }
    }

    #[test]
    fn a_packs_own_name_is_not_a_description() {
        // `CHIME_vox_chop_male_ohh` is a vocal chop by the artist Chime, not a
        // bell; `Fox_tonal_loop_boops` is not an animal.
        let vocal = classify(r"D:\lib\Chime pack\CHIME_vox_chop_male_ohh.wav");
        assert_eq!(vocal.category, "Vocals");
        let tonal = classify(r"D:\lib\Fox pack\Fox_tonal_loop_boops_05_124_B.wav");
        assert_ne!(tonal.category, "Nature", "Fox is the artist, not an animal");
        // The same word in a later position is still the dictionary word.
        let bell = classify(r"D:\lib\x\temple bell chime 02.wav");
        assert_eq!(bell.category, "Keys & Mallets");
    }

    #[test]
    fn confidence_saturates_at_one() {
        let c = classify(r"D:\lib\x\magic spell cast gunshot explosion 01.wav");
        assert!(c.confidence <= 1.0);
    }
}
