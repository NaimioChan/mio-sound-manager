"""Measure every UCS synonym against the reference corpus, for the import filter.

    python tools/ucs-evidence.py [--db <index.db>] [--paths <file>]

Writes `tools/ucs-evidence.json`: for each candidate word, how many files and
packs contain it, and — on the files whose *filename* contains it, which the
curated dictionary already classified — how often the curated answer agrees with
the UCS target. The import script applies the rule; this file is the audit trail
so a later revision can see the evidence without re-scanning the library.

Two measurements, two different failures:

`packs` catches pack-theme words. A word in 5,000 files spread over 4 packs is
describing the pack, not the sound, and would relabel a whole vendor folder —
the same failure the curated dictionary documents for `epic`.

`same`/`other` catches wrong mappings. A thesaurus entry is one word with
several meanings; `trance` is a UCS synonym for MAGIC-SPELL and a music genre in
this library. Files the curated classifier already places elsewhere are the
evidence that the mapping is wrong.
"""

import argparse
import collections
import json
import os
import sqlite3
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import taxonomy  # noqa: E402

# The Rust tokenizer, mirrored: `classify::MIN_TOKEN_LEN` and `classify::STOP_WORDS`.
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


def tokenize(text):
    words = "".join(c.lower() if c.isascii() and c.isalnum() else " " for c in text).split()
    return [w for w in words if len(w) >= MIN_TOKEN_LEN and w not in STOP_WORDS]


def pack_of(path):
    """The pack folder: the directory under Bought|UnBought\\SFX|Sample."""
    parts = path.replace("/", "\\").split("\\")
    for i, part in enumerate(parts):
        if part in ("Bought", "UnBought") and i + 2 < len(parts):
            return part + "/" + parts[i + 2]
    return parts[-2] if len(parts) > 1 else "?"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=r"D:\reslip-test-library\slip-sound.db")
    ap.add_argument("--out", default=os.path.join(HERE, "ucs-evidence.json"))
    args = ap.parse_args()

    import ucs_keywords as ucs

    curated = {}
    for cat, subs in taxonomy.TAXONOMY.items():
        for sub, (kws, _) in subs.items():
            for w in kws.split():
                curated.setdefault(w, (cat, None if sub == "__bare__" else sub))

    candidates = {}
    for target, words in ucs.KEYWORDS.items():
        for w in words:
            if w not in curated:
                candidates[w] = tuple(target)

    conn = sqlite3.connect(args.db)
    rows = conn.execute("SELECT path, category FROM sounds").fetchall()
    print(f"corpus {len(rows)} rows, {len(candidates)} candidate words")

    files = collections.Counter()
    packs = collections.defaultdict(set)
    same = collections.Counter()
    other = collections.Counter()
    silent = collections.Counter()
    confusion = collections.defaultdict(collections.Counter)

    for path, category in rows:
        normalised = path.replace("/", "\\")
        cut = normalised.rfind("\\")
        filename = normalised[cut + 1:] if cut >= 0 else normalised
        pack = pack_of(normalised)
        whole = set(tokenize(normalised))
        for tok in list(whole) + [stem(t) for t in whole]:
            if tok in candidates:
                files[tok] += 1
                packs[tok].add(pack)
        # Agreement is measured on the filename only: a directory word is a pack
        # theme, and the curated classifier already discounts it.
        head = os.path.splitext(filename)[0]
        for tok in set(tokenize(head)):
            for cand in (tok, stem(tok)):
                if cand not in candidates:
                    continue
                if category == candidates[cand][0]:
                    same[cand] += 1
                elif category == "Uncategorized":
                    silent[cand] += 1
                else:
                    other[cand] += 1
                    confusion[cand][category] += 1
                break

    evidence = {}
    for w, target in candidates.items():
        classified = same[w] + other[w]
        evidence[w] = {
            "target": list(target),
            "files": files[w],
            "packs": len(packs[w]),
            "same": same[w],
            "other": other[w],
            "silent": silent[w],
            "precision": round(same[w] / classified, 3) if classified else None,
            "confusion": dict(confusion[w].most_common(3)),
        }

    with open(args.out, "w", encoding="utf-8", newline="\n") as f:
        json.dump(evidence, f, indent=0, sort_keys=True)
    print(f"wrote {args.out}")

    # A short shape report, so the thresholds in `import-ucs.py` can be seen to
    # sit where the data has a knee rather than being picked out of the air.
    for min_hits in (5, 10, 20):
        band = [e for e in evidence.values() if e["same"] + e["other"] >= min_hits]
        for cut in (0.3, 0.5, 0.7, 0.9):
            good = sum(1 for e in band if (e["precision"] or 0) >= cut)
            print(f"  >= {min_hits:2d} classified hits: {good:4d} of {len(band):4d} words "
                  f"at precision >= {cut}")
    spread = sorted((e["files"] / max(e["packs"], 1), w) for w, e in evidence.items()
                    if e["packs"] >= 2)
    print(f"\nfiles-per-pack median {spread[len(spread)//2][0]:.0f}, "
          f"90th percentile {spread[int(len(spread)*0.9)][0]:.0f}, "
          f"worst {spread[-1][0]:.0f} ({spread[-1][1]})")


if __name__ == "__main__":
    main()
