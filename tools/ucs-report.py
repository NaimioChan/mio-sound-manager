"""Report what the shipped UCS fallback does to the reference corpus.

    python tools/ucs-report.py                    # the numbers in docs/index.md
    python tools/ucs-report.py --db <index.db>    # another library

Two runs over the same paths: the classifier with the UCS tables, and the same
classifier built from the curated dictionary alone. The difference is what UCS
contributes — how many files gain a category, and how many already-classified
files change. The second number must be 0: the fallback pass only runs when the
curated pass scored nothing, so a non-zero count means the structure broke.

Uses `tools/classify-mirror.py`, which mirrors the Rust scoring. Run
`mio-cli classify-check` first if the mirror has been touched; the numbers here
are only worth reading while that check reports 0 mismatches.
"""

import argparse
import collections
import importlib.util
import os
import sqlite3
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import taxonomy  # noqa: E402


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, filename))
    if spec is None or spec.loader is None:
        raise ImportError(filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=r"D:\reslip-test-library\slip-sound.db")
    ap.add_argument("--examples", type=int, default=10)
    args = ap.parse_args()

    mirror = load("classify_mirror", "classify-mirror.py")
    cat_ids, keywords, phrases = mirror.load_ucs()

    curated = mirror.Classifier(taxonomy.TAXONOMY)
    shipped = mirror.Classifier(taxonomy.TAXONOMY, cat_ids=cat_ids,
                                ucs_keywords=keywords, ucs_phrases=phrases)

    rows = sqlite3.connect(args.db).execute(
        "SELECT path, category FROM sounds").fetchall()
    total = len(rows)
    before = collections.Counter()
    after = collections.Counter()
    gained = collections.Counter()
    regressions = []
    examples = []

    for path, category in rows:
        a = curated.classify(path)
        b = shipped.classify(path)
        before[a[0]] += 1
        after[b[0]] += 1
        if a[0] == b[0]:
            continue
        if a[0] == "Uncategorized":
            gained[b[0]] += 1
            if len(examples) < args.examples:
                examples.append((path.rsplit("\\", 1)[-1], b[0], b[1], b[3]))
        else:
            regressions.append((path, a[0], b[0]))

    freed = before["Uncategorized"] - after["Uncategorized"]
    print(f"corpus {total} paths from {args.db}")
    print(f"  uncategorized   {before['Uncategorized']} "
          f"({before['Uncategorized']/total:.2%}) -> {after['Uncategorized']} "
          f"({after['Uncategorized']/total:.2%})")
    print(f"  gained          {freed}")
    print(f"  regressions     {len(regressions)}   "
          f"(must be 0: the fallback only runs when the curated pass scored nothing)")
    print(f"  ucs vocabulary  {sum(len(v) for v in keywords.values())} words, "
          f"{sum(len(v) for v in phrases.values())} phrases, {len(cat_ids)} CatIDs")

    if gained:
        print("\ngains by category:")
        for name, count in gained.most_common():
            print(f"  {name:22s} +{count}")
    if examples:
        print("\nexamples:")
        for name, category, subcategory, raw in examples:
            print(f"  {name[:66]:68s} -> {category} / {subcategory} (raw {raw})")
    if regressions:
        print("\nREGRESSIONS (a UCS decision overrode the curated dictionary):")
        for path, was, now in regressions[:20]:
            print(f"  {was} -> {now}   {path}")

    return 0 if not regressions else 1


if __name__ == "__main__":
    raise SystemExit(main())
