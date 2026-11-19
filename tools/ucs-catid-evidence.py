"""Evidence for UCS CatIDs, measured the same way as the synonyms.

A CatID is a stronger signal than a word: a filename that opens with `GORESrce_`
is declaring its category, not suggesting one. But the *mapping* from that UCS
entry to one of our 22 categories can still be wrong, so it gets the same test —
on the files that carry the CatID, how often does the curated classifier already
agree with the mapping?

Run after `ucs-evidence.py`; writes `tools/ucs-catid-evidence.json`.
"""

import collections
import json
import os
import re
import sqlite3
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

DB = r"D:\reslip-test-library\slip-sound.db"


def main():
    import ucs_keywords as ucs

    cat_ids = {k: tuple(v) for k, v in ucs.CAT_IDS.items()}
    rows = sqlite3.connect(DB).execute("SELECT path, category FROM sounds").fetchall()
    print(f"corpus {len(rows)} rows, {len(cat_ids)} CatIDs")

    files = collections.Counter()
    packs = collections.defaultdict(set)
    same = collections.Counter()
    other = collections.Counter()
    silent = collections.Counter()
    confusion = collections.defaultdict(collections.Counter)

    for path, category in rows:
        parts = path.replace("/", "\\").split("\\")
        filename = parts[-1]
        pack = parts[3] if len(parts) > 4 else "?"
        stem = os.path.splitext(filename)[0]
        head = re.split(r"[_\-\s.]", stem, maxsplit=1)[0]
        target = cat_ids.get(head)
        if target is None:
            continue
        files[head] += 1
        packs[head].add(pack)
        if category == target[0]:
            same[head] += 1
        elif category == "Uncategorized":
            silent[head] += 1
        else:
            other[head] += 1
            confusion[head][category] += 1

    evidence = {}
    for cid, target in cat_ids.items():
        classified = same[cid] + other[cid]
        evidence[cid] = {
            "target": list(target),
            "files": files[cid],
            "packs": len(packs[cid]),
            "same": same[cid],
            "other": other[cid],
            "silent": silent[cid],
            "precision": round(same[cid] / classified, 3) if classified else None,
            "confusion": dict(confusion[cid].most_common(3)),
        }
    out = os.path.join(HERE, "ucs-catid-evidence.json")
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        json.dump(evidence, f, indent=0, sort_keys=True)
    print(f"wrote {out}")

    used = {k: v for k, v in evidence.items() if v["files"]}
    print(f"\nCatIDs that occur in the corpus: {len(used)} of {len(cat_ids)}")
    print(f"{'CatID':11s} {'files':>6s} {'packs':>5s} {'same':>5s} {'other':>5s} {'silent':>6s} {'prec':>5s}  target / confusion")
    for cid, e in sorted(used.items(), key=lambda kv: -kv[1]["files"]):
        p = e["precision"]
        print(f"{cid:11s} {e['files']:6d} {e['packs']:5d} {e['same']:5d} {e['other']:5d} "
              f"{e['silent']:6d} {('%.2f' % p) if p is not None else '   -':>5s}  "
              f"{tuple(e['target'])} {e['confusion'] if e['confusion'] else ''}")

    bad = [(c, e) for c, e in used.items()
           if e["same"] + e["other"] >= 5 and (e["precision"] or 0) < 0.6]
    print(f"\nCatIDs the curated classifier contradicts (>=5 classified, precision < 0.6): {len(bad)}")
    for cid, e in sorted(bad, key=lambda kv: -kv[1]["files"]):
        print(f"  {cid:11s} {e['files']:5d}f prec {e['precision']} -> {tuple(e['target'])} "
              f"{e['confusion']}")


if __name__ == "__main__":
    main()
