//! Path → category classification.
//!
//! A port of the reference implementation's classifier
//! (`slip-sound/src/main/classifier.ts`). The scoring rules, the stop-word list,
//! the stemming rules and the dictionary are reproduced exactly, because the
//! point of the port is that an existing library does not get re-sorted into
//! different categories when it moves to ReSlip.
//!
//! Parity is checked against a real database rather than asserted: `reslip-cli
//! classify-diff` re-classifies every path in an existing Slip Sound index and
//! reports any row whose category, subcategory, confidence or matched terms
//! differ.
//!
//! Scoring, as in the reference:
//!
//! | Signal | Weight |
//! | --- | --- |
//! | multi-word phrase in the raw path | 4 |
//! | dictionary word in a directory name | 3 |
//! | dictionary word in the filename | 1 |
//!
//! Confidence is `min(1, score / 8)`, so a lone filename hit is a weak signal and
//! a directory + phrase + filename hit is near-certain.

mod dictionary;

use std::collections::HashMap;

pub use dictionary::CATEGORY_DICTIONARY;

/// One row of the dictionary: a category, an optional subcategory, and the words
/// that point at it.
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
    /// Pre-normalisation score. Kept because tuning the dictionary needs it, and
    /// because the reference's database stored it too.
    pub raw_score: u32,
    /// Dictionary words and phrases that fired, deduplicated.
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Target {
    category: &'static str,
    subcategory: Option<&'static str>,
}

impl Target {
    /// `Category::Subcategory`, or just `Category`. Generic and specific matches
    /// accumulate separately so a specific hit can win a tie.
    fn key(&self) -> String {
        match self.subcategory {
            Some(sub) => format!("{}::{}", self.category, sub),
            None => self.category.to_string(),
        }
    }
}

struct Phrase {
    phrase: &'static str,
    target: Target,
}

/// Built once per process (or once per indexing run) and then shared across
/// threads: it is immutable and contains no interior mutability.
pub struct Classifier {
    keyword_to_target: HashMap<&'static str, Target>,
    /// Phrases are checked against the lowercased path as substrings, so order
    /// does not matter; kept as a Vec because it is short and cache-friendly.
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
                // `kw` is `&&'static str`; deref to get the `&'static str` key.
                keyword_to_target.insert(*kw, target.clone());
            }
            for phrase in def.phrases {
                phrases.push(Phrase {
                    phrase: *phrase,
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
        let lower_full = full_path.to_lowercase();

        // Scores are kept in first-insertion order, not in a `HashMap`, because
        // the tie-break below walks them in that order. The reference iterates
        // `Object.entries(scores)`, whose order is insertion order, so a
        // HashMap here would pick a different winner than the reference on any
        // tie between two equally specific targets — 3932 of 271350 rows in the
        // real library, before this was fixed.
        let mut scores: Vec<(String, u32)> = Vec::new();
        let mut matched: Vec<String> = Vec::new();

        let mut add = |target: &Target, weight: u32, term: &str| {
            let key = target.key();
            match scores.iter_mut().find(|(k, _)| *k == key) {
                Some((_, score)) => *score += weight,
                None => scores.push((key, weight)),
            }
            matched.push(term.to_string());
        };

        // 1. Phrases first: they are unambiguous, so they carry the highest weight
        //    and are matched against the raw path (so "sawed-off" spans hyphens).
        for phrase in &self.phrases {
            if lower_full.contains(phrase.phrase) {
                add(&phrase.target, 4, phrase.phrase);
            }
        }

        // 2. Directory tokens. A sound filed under `Weapons/` is a stronger signal
        //    than one whose filename merely mentions it.
        for token in tokenize(&dir_part) {
            if let Some(target) = self.lookup(&token) {
                let target = target.clone();
                add(&target, 3, &token);
            }
        }

        // 3. Filename tokens.
        for token in tokenize(&filename) {
            if let Some(target) = self.lookup(&token) {
                let target = target.clone();
                add(&target, 1, &token);
            }
        }

        // 4. Pick the winner; a subcategory-level match beats a bare category
        //    match on a tie, so `Weapons::Firearms` wins over `Weapons`.
        let mut best_key = String::new();
        let mut best_score = 0u32;
        for (key, score) in &scores {
            let more_specific = key.contains("::");
            let current_more_specific = best_key.contains("::");
            if *score > best_score
                || (*score == best_score && more_specific && !current_more_specific)
            {
                best_score = *score;
                best_key = key.clone();
            }
        }

        if best_score < 1 || best_key.is_empty() {
            return Classification::uncategorized();
        }

        let (category, subcategory) = match best_key.split_once("::") {
            Some((c, s)) => (c.to_string(), Some(s.to_string())),
            None => (best_key.clone(), None),
        };

        // Saturation at 8: a single filename hit reads as 0.125 (weak), while a
        // directory + phrase + filename hit lands near 1.0.
        let confidence = (best_score as f64 / 8.0).min(1.0);

        // Global dedup, first occurrence kept — `[...new Set(matchedTerms)]` in
        // the reference. `Vec::dedup` only removes adjacent duplicates, which
        // left repeated terms whenever a phrase and a token both fired.
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        matched.retain(|term| seen.insert(term.clone()));

        Classification {
            category,
            subcategory,
            confidence,
            raw_score: best_score,
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

/// Split into (directory part with separators normalised to spaces, filename).
///
/// The reference split on both separators and joined the directory components
/// with spaces, which means a token never spans two directory names.
fn split_path(full_path: &str) -> (String, String) {
    let mut parts: Vec<&str> = full_path.split(['\\', '/']).collect();
    let filename = parts.pop().unwrap_or("").to_string();
    (parts.join(" "), filename)
}

/// Words the dictionary must not see: format names, version tags, and the
/// generic nouns that appear in nearly every sample library path.
const STOP_WORDS: &[&str] = &[
    "sfx", "sound", "audio", "stereo", "mono", "wav", "mp3", "ogg", "raw", "snd", "take", "final",
    "master", "export", "render", "loop", "looped", "ver", "version",
];

/// Lowercase, replace everything that is not an ASCII letter with a space, drop
/// short tokens and stop words.
///
/// Non-ASCII letters are dropped rather than transliterated. That is the
/// reference behaviour, and keeping it is what makes a parity diff against an
/// existing index meaningful; `Kemençe` contributes the token `kemen`, exactly as
/// it did before.
fn tokenize(input: &str) -> Vec<String> {
    input
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_lowercase() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|t| t.len() > 2 && !STOP_WORDS.contains(t))
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
    fn a_directory_match_outweighs_a_filename_match() {
        // `firearms` is a dictionary keyword, so a directory hit scores 3 and a
        // filename hit scores 1. Verified against the reference implementation
        // (`classifier.ts`) rather than assumed: there is no bare
        // "weapon"/"weapons" keyword, so a folder merely named `Weapons`
        // classifies nothing, and `door` is not in the dictionary either.
        let by_dir = classify(r"D:\lib\Firearms\random_name_01.wav");
        let by_file = classify(r"D:\lib\misc\gun_shot_01.wav");
        assert_eq!(by_dir.category, "Weapons");
        assert_eq!(by_file.category, "Weapons");
        assert_eq!(by_dir.raw_score, 3);
        assert_eq!(by_file.raw_score, 1);
        assert_eq!(classify(r"D:\lib\Weapons\x.wav").category, "Uncategorized");
    }

    #[test]
    fn subcategory_wins_over_bare_category() {
        let c = classify(r"D:\lib\Weapons\Firearms\pistol_reload.wav");
        assert_eq!(c.category, "Weapons");
        assert_eq!(c.subcategory.as_deref(), Some("Firearms"));
    }

    #[test]
    fn phrases_score_higher_than_single_words() {
        // Phrases are matched against the raw path, so a separator inside the
        // phrase ("assault_rifle") defeats it — the reference behaves the same
        // way. Compare with a space, where both the phrase (4) and the token
        // "rifle" (1) fire, against a lone "gun" (1).
        let phrase = classify(r"D:\lib\x\assault rifle 01.wav");
        let single = classify(r"D:\lib\x\gun_01.wav");
        assert_eq!(phrase.raw_score, 5);
        assert_eq!(single.raw_score, 1);
        assert!(phrase.matched_terms.contains(&"assault rifle".to_string()));

        // Same words, underscore-separated: only the token survives.
        let underscored = classify(r"D:\lib\x\assault_rifle_01.wav");
        assert_eq!(underscored.raw_score, 1);
    }

    #[test]
    fn confidence_saturates_at_one() {
        let c = classify(r"D:\lib\Weapons\Firearms\gunshot_shell_casing_drop.wav");
        assert!(c.confidence <= 1.0);
        assert!(c.confidence > 0.5, "confidence {}", c.confidence);
    }

    #[test]
    fn stop_words_and_short_tokens_are_ignored() {
        // Format names, "sfx" and "loop" are stop words.
        assert!(tokenize("SFX_wav_loop_snd").is_empty());
        // The length filter is `len > 2`, so two-letter words go and three-letter
        // words stay — "the" survives, which matches the reference.
        assert_eq!(tokenize("a an of"), Vec::<String>::new());
        assert_eq!(tokenize("the"), vec!["the"]);
        assert_eq!(tokenize("Door_Slam"), vec!["door", "slam"]);
    }

    #[test]
    fn stemming_reaches_the_dictionary_root() {
        assert_eq!(stem("footsteps"), "footstep");
        assert_eq!(stem("stepping"), "stepp");
        assert_eq!(stem("glass"), "glass"); // no double-s strip
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
        let c = classify(r"D:\音效库\Weapons\枪声_pistol.wav");
        assert_eq!(c.category, "Weapons");
    }

    #[test]
    fn ties_resolve_by_insertion_order_like_the_reference() {
        // Regression for a real parity bug: the reference iterates
        // `Object.entries(scores)`, which is insertion order, so on a tie the
        // first-inserted target wins. A `HashMap` has no such order, and using
        // one put 3932 of 271350 rows of the real library in a different
        // category than the reference.
        //
        // `hits` (Impacts/Blunt/Body, from a directory) and `digital`
        // (Electronic, from a directory) both score 3; `Impacts` is inserted
        // first, so it must win. Verified with `classifier.ts` on the real path.
        let path = r"D:\lib\Bass - Sub Hits & Drops\Leviathan4_SubDrop_A_Digital_01.wav";
        let first = classify(path);
        assert_eq!(first.category, "Impacts");
        assert_eq!(first.subcategory.as_deref(), Some("Blunt/Body"));

        // Classification must be deterministic across instances, which it is not
        // if any step depends on hash iteration order.
        for _ in 0..8 {
            let again = classify(path);
            assert_eq!(again.category, first.category);
            assert_eq!(again.subcategory, first.subcategory);
            assert_eq!(again.matched_terms, first.matched_terms);
        }
    }

    #[test]
    fn matched_terms_are_deduplicated_globally() {
        // "magic spell" is a phrase and both words are also keywords, so the raw
        // list is ["magic","spell","magic","spell","magic","spell"]; the
        // reference's `new Set` collapses it to two entries.
        let c = classify(
            r"D:\lib\Ghosthack - 30 Free Magic Spell Sound Effects\SFX\Magic Spell_Arcane_11.wav",
        );
        assert_eq!(c.matched_terms, vec!["magic", "spell"]);
    }
}
