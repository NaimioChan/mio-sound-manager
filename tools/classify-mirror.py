"""A Python mirror of `crates/mio-index/src/classify/mod.rs`, for measuring.

The classifier is tuned by measurement, and a Rust rebuild per candidate
vocabulary is a slow loop. This mirrors the scoring exactly so a candidate
dictionary can be scored against the whole corpus in seconds.

`parity(db)` re-classifies every row of an index the Rust side produced and
reports disagreements. **Run it before trusting any number this file prints**: a
mirror that has drifted produces plausible-looking measurements of a classifier
that does not exist.

Two optional layers on top of the curated dictionary, both off by default so the
parity check stays honest:

- `ucs_keywords`: a second, weaker tier. Curated words keep weight 3 in the
  filename; UCS words get weight `UCS_FILE_WEIGHT` (1), so a UCS synonym can only
  decide a file the curated vocabulary says nothing about.
- `cat_ids`: a filename that opens with a UCS CatID (`GORESrce_...`) has declared
  its category. Matched case-sensitively, because UCS writes CatIDs in CamelCase
  and a lowercase match would let the ordinary word `rain` claim `RAIN`.
"""

import os
import re

WEIGHT_PHRASE = 4
WEIGHT_FILE = 3
WEIGHT_DIR = 1
WEIGHT_CAT_ID = 8
# The UCS fallback weights, mirrored from `crates/mio-index/src/classify/mod.rs`:
# a phrase 3, a filename word 2, a directory vote 1. No curated weight is used
# here because this pass only runs when the curated one scored nothing.
UCS_FILE_WEIGHT = 2
UCS_PHRASE_WEIGHT = 3
SATURATION = 8.0
MIN_TOKEN_LEN = 2

STOP_WORDS = {
    "sfx", "sound", "sounds", "audio", "stereo", "mono", "wav", "mp3", "ogg", "aif", "aiff", "flac",
    "raw", "snd", "take", "final", "master", "export", "render", "loop", "looped", "loops", "ver",
    "version", "bit", "sample", "samples", "pack", "packs", "bundle", "edition", "complete",
    "formats", "format", "custom", "library", "vol", "volume", "bpm", "kit", "kits", "epic",
    "cinematic", "trailer", "game", "games", "source", "designed", "stock", "media", "sonniss",
    "gdc", "aaa", "esm", "spg", "vedm", "dpte", "advanced", "mobile", "app", "starter",
    "essentials", "essential", "ultimate", "modern", "pro", "professional", "premium", "collection",
    "series", "part",
}

BRAND_TOKENS = {"fox", "chime", "ghost", "htech"}


def split_path(full_path):
    cut = max(full_path.rfind("\\"), full_path.rfind("/"))
    if cut < 0:
        return "", full_path
    return full_path[:cut], full_path[cut + 1:]


def normalise(text):
    """Lowercase alphanumerics, everything else to a space. Not ASCII-filtered."""
    return "".join(ch.lower() if ch.isalnum() else " " for ch in text)


def normalise_ascii(text):
    """The tokenizer's view: ASCII alphanumerics only."""
    return "".join(ch.lower() if ch.isascii() and ch.isalnum() else " " for ch in text)


def tokenize(text):
    out = []
    for raw in normalise_ascii(text).split():
        if len(raw) < MIN_TOKEN_LEN or raw in STOP_WORDS:
            continue
        out.append(raw)
    return out


def stem(token):
    n = len(token)
    if n > 6 and token.endswith("ing"):
        return token[:-3]
    if n > 5 and token.endswith("ed"):
        return token[:-2]
    if n > 4 and token.endswith("es"):
        return token[:-2]
    if n > 4 and token.endswith("s") and not token.endswith("ss"):
        return token[:-1]
    return token


def cat_id_prefix(filename, cat_ids):
    """The CatID a filename declares, or None.

    Compared with its case intact: `GORESrce_x.wav` declares, `goresrce` does
    not. The separator after the CatID has to be one UCS uses.
    """
    if not cat_ids:
        return None
    stem_ = os.path.splitext(filename)[0]
    head = re.split(r"[_\-\s.]", stem_, maxsplit=1)[0]
    return head if head in cat_ids else None


class Classifier:
    def __init__(self, taxonomy, cat_ids=None, ucs_keywords=None, ucs_phrases=None):
        self.keyword_to_target = {}
        self.phrases = []
        for cat, subs in taxonomy.items():
            for sub, (kws, phs) in subs.items():
                target = (cat, None if sub == "__bare__" else sub)
                for kw in kws.split():
                    self.keyword_to_target.setdefault(kw, target)
                for phrase in (p.strip() for p in phs.split("|")):
                    if phrase:
                        self.phrases.append((phrase, target))
        self.cat_ids = {k: tuple(v) for k, v in (cat_ids or {}).items()}
        self.ucs = {}
        for target, words in (ucs_keywords or {}).items():
            for word in words:
                # Complete UCS A scores both vocabularies. A shared word still
                # contributes one vote to each target table, and the Rust Aho
                # matcher keeps the vocabulary-specific match alongside the
                # curated one.
                self.ucs.setdefault(word, tuple(target))
        self.ucs_phrases = [(p, tuple(t)) for t, ps in (ucs_phrases or {}).items() for p in ps]

    def lookup(self, token):
        """Curated only. The UCS table has its own lookup, in the fallback pass."""
        hit = self.keyword_to_target.get(token)
        if hit is not None:
            return hit
        st = stem(token)
        if st != token:
            return self.keyword_to_target.get(st)
        return None

    def lookup_ucs(self, token):
        hit = self.ucs.get(token)
        if hit is not None:
            return hit
        st = stem(token)
        if st != token:
            return self.ucs.get(st)
        return None

    def classify(self, path):
        """Score curated and UCS evidence together, with CatID as the declaration."""
        dir_part, filename = split_path(path)
        normalised = normalise(path)

        scores, matched = [], []

        def add(target, weight, term):
            for i, (t, s) in enumerate(scores):
                if t == target:
                    scores[i] = (t, s + weight)
                    break
            else:
                scores.append((target, weight))
            matched.append(term)

        declared = cat_id_prefix(filename, self.cat_ids)
        if declared:
            add(self.cat_ids[declared], WEIGHT_CAT_ID, declared)

        for phrase, target in self.phrases:
            if phrase in normalised:
                add(target, WEIGHT_PHRASE, phrase)

        for phrase, target in self.ucs_phrases:
            if phrase in normalised:
                add(target, UCS_PHRASE_WEIGHT, phrase)

        # Keep the same insertion order as Rust: curated directory, UCS
        # directory, curated filename, UCS filename. Tie-breaking depends on it.
        dir_voted = []
        for token in tokenize(dir_part):
            if token in BRAND_TOKENS:
                continue
            target = self.lookup(token)
            if target is None or target in dir_voted:
                continue
            dir_voted.append(target)
            add(target, WEIGHT_DIR, token)

        dir_voted = []
        for token in tokenize(dir_part):
            if token in BRAND_TOKENS:
                continue
            target = self.lookup_ucs(token)
            if target is None or target in dir_voted:
                continue
            dir_voted.append(target)
            add(target, WEIGHT_DIR, token)

        file_voted = []
        seen = set()
        first = True
        for token in tokenize(filename):
            if token in seen:
                continue
            seen.add(token)
            is_first = first
            first = False
            if is_first and token in BRAND_TOKENS:
                continue
            target = self.lookup(token)
            if target is None or target in file_voted:
                continue
            file_voted.append(target)
            add(target, WEIGHT_FILE, token)

        voted = []
        seen = set()
        first = True
        for token in tokenize(filename):
            if token in seen:
                continue
            seen.add(token)
            is_first = first
            first = False
            if is_first and token in BRAND_TOKENS:
                continue
            target = self.lookup_ucs(token)
            if target is None or target in voted:
                continue
            voted.append(target)
            add(target, UCS_FILE_WEIGHT, token)
        return self.finish(scores, matched)

    @staticmethod
    def finish(scores, matched):
        best = None
        for target, score in scores:
            if best is None:
                best = (target, score)
                continue
            more = target[1] is not None
            cur_more = best[0][1] is not None
            if score > best[1] or (score == best[1] and more and not cur_more):
                best = (target, score)
        if best is None or best[1] < 1:
            return ("Uncategorized", None, 0.0, 0)
        target, raw = best
        seen_terms, terms = set(), []
        for term in matched:
            if term not in seen_terms:
                seen_terms.add(term)
                terms.append(term)
        return (target[0], target[1], min(1.0, raw / SATURATION), raw)


def load_ucs(module_path=None):
    """Read `tools/ucs_keywords.py` as (cat_ids, keywords, phrases)."""
    import importlib.util

    here = os.path.dirname(os.path.abspath(__file__))
    path = module_path or os.path.join(here, "ucs_keywords.py")
    spec = importlib.util.spec_from_file_location("ucs_keywords", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.CAT_IDS, module.KEYWORDS, module.PHRASES


def parity(db_path, verbose=True):
    """Every row of a Rust-produced index must classify the same way here."""
    import sqlite3
    import sys

    here = os.path.dirname(os.path.abspath(__file__))
    sys.path.insert(0, here)
    import taxonomy

    conn = sqlite3.connect(db_path)
    rows = conn.execute(
        "SELECT path, category, subcategory FROM sounds WHERE category_manual = 0"
    ).fetchall()
    clf = Classifier(taxonomy.TAXONOMY)
    mismatch = 0
    examples = []
    for path, cat, sub in rows:
        got = clf.classify(path)
        if got[0] != cat or got[1] != sub:
            mismatch += 1
            if len(examples) < 10:
                examples.append((path, (cat, sub), got[:2]))
    if verbose:
        print(f"parity: {len(rows)} rows, {mismatch} mismatches")
        for e in examples:
            print("  ", e)
    return mismatch


def main():
    """Emit expected classifications for a sample of a real index, as JSON.

    The Rust side reads this back with `mio-cli classify-check`. This is the only
    way to keep the mirror honest, and the mirror is the only fast way to tune the
    vocabulary, so the loop is: change the dictionary, re-run this, re-run the
    Rust check, then measure with `tools/ucs-report.py`.
    """
    import argparse
    import json
    import random
    import sqlite3
    import sys

    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=r"D:\reslip-test-library\slip-sound.db")
    ap.add_argument("--sample", type=int, default=3000)
    ap.add_argument("--seed", type=int, default=11)
    ap.add_argument("--ucs", action="store_true",
                    help="include the complete UCS A vocabulary")
    args = ap.parse_args()

    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import taxonomy

    rows = sqlite3.connect(args.db).execute("SELECT path FROM sounds").fetchall()
    paths = [r[0] for r in rows]
    random.seed(args.seed)
    sample = random.sample(paths, min(args.sample, len(paths)))

    if args.ucs:
        cat_ids, keywords, phrases = load_ucs()
        clf = Classifier(taxonomy.TAXONOMY, cat_ids=cat_ids,
                         ucs_keywords=keywords, ucs_phrases=phrases)
        out = [[p, *clf.classify(p)[:2]] for p in sample]
    else:
        clf = Classifier(taxonomy.TAXONOMY)
        out = [[p, *clf.classify(p)[:2]] for p in sample]

    json.dump(out, sys.stdout, ensure_ascii=False)
    print(file=sys.stderr)
    print(f"wrote {len(out)} expectations to stdout", file=sys.stderr)


if __name__ == "__main__":
    main()
