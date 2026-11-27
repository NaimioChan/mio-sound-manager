"""Emit the Rust dictionary + UCS vocabulary from the tuned Python taxonomy.

Two generated files under `crates/mio-index/src/classify/`:

- `dictionary.rs` — the curated vocabulary (`tools/taxonomy.py`), the one that
  decides every file it can.
- `ucs.rs` — the UCS vocabulary (`tools/ucs_keywords.py`), consulted only when
  the curated pass returns `Uncategorized`.

Rust gets generated files so the two sides can never drift, and this script is
the only writer of either.

    python tools/gen-category-dict.py            # both files, in place
    python tools/gen-category-dict.py --stdout   # dictionary.rs to stdout
"""

import argparse
import collections
import json
import os
import sys

W = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, W)
import taxonomy

SRC = os.path.join(os.path.dirname(W), "crates", "mio-index", "src", "classify")

DICTIONARY_HEADER = '''//! Category taxonomy: category → subcategory → the words that select it.
//!
//! Authored for this project against a real 271k-file sample library, not
//! transcribed from another implementation. The vocabulary reflects what that
//! library actually contains and how it names things: pack folder names are
//! marketing (`Epic.Stock.Media.Hybrid.Game.Sounds`) and are ignored by the
//! matcher, filenames are the description (`ESM_SPG_fx_servo_metal_latch_02`),
//! and the Epic Stock Media packs carry a Chinese folder name that states the
//! pack theme, which is deliberately not read: see `tools/taxonomy.py`.
//!
//! Do not hand-edit: `tools/gen-category-dict.py` generates this file. The
//! vocabulary, the weights and the tokenizer are tuned together, so a word
//! added here without re-running the corpus check is a guess.
//!
//! `__bare__` is the category-level entry: the words that name the category
//! itself (`drum`, `foley`, `weapon`) rather than one of its subcategories. A
//! subcategory hit outranks a bare-category hit on a tie, so the bare entry only
//! decides files that carry no more specific word.

use super::CategoryDefinition;

pub static CATEGORY_DICTIONARY: &[CategoryDefinition] = &[
'''

UCS_HEADER = '''//! UCS 8.2.1 vocabulary, projected onto this project's 22 categories.
//!
//! Do not hand-edit: `tools/import-ucs.py` writes `tools/ucs_keywords.py` from a
//! UCS release and `tools/gen-category-dict.py` generates this file from it.
//!
//! The Universal Category System (<https://www.universalcategorysystem.com/>) is
//! a public-domain initiative by Tim Nielsen, Justin Drury, Kai Paquin and
//! others. Its release is not redistributed with this project — only the derived
//! word list below, which this project's own measurement tooling filtered.
//!
//! **Every word here is a fallback.** `Classifier::classify` consults this table
//! only when the curated dictionary returned `Uncategorized`, so nothing here can
//! move a file the curated vocabulary already decided. That is why the weights
//! are lower than the curated ones: they only ever compete with each other.
//!
//! ## How entries got in
//!
//! UCS is a sound librarian's index (753 Category/SubCategory entries); this
//! project's 22 categories are a producer's taxonomy. `tools/ucs_mapping.py`
//! holds the hand-authored projection. A UCS synonym then had to survive a
//! corpus check (`tools/ucs-evidence.py`): on the files whose *filename* contains
//! it, the curated classifier must already agree with the mapped category at
//! >= 0.6 precision over >= 10 files, or the word must be rare enough
//! (<= 250 files) that no evidence either way exists.
//!
//! The words this excludes are the point of the exercise. `trance` is a UCS
//! synonym for MAGIC-SPELL and a genre in this library (46 files say Drums, 9 say
//! Magic). `hybrid` is a UCS synonym for VEHICLES-CAR, and in this library it is
//! a pack name: 12,454 files across 19 packs. `dark`, `organic`, `acid`, `soul`,
//! `calendar`, `steampunk` — all dropped for the same reason, and each with its
//! counts in `tools/ucs-evidence.json`.

use super::CategoryDefinition;

/// UCS-derived words, in the same shape as [`super::CATEGORY_DICTIONARY`].
pub static UCS_DICTIONARY: &[CategoryDefinition] = &[
'''


def rust_string(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def rust_list(items, indent):
    if not items:
        return "&[]"
    pad = " " * indent
    lines, line = [], ""
    for item in items:
        piece = f"{rust_string(item)}, "
        if len(pad) + len(line) + len(piece) > 96:
            lines.append(pad + line.rstrip())
            line = ""
        line += piece
    if line:
        lines.append(pad + line.rstrip())
    return "&[\n" + "\n".join(lines) + "\n" + " " * (indent - 4) + "]"


def emit_entries(entries):
    """One `CategoryDefinition` block per (category, subcategory) pair.

    Order is preserved as given. The curated file is checked into git, so
    sorting here would rewrite 451 lines of a file nobody changed; the UCS file
    is generated from a dict and gets sorted at the call site instead.
    """
    out = []
    keywords = phrases = 0
    for (cat, sub), (kws, phs) in entries:
        sub_expr = "None" if sub is None else f"Some({rust_string(sub)})"
        keywords += len(kws)
        phrases += len(phs)
        out.append(
            "    CategoryDefinition {\n"
            f"        category: {rust_string(cat)},\n"
            f"        subcategory: {sub_expr},\n"
            f"        keywords: {rust_list(list(kws), 12)},\n"
            f"        phrases: {rust_list(list(phs), 12)},\n"
            "    },\n"
        )
    return out, keywords, phrases


def curated_entries():
    for cat, subs in taxonomy.TAXONOMY.items():
        for sub, (kws, phs) in subs.items():
            yield ((cat, None if sub == "__bare__" else sub),
                   (kws.split(), [p.strip() for p in phs.split("|") if p.strip()]))


def load_ucs():
    import importlib.util

    path = os.path.join(W, "ucs_keywords.py")
    if not os.path.exists(path):
        return None
    spec = importlib.util.spec_from_file_location("ucs_keywords", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def ucs_entries(module):
    """Fold KEYWORDS and PHRASES into one (category, subcategory) list."""
    merged = collections.defaultdict(lambda: (set(), set()))
    for (cat, sub), words in module.KEYWORDS.items():
        merged[(cat, sub)][0].update(words)
    for (cat, sub), phrases in module.PHRASES.items():
        merged[(cat, sub)][1].update(phrases)
    order = {name: i for i, name in enumerate(taxonomy.TAXONOMY)}
    # Sorted because the input is a dict: an unsorted write would produce a
    # different file on every run under a different hash seed.
    return sorted(
        ((k, (sorted(v[0]), sorted(v[1]))) for k, v in merged.items()),
        key=lambda kv: (order.get(kv[0][0], 99), kv[0][1] or ""),
    )


def emit_cat_ids(module):
    out = ["\n/// UCS CatID → projected target. A filename that opens with one of these has declared\n"
           "/// its category; matched case-sensitively, because UCS writes CatIDs in\n"
           "/// CamelCase and a lowercase match would let the ordinary word `rain` claim\n"
           "/// `RAIN`.\n",
           "pub static UCS_CAT_IDS: &[(&str, &str, Option<&str>)] = &[\n"]
    for cid in sorted(module.CAT_IDS):
        cat, sub = module.CAT_IDS[cid]
        sub_expr = "None" if sub is None else f"Some({rust_string(sub)})"
        out.append(f"    ({rust_string(cid)}, {rust_string(cat)}, {sub_expr}),\n")
    out.append("];\n\n")
    out.append("/// UCS CatID → native UCS category and subcategory, for A-lite storage.\n")
    out.append("pub static UCS_CAT_ID_NATIVE: &[(&str, &str, Option<&str>)] = &[\n")
    for cid in sorted(module.CAT_IDS_NATIVE):
        category, subcategory = module.CAT_IDS_NATIVE[cid]
        sub_expr = "None" if subcategory is None else f"Some({rust_string(subcategory)})"
        out.append(f"    ({rust_string(cid)}, {rust_string(category)}, {sub_expr}),\n")
    out.append("];\n\n")
    out.append("/// UCS CatIDs that are deliberately not projected. A filename that opens\n")
    out.append("/// with one carries a native UCS label but declares no category here, so the\n")
    out.append("/// classifier must not let it outrank keyword evidence.\n")
    out.append("pub static UCS_CAT_ID_DROPPED: &[(&str, &str)] = &[\n")
    for cid in sorted(module.DROPPED_CAT_IDS):
        out.append(f"    ({rust_string(cid)}, {rust_string(module.DROPPED_CAT_IDS[cid])}),\n")
    out.append("];\n\n")
    out.append(f"/// The UCS release this vocabulary was derived from.\n"
               f"pub const UCS_VERSION: &str = {rust_string(module.UCS_VERSION)};\n")
    out.append("\n/// Unique UCS keyword -> official native category and subcategory.\n")
    out.append("pub static UCS_NATIVE_KEYWORDS: &[(&str, &str, Option<&str>)] = &[\n")
    for word in sorted(module.NATIVE_KEYWORDS):
        category, subcategory = module.NATIVE_KEYWORDS[word]
        sub = "None" if subcategory is None else f"Some({rust_string(subcategory)})"
        out.append(f"    ({rust_string(word)}, {rust_string(category)}, {sub}),\n")
    out.append("];\n")
    out.append("\n/// Unique UCS phrase -> official native category and subcategory.\n")
    out.append("pub static UCS_NATIVE_PHRASES: &[(&str, &str, Option<&str>)] = &[\n")
    for phrase in sorted(module.NATIVE_PHRASES):
        category, subcategory = module.NATIVE_PHRASES[phrase]
        sub = "None" if subcategory is None else f"Some({rust_string(subcategory)})"
        out.append(f"    ({rust_string(phrase)}, {rust_string(category)}, {sub}),\n")
    out.append("];\n")
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--stdout", action="store_true",
                    help="print dictionary.rs instead of writing both files")
    args = ap.parse_args()

    blocks, keywords, phrases = emit_entries(list(curated_entries()))
    dictionary = DICTIONARY_HEADER + "".join(blocks) + "];\n"

    if args.stdout:
        sys.stdout.write(dictionary)
        print(f"// entries {len(blocks)}, keywords {keywords}, phrases {phrases}",
              file=sys.stderr)
        return

    module = load_ucs()
    if module is None:
        raise SystemExit("tools/ucs_keywords.py is missing; run tools/import-ucs.py first")
    entries = ucs_entries(module)
    ucs_blocks, ucs_keywords, ucs_phrases = emit_entries(entries)
    ucs = UCS_HEADER + "".join(ucs_blocks) + "];\n" + "".join(emit_cat_ids(module))

    for name, text in (("dictionary.rs", dictionary), ("ucs.rs", ucs)):
        path = os.path.join(SRC, name)
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        print(f"wrote {path}")

    print(f"curated  entries {len(blocks):3d}  keywords {keywords:5d}  phrases {phrases:3d}")
    print(f"ucs      entries {len(ucs_blocks):3d}  keywords {ucs_keywords:5d}  "
          f"phrases {ucs_phrases:3d}  catids {len(module.CAT_IDS):3d}")


if __name__ == "__main__":
    main()
