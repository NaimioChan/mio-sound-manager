"""Emit the Rust dictionary + classifier constants from the tuned Python taxonomy.

The Python side is where the taxonomy is authored and measured against the real
library (see emulate3.py / why.py in this directory). Rust gets a generated file
so the two can never drift, and this script is the only writer of it.

    python emit_rust.py > ../../../projects/reslip/crates/reslip-index/src/classify/dictionary.rs
"""

import os
import sys

W = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, W)
import taxonomy

HEADER = '''//! Category taxonomy: category → subcategory → the words that select it.
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


def rust_string(s):
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"') + '"'


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


def main():
    out = [HEADER]
    keywords = phrases = 0
    for cat, subs in taxonomy.TAXONOMY.items():
        for sub, (kws, phs) in subs.items():
            sub_expr = "None" if sub == "__bare__" else f"Some({rust_string(sub)})"
            kw = kws.split()
            ph = [p.strip() for p in phs.split("|") if p.strip()]
            keywords += len(kw)
            phrases += len(ph)
            out.append(
                "    CategoryDefinition {\n"
                f"        category: {rust_string(cat)},\n"
                f"        subcategory: {sub_expr},\n"
                f"        keywords: {rust_list(kw, 12)},\n"

                f"        phrases: {rust_list(ph, 12)},\n"
                "    },\n"
            )
    out.append("];\n")
    sys.stdout.write("".join(out))
    print(
        f"// entries {sum(len(v) for v in taxonomy.TAXONOMY.values())}, "
        f"keywords {keywords}, phrases {phrases}",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
