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

//! ## UCS as a second, weaker pass
//!
//! `ucs.rs` carries a vocabulary derived from the Universal Category System
//! (version 8.2.1, <https://www.universalcategorysystem.com/>), filtered by
//! measurement against the same corpus. It is consulted **only when the curated
//! pass above returns `Uncategorized`**, which makes "a UCS word never overrides
//! a curated answer" a structural property instead of an argument about weights.
//!
//! The filter is in `tools/ucs_mapping.py` and `tools/import-ucs.py`: a UCS
//! synonym is admitted only when the files that carry it in their *filename* are
//! already classified the way the mapping says they should be. Words whose
//! evidence disagrees (`trance` is a UCS synonym for MAGIC-SPELL and a genre in
//! this library) are dropped. Measured effect on the 271,350-path reference
//! corpus: 1,388 files gain a category, 0 already-classified files change.
//!
//! A filename that opens with a UCS CatID (`GORESrce_…`) is a special case: it
//! has *declared* its category, so it is not a guess and is not filtered the same
//! way, only checked for whether the mapping contradicts the corpus.

mod dictionary;
mod ucs;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use std::collections::HashMap;

pub use dictionary::CATEGORY_DICTIONARY;
pub use ucs::{UCS_CAT_ID_NATIVE, UCS_CAT_IDS, UCS_DICTIONARY, UCS_VERSION};

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
    /// Native UCS fields, populated only when the filename declares a CatID.
    pub ucs_category: Option<String>,
    pub ucs_subcategory: Option<String>,
    pub ucs_cat_id: Option<String>,
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
            ucs_category: None,
            ucs_subcategory: None,
            ucs_cat_id: None,
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Vocabulary {
    Curated,
    Ucs,
}

struct KeywordPattern {
    target: Target,
    kind: PatternKind,
}

#[derive(Clone, Copy)]
enum PatternKind {
    Keyword {
        vocabulary: Vocabulary,
        exact: bool,
    },
    Phrase {
        vocabulary: Vocabulary,
        order: usize,
    },
}

struct KeywordMatches {
    curated: Vec<Option<usize>>,
    ucs: Vec<Option<usize>>,
    phrases: std::collections::HashSet<(Vocabulary, usize)>,
}

struct TokenSpan {
    text: String,
    start: usize,
    end: usize,
    raw_start: usize,
    directory: bool,
    description: bool,
}

/// Built once per process (or once per indexing run) and then shared across
/// threads: it is immutable and contains no interior mutability.
///
/// Three tables, consulted in this order:
///
/// 1. `keyword_to_target` / `phrases` — the curated vocabulary. Weight 3 in the
///    filename, 4 for a phrase, 1 per directory vote.
/// 2. `cat_id_to_target` — UCS CatIDs. A filename that opens with one has
///    declared its category, so this scores 8, above any combination of guesses.
/// 3. `ucs_keyword_to_target` / `ucs_phrases` — the UCS fallback. Only consulted
///    when 1 and 2 produced no match at all, so it can never move a file the
///    curated dictionary already decided.
pub struct Classifier {
    keyword_matcher: AhoCorasick,
    keyword_patterns: Vec<KeywordPattern>,
    phrases: Vec<Phrase>,
    ucs_phrases: Vec<Phrase>,
    cat_id_to_target: HashMap<&'static str, Target>,
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
        let ucs_keyword_to_target = build_ucs_keywords();
        let ucs_phrases = build_ucs_phrases();
        let (keyword_matcher, keyword_patterns) = build_pattern_matcher(
            &keyword_to_target,
            &phrases,
            &ucs_keyword_to_target,
            &ucs_phrases,
        );
        Self {
            keyword_matcher,
            keyword_patterns,
            phrases,
            ucs_phrases,
            cat_id_to_target: build_cat_ids(),
        }
    }

    pub fn classify(&self, full_path: &str) -> Classification {
        self.classify_with_description(full_path, None)
    }

    pub fn classify_with_description(
        &self,
        full_path: &str,
        description: Option<&str>,
    ) -> Classification {
        let (haystack, mut tokens, filename_start) = tokenize_path_spans(full_path, description);
        let filename = &full_path[filename_start..];
        let native_ucs = native_ucs_for_filename(filename);
        for token in &mut tokens {
            token.directory = token.raw_start < filename_start && !token.description;
        }
        let matches = find_pattern_matches(
            &haystack,
            &tokens,
            &self.keyword_matcher,
            &self.keyword_patterns,
        );

        // Scores are first-insertion ordered rather than a `HashMap` because the
        // tie-break walks them in that order: on a tie the first target that
        // scored wins, which keeps classification deterministic and single-pass.
        let mut scores: Vec<(Target, u32)> = Vec::new();
        let mut matched: Vec<String> = Vec::new();

        for (order, phrase) in self.phrases.iter().enumerate() {
            if matches.phrases.contains(&(Vocabulary::Curated, order)) {
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
        for (index, token) in tokens.iter().enumerate() {
            if !token.directory {
                continue;
            }
            // A brand token in a folder name is the pack's label, never its
            // contents. `Fox Stevenson Universal Samples` is not about foxes.
            if BRAND_TOKENS.contains(&token.text.as_str()) {
                continue;
            }
            let Some(pattern_index) = matches.curated[index] else {
                continue;
            };
            let target = &self.keyword_patterns[pattern_index].target;
            if dir_voted.contains(&target) {
                continue;
            }
            dir_voted.push(target);
            add(&mut scores, &mut matched, target, 1, &token.text);
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
        for (index, token) in tokens.iter().enumerate() {
            if token.directory {
                continue;
            }
            if seen.contains(&token.text) {
                continue;
            }
            seen.push(token.text.clone());
            // A pack's own name is usually the first word of every filename in
            // it — `FOX_STVNSN_BASS_SHOT`, `CHIME_vox_chop_male_ohh` — and it is
            // the artist, not the sound. Those are skipped rather than deleted
            // from the dictionary, because the same word is a real index term
            // elsewhere: `chime` is a bell here, `Chime` is an artist.
            let is_first = first;
            first = false;
            if is_first && BRAND_TOKENS.contains(&token.text.as_str()) {
                continue;
            }
            let Some(pattern_index) = matches.curated[index] else {
                continue;
            };
            let target = &self.keyword_patterns[pattern_index].target;
            if file_voted.contains(&target) {
                continue;
            }
            file_voted.push(target);
            add(
                &mut scores,
                &mut matched,
                target,
                if token.description { 2 } else { 3 },
                &token.text,
            );
        }

        // 4. Nothing above fired. Now — and only now — the UCS layers get a look:
        //    a declared CatID first, then the UCS vocabulary. Doing this as a
        //    second pass rather than as low-weight votes in the same score is what
        //    makes "UCS never overrides the curated answer" true by construction
        //    instead of by weight arithmetic.
        if scores.is_empty() {
            return self.classify_fallback(filename, &tokens, &matches, native_ucs);
        }

        finish(scores, matched, native_ucs)
    }

    /// The UCS pass: a declared CatID, then UCS words and phrases.
    ///
    /// Reached only from [`classify`](Self::classify) when the curated pass
    /// produced no match at all, so every decision here is a decision the curated
    /// dictionary abstained from.
    ///
    /// Directories vote here too, and for the same reason they do in the curated
    /// pass: `VEH4 Slides & Scratch\VEH4 Slide Scratch 10.wav` has no UCS word in
    /// its filename, and the folder is the only description it has. The cap is
    /// what makes this safe — one vote per target, whatever the folder says.
    fn classify_fallback(
        &self,
        filename: &str,
        tokens: &[TokenSpan],
        matches: &KeywordMatches,
        native_ucs: Option<(String, Option<String>, String)>,
    ) -> Classification {
        let mut scores: Vec<(Target, u32)> = Vec::new();
        let mut matched: Vec<String> = Vec::new();

        // A filename opening with a UCS CatID has declared its category
        // (`GORESrce_SMorph_Gore 2 - Builds Bash Impact Wet.wav`). That is not a
        // guess, so it outranks any word in the same filename.
        if let Some(cat_id) = cat_id_prefix(filename) {
            if let Some(target) = self.cat_id_to_target.get(cat_id) {
                add(&mut scores, &mut matched, target, 8, cat_id);
            }
        }

        for (order, phrase) in self.ucs_phrases.iter().enumerate() {
            if matches.phrases.contains(&(Vocabulary::Ucs, order)) {
                add(&mut scores, &mut matched, &phrase.target, 3, phrase.phrase);
            }
        }

        let mut dir_voted: Vec<&Target> = Vec::new();
        for (index, token) in tokens.iter().enumerate() {
            if !token.directory || BRAND_TOKENS.contains(&token.text.as_str()) {
                continue;
            }
            let Some(pattern_index) = matches.ucs[index] else {
                continue;
            };
            let target = &self.keyword_patterns[pattern_index].target;
            if dir_voted.contains(&target) {
                continue;
            }
            dir_voted.push(target);
            add(&mut scores, &mut matched, target, 1, &token.text);
        }

        let mut voted: Vec<&Target> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut first = true;
        for (index, token) in tokens.iter().enumerate() {
            if token.directory || seen.contains(&token.text) {
                continue;
            }
            seen.push(token.text.clone());
            let is_first = first;
            first = false;
            if is_first && BRAND_TOKENS.contains(&token.text.as_str()) {
                continue;
            }
            let Some(pattern_index) = matches.ucs[index] else {
                continue;
            };
            let target = &self.keyword_patterns[pattern_index].target;
            if voted.contains(&target) {
                continue;
            }
            voted.push(target);
            add(
                &mut scores,
                &mut matched,
                target,
                if token.description { 1 } else { 2 },
                &token.text,
            );
        }

        finish(scores, matched, native_ucs)
    }
}

/// Pick the winner from accumulated scores and package it as a [`Classification`].
///
/// A subcategory-level match beats a bare-category match on a tie, so
/// `Weapons::Firearms` wins over `Weapons`.
fn finish(
    scores: Vec<(Target, u32)>,
    mut matched: Vec<String>,
    native_ucs: Option<(String, Option<String>, String)>,
) -> Classification {
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
        let mut classification = Classification::uncategorized();
        apply_native_ucs(&mut classification, native_ucs);
        return classification;
    };
    if raw_score < 1 {
        let mut classification = Classification::uncategorized();
        apply_native_ucs(&mut classification, native_ucs);
        return classification;
    }

    // Saturation at 8: one filename word reads as weak, a phrase plus a filename
    // word lands near 1.0.
    let confidence = (raw_score as f64 / 8.0).min(1.0);

    let mut seen_terms = std::collections::HashSet::new();
    matched.retain(|term| seen_terms.insert(term.clone()));

    Classification {
        category: target.category.to_string(),
        subcategory: target.subcategory.map(str::to_string),
        confidence,
        raw_score,
        matched_terms: matched,
        ucs_category: native_ucs.as_ref().map(|(category, _, _)| category.clone()),
        ucs_subcategory: native_ucs
            .as_ref()
            .and_then(|(_, subcategory, _)| subcategory.clone()),
        ucs_cat_id: native_ucs.map(|(_, _, cat_id)| cat_id),
    }
}

fn native_ucs_for_filename(filename: &str) -> Option<(String, Option<String>, String)> {
    let cat_id = cat_id_prefix(filename)?;
    let (category, subcategory) = UCS_CAT_ID_NATIVE
        .iter()
        .find(|(known, _, _)| *known == cat_id)
        .map(|(_, category, subcategory)| {
            ((*category).to_string(), subcategory.map(str::to_string))
        })?;
    Some((category, subcategory, cat_id.to_string()))
}

fn apply_native_ucs(
    classification: &mut Classification,
    native_ucs: Option<(String, Option<String>, String)>,
) {
    classification.ucs_category = native_ucs.as_ref().map(|(category, _, _)| category.clone());
    classification.ucs_subcategory = native_ucs
        .as_ref()
        .and_then(|(_, subcategory, _)| subcategory.clone());
    classification.ucs_cat_id = native_ucs.map(|(_, _, cat_id)| cat_id);
}

/// Accumulate a vote for `target`, merging into its existing score if it has one.
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

fn build_cat_ids() -> HashMap<&'static str, Target> {
    UCS_CAT_IDS
        .iter()
        .map(|(cat_id, category, subcategory)| {
            (
                *cat_id,
                Target {
                    category,
                    subcategory: *subcategory,
                },
            )
        })
        .collect()
}

fn build_ucs_keywords() -> HashMap<&'static str, Target> {
    let mut map = HashMap::new();
    for def in UCS_DICTIONARY {
        let target = Target {
            category: def.category,
            subcategory: def.subcategory,
        };
        for kw in def.keywords {
            map.insert(*kw, target.clone());
        }
    }
    map
}

fn build_pattern_matcher(
    curated: &HashMap<&'static str, Target>,
    curated_phrases: &[Phrase],
    ucs: &HashMap<&'static str, Target>,
    ucs_phrases: &[Phrase],
) -> (AhoCorasick, Vec<KeywordPattern>) {
    let mut variants: HashMap<(String, Vocabulary, bool), Target> = HashMap::new();
    for (vocabulary, keywords) in [(Vocabulary::Curated, curated), (Vocabulary::Ucs, ucs)] {
        let mut ordered: Vec<_> = keywords.iter().collect();
        ordered.sort_by_key(|(word, _)| **word);
        for (word, target) in ordered {
            variants.insert(((*word).to_string(), vocabulary, true), target.clone());
            for suffix in ["ing", "ed", "es", "s"] {
                let variant = format!("{word}{suffix}");
                if stem(&variant) == *word {
                    variants
                        .entry((variant, vocabulary, false))
                        .or_insert_with(|| target.clone());
                }
            }
        }
    }

    let mut patterns = Vec::new();
    for ((text, vocabulary, exact), target) in variants {
        patterns.push((
            text,
            KeywordPattern {
                target,
                kind: PatternKind::Keyword { vocabulary, exact },
            },
        ));
    }
    for (vocabulary, phrases) in [
        (Vocabulary::Curated, curated_phrases),
        (Vocabulary::Ucs, ucs_phrases),
    ] {
        for (order, phrase) in phrases.iter().enumerate() {
            let normalized = normalise(phrase.phrase).trim().to_string();
            let normalized = normalized.replace(' ', " ");
            patterns.push((
                normalized.clone(),
                KeywordPattern {
                    target: phrase.target.clone(),
                    kind: PatternKind::Phrase { vocabulary, order },
                },
            ));
        }
    }
    patterns.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| pattern_priority(&a.1).cmp(&pattern_priority(&b.1)))
            .then_with(|| a.1.target.category.cmp(b.1.target.category))
            .then_with(|| a.1.target.subcategory.cmp(&b.1.target.subcategory))
    });
    let matcher = AhoCorasickBuilder::new()
        .match_kind(MatchKind::Standard)
        .build(patterns.iter().map(|(pattern, _)| pattern.as_str()))
        .expect("static classifier patterns are valid");
    (
        matcher,
        patterns.into_iter().map(|(_, pattern)| pattern).collect(),
    )
}

fn pattern_priority(pattern: &KeywordPattern) -> (u8, u8) {
    match pattern.kind {
        PatternKind::Keyword { vocabulary, exact } => (
            if vocabulary == Vocabulary::Curated {
                0
            } else {
                1
            },
            if exact { 0 } else { 1 },
        ),
        PatternKind::Phrase { vocabulary, order } => (
            if vocabulary == Vocabulary::Curated {
                2
            } else {
                3
            },
            order as u8,
        ),
    }
}

fn tokenize_path_spans(path: &str, description: Option<&str>) -> (String, Vec<TokenSpan>, usize) {
    let mut haystack = String::with_capacity(path.len());
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut word_start = 0;
    let mut word_raw_start = 0;
    let mut last_separator = None;

    for (raw_offset, character) in path.char_indices() {
        if character.is_ascii_alphanumeric() {
            if word.is_empty() {
                word_start = haystack.len();
                word_raw_start = raw_offset;
            }
            let lower = character.to_ascii_lowercase();
            word.push(lower);
            haystack.push(lower);
            continue;
        }

        push_token(
            &mut word,
            word_start,
            word_raw_start,
            haystack.len(),
            &mut tokens,
            false,
        );
        if character == '\\' || character == '/' {
            last_separator = Some(raw_offset);
        }
        if character.is_alphanumeric() {
            haystack.push('\u{1}');
        } else {
            haystack.push(' ');
        }
    }
    push_token(
        &mut word,
        word_start,
        word_raw_start,
        haystack.len(),
        &mut tokens,
        false,
    );
    let filename_start = last_separator.map(|offset| offset + 1).unwrap_or(0);
    if let Some(description) = description {
        let base = path.len() + 1;
        haystack.push(' ');
        append_text_tokens(description, base, &mut haystack, &mut tokens);
    }
    (haystack, tokens, filename_start)
}

fn append_text_tokens(
    text: &str,
    raw_offset: usize,
    haystack: &mut String,
    tokens: &mut Vec<TokenSpan>,
) {
    let mut word = String::new();
    let mut word_start = haystack.len();
    let mut word_raw_start = raw_offset;
    for (offset, character) in text.char_indices() {
        if character.is_ascii_alphanumeric() {
            if word.is_empty() {
                word_start = haystack.len();
                word_raw_start = raw_offset + offset;
            }
            let lower = character.to_ascii_lowercase();
            word.push(lower);
            haystack.push(lower);
        } else {
            push_token(
                &mut word,
                word_start,
                word_raw_start,
                haystack.len(),
                tokens,
                true,
            );
            haystack.push(' ');
        }
    }
    push_token(
        &mut word,
        word_start,
        word_raw_start,
        haystack.len(),
        tokens,
        true,
    );
}

fn push_token(
    word: &mut String,
    start: usize,
    raw_start: usize,
    end: usize,
    tokens: &mut Vec<TokenSpan>,
    description: bool,
) {
    if word.len() >= MIN_TOKEN_LEN && !STOP_WORDS.contains(&word.as_str()) {
        tokens.push(TokenSpan {
            text: std::mem::take(word),
            start,
            end,
            raw_start,
            directory: false,
            description,
        });
    } else {
        word.clear();
    }
}

fn find_pattern_matches(
    haystack: &str,
    tokens: &[TokenSpan],
    matcher: &AhoCorasick,
    patterns: &[KeywordPattern],
) -> KeywordMatches {
    let mut found = KeywordMatches {
        curated: vec![None; tokens.len()],
        ucs: vec![None; tokens.len()],
        phrases: std::collections::HashSet::new(),
    };
    for matched in matcher.find_overlapping_iter(haystack) {
        let pattern_index = matched.pattern().as_usize();
        let pattern = &patterns[pattern_index];
        match pattern.kind {
            PatternKind::Phrase { vocabulary, order } => {
                found.phrases.insert((vocabulary, order));
            }
            PatternKind::Keyword { vocabulary, exact } => {
                let Ok(index) = tokens.binary_search_by_key(&matched.start(), |token| token.start)
                else {
                    continue;
                };
                if tokens[index].end != matched.end() {
                    continue;
                }
                let slots = match vocabulary {
                    Vocabulary::Curated => &mut found.curated,
                    Vocabulary::Ucs => &mut found.ucs,
                };
                let current_is_exact = slots[index].is_some_and(|current| {
                    matches!(
                        patterns[current].kind,
                        PatternKind::Keyword { exact: true, .. }
                    )
                });
                if slots[index].is_none() || (exact && !current_is_exact) {
                    slots[index] = Some(pattern_index);
                }
            }
        }
    }
    found
}

fn build_ucs_phrases() -> Vec<Phrase> {
    let mut phrases = Vec::new();
    for def in UCS_DICTIONARY {
        let target = Target {
            category: def.category,
            subcategory: def.subcategory,
        };
        for phrase in def.phrases {
            phrases.push(Phrase {
                phrase,
                target: target.clone(),
            });
        }
    }
    phrases
}

/// The UCS CatID a filename declares, if any.
///
/// Matched with case intact. UCS writes CatIDs in CamelCase (`GORESrce`,
/// `VehCar`, `AMBUrbn`) and a handful are ordinary English words in upper case
/// (`RAIN`, `WIND`, `STORM`, `TEST`); folding case would let a file named
/// `rain_on_roof.wav` claim the `RAIN` CatID.
///
/// The token after the CatID must be one UCS itself uses as a separator (`_`,
/// `-`, space, or `.`), so `GORESrce2.wav` is not read as `GORESrce`.
fn cat_id_prefix(filename: &str) -> Option<&str> {
    let stem_end = filename.rfind('.').unwrap_or(filename.len());
    let stem = &filename[..stem_end];
    let head_end = stem.find(['_', '-', ' ', '.']).unwrap_or(stem.len());
    let head = &stem[..head_end];
    if head.is_empty() { None } else { Some(head) }
}

/// Split into (directory part with its separators intact, filename).
///
/// The directory part keeps its separators; tokenisation turns them into spaces
/// on its own, so a token never spans two folder names.
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
    "sfx",
    "sound",
    "sounds",
    "audio",
    "stereo",
    "mono",
    "wav",
    "mp3",
    "ogg",
    "aif",
    "aiff",
    "flac",
    "raw",
    "snd",
    "take",
    "final",
    "master",
    "export",
    "render",
    "loop",
    "looped",
    "loops",
    "ver",
    "version",
    "bit",
    "sample",
    "samples",
    "pack",
    "packs",
    "bundle",
    "edition",
    "complete",
    "formats",
    "format",
    "custom",
    "library",
    "vol",
    "volume",
    "bpm",
    "kit",
    "kits",
    "epic",
    "cinematic",
    "trailer",
    "game",
    "games",
    "source",
    "designed",
    "stock",
    "media",
    "sonniss",
    "gdc",
    "aaa",
    "esm",
    "spg",
    "vedm",
    "dpte",
    "advanced",
    "mobile",
    "app",
    "starter",
    "essentials",
    "essential",
    "ultimate",
    "modern",
    "pro",
    "professional",
    "premium",
    "collection",
    "series",
    "part",
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
#[cfg(test)]
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
    fn an_exact_keyword_beats_a_stem_variant_in_the_same_token() {
        let root = Target {
            category: "Foley",
            subcategory: None,
        };
        let exact = Target {
            category: "Weapons",
            subcategory: None,
        };
        let curated = HashMap::from([("dog", root), ("dogs", exact)]);
        let ucs = HashMap::new();
        let (matcher, patterns) = build_pattern_matcher(&curated, &[], &ucs, &[]);
        let (_, tokens, _) = tokenize_path_spans("dogs.wav", None);
        let matches = find_pattern_matches("dogs wav", &tokens, &matcher, &patterns);
        let target = &patterns[matches.curated[0].expect("exact match")].target;
        assert_eq!(target.category, "Weapons");
    }

    #[test]
    fn phrase_patterns_can_overlap_in_the_single_automaton_scan() {
        let target = Target {
            category: "Foley",
            subcategory: None,
        };
        let phrases = vec![
            Phrase {
                phrase: "ha ha",
                target: target.clone(),
            },
            Phrase {
                phrase: "ha ha ha",
                target,
            },
        ];
        let empty = HashMap::new();
        let (matcher, patterns) = build_pattern_matcher(&empty, &phrases, &empty, &[]);
        let (haystack, tokens, _) = tokenize_path_spans("ha-ha-ha.wav", None);
        let found = find_pattern_matches(&haystack, &tokens, &matcher, &patterns);
        assert!(found.phrases.contains(&(Vocabulary::Curated, 0)));
        assert!(found.phrases.contains(&(Vocabulary::Curated, 1)));
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
        // Neither the curated dictionary nor the UCS table knows these tokens.
        // Avoid `zzz`, which UCS lists as a snore sound.
        let c = classify(r"D:\lib\qwzx\qwertyuiop.wav");
        assert_eq!(c.category, "Uncategorized");
        assert_eq!(c.confidence, 0.0);
        assert!(c.matched_terms.is_empty());
    }

    #[test]
    fn metadata_description_can_supply_a_curated_signal() {
        let classifier = Classifier::new();
        let result = classifier
            .classify_with_description(r"C:\lib\unknown_001.wav", Some("Metal, Impact, Hit"));
        assert_eq!(result.category, "Impacts");
        assert!(result.matched_terms.iter().any(|term| term == "metal"));
    }

    #[test]
    fn native_ucs_fields_keep_the_official_cat_id_label() {
        let result = classify(r"C:\lib\GORESrce_source_01.wav");
        assert_eq!(result.ucs_cat_id.as_deref(), Some("GORESrce"));
        assert_eq!(result.ucs_category.as_deref(), Some("GORE"));
        assert_eq!(result.ucs_subcategory.as_deref(), Some("SOURCE"));
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

    // ---------------------------------------------------------------------
    // UCS fallback. The curated pass must always win; these pin the cases that
    // would break if the fallback were ever folded into the same score table.
    // ---------------------------------------------------------------------

    #[test]
    fn a_ucs_word_never_overrides_a_curated_answer() {
        // `magic` and `spell` are curated (Magic / Spells & Casts). If a UCS word
        // could outvote them this file would move to whatever UCS says a spell is.
        let c = classify(r"D:\lib\x\magic spell arcane sparkle 01.wav");
        assert_eq!(c.category, "Magic");
        // The UCS table is only reachable when the curated pass scores nothing, so
        // a file the curated dictionary names keeps its answer no matter what.
        let curated = CATEGORY_DICTIONARY
            .iter()
            .filter(|d| d.keywords.contains(&"magic") && d.category != "Magic");
        assert_eq!(curated.count(), 0, "no other category may claim `magic`");
    }

    #[test]
    fn a_declared_cat_id_decides_a_file_the_curated_dictionary_ignores() {
        // `GORESqsh` declares Creatures/Gore & Violence. The rest of the filename
        // is opaque to both dictionaries, so the declaration is the only evidence
        // and this pins that it is read.
        let c = classify(r"D:\lib\344 Audio\x\GORESqsh_zxqv 07.wav");
        assert_eq!(c.category, "Creatures");
        assert_eq!(c.subcategory.as_deref(), Some("Gore & Violence"));
        assert!(
            c.matched_terms.iter().any(|t| t == "GORESqsh"),
            "the CatID should be reported as a matched term, got {:?}",
            c.matched_terms
        );
    }

    #[test]
    fn a_curated_word_beats_a_declared_cat_id() {
        // The other half of the same rule: a CatID lives in the fallback pass, so
        // it cannot override the curated dictionary. `liquid` and `drip` are both
        // curated (Foley/Water Foley), and they win even though this filename
        // declares GORE.
        let c = classify(r"D:\lib\x\GORESqsh_liquid drip 01.wav");
        assert_eq!(c.category, "Foley");
        assert_eq!(c.subcategory.as_deref(), Some("Water Foley"));
    }

    #[test]
    fn a_cat_id_is_matched_with_its_case_intact() {
        // `RAIN` and `WIND` are UCS CatIDs and also the most ordinary words in a
        // nature library. A lowercase lookup would let every `rain_on_roof.wav`
        // claim a declared category, so the head is compared as written.
        assert_eq!(cat_id_prefix("GORESqsh_x.wav"), Some("GORESqsh"));
        assert_eq!(cat_id_prefix("rain_on_roof.wav"), Some("rain"));
        assert_eq!(cat_id_prefix("nounderscore.wav"), Some("nounderscore"));
        assert_eq!(cat_id_prefix(".wav"), None);

        // The curated word decides these, which is the point: no declaration is
        // read from a lowercase head.
        let rain = classify(r"D:\lib\x\rain on a tin roof 01.wav");
        assert_eq!(rain.category, "Nature");
        assert_eq!(rain.subcategory.as_deref(), Some("Water & Rain"));
    }

    #[test]
    fn an_uncategorized_path_can_be_rescued_by_the_ucs_vocabulary() {
        // `barbershop` and `hair dryer` are not in the curated dictionary. The UCS
        // table reaches it, which is the whole reason for the fallback pass.
        let c = classify(r"D:\lib\x\Barbershop hair dryer idle off.wav");
        assert_ne!(
            c.category, "Uncategorized",
            "the UCS pass should have decided"
        );
        assert!(c.raw_score > 0);
    }

    #[test]
    fn the_ucs_table_never_contradicts_a_curated_target() {
        // A word listed here and in the curated dictionary under a different
        // target would be dead weight at best and a silent override at worst.
        let curated: HashMap<&str, (&str, Option<&str>)> = CATEGORY_DICTIONARY
            .iter()
            .flat_map(|d| {
                d.keywords
                    .iter()
                    .map(move |kw| (*kw, (d.category, d.subcategory)))
            })
            .collect();
        for def in UCS_DICTIONARY {
            for kw in def.keywords {
                if let Some((category, subcategory)) = curated.get(kw) {
                    assert_eq!(
                        (*category, *subcategory),
                        (def.category, def.subcategory),
                        "UCS word {kw:?} contradicts the curated dictionary"
                    );
                }
            }
        }
    }

    #[test]
    fn every_cat_id_maps_to_a_real_category() {
        let known: std::collections::HashSet<&str> =
            CATEGORY_DICTIONARY.iter().map(|d| d.category).collect();
        for (cat_id, category, subcategory) in UCS_CAT_IDS {
            assert!(
                known.contains(category),
                "CatID {cat_id:?} maps to unknown category {category:?}"
            );
            if let Some(sub) = subcategory {
                assert!(
                    CATEGORY_DICTIONARY
                        .iter()
                        .any(|d| d.category == *category && d.subcategory == Some(*sub)),
                    "CatID {cat_id:?} maps to unknown subcategory {category:?}/{sub:?}"
                );
            }
        }
    }

    #[test]
    fn every_ucs_keyword_is_reachable_by_the_tokenizer() {
        for def in UCS_DICTIONARY {
            for kw in def.keywords {
                assert_eq!(
                    tokenize(kw),
                    vec![kw.to_string()],
                    "UCS keyword {kw:?} ({:?}) can never be produced by the tokenizer",
                    def.category
                );
            }
            for phrase in def.phrases {
                assert!(
                    !phrase.is_empty() && phrase.chars().all(|c| !c.is_uppercase()),
                    "UCS phrase {phrase:?} must be lowercase and non-empty"
                );
            }
        }
    }

    #[test]
    fn ucs_version_is_recorded() {
        assert_eq!(UCS_VERSION, "8.2.1");
    }
}
