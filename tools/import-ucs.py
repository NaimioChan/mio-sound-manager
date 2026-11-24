"""Turn a local UCS release into `tools/ucs_keywords.py`.

    python tools/import-ucs.py                       # reads docs/UCS Release
    python tools/import-ucs.py --release <folder>    # another unpacked release

UCS is not redistributed with this repository (the release folder is git-ignored).
What this script writes is a derived vocabulary file, committed, so a build never
depends on the download. Re-run it when a new UCS release arrives.

Three stages, and the third is the one that matters:

1. **Projection.** UCS's 82 categories and 753 entries are a sound librarian's
   index; this project's 22 are a producer's taxonomy. `tools/ucs_mapping.py` is
   the hand-authored map from one to the other.

2. **Filtering by shape.** A synonym reaches one of our targets only if exactly
   one wants it. Words claimed by two targets in our taxonomy are ambiguous *for
   us* even when UCS gives them one meaning, and a word in UCS's own "misc"
   bucket, a function word, or a production term (`designed`, `morph`,
   `recording`) is not a description of a sound. `DROP_SYNONYMS` lists those with
   the corpus count behind each decision.

3. **Filtering by measurement.** `tools/ucs-evidence.json` — produced by
   `tools/ucs-evidence.py` against the reference library — records, for every
   candidate, how many files carry it and how often the curated classifier
   already agrees with the mapped category on the files whose *filename* contains
   it. A word gets in only if the evidence agrees, or if there is too little
   evidence to disagree and the word is rare enough not to sweep a pack.

Without stage 3 the vocabulary looks fine and behaves badly: `one shot` alone
would move 15,816 files to Music, and `trance` (a UCS synonym for MAGIC-SPELL, a
genre in this library) would relabel a whole pack. The measurements are in
`tools/ucs-report.py`'s output and in `docs/index.md`.
"""

import argparse
import collections
import json
import os
import sys
import zipfile
import xml.etree.ElementTree as ET

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import ucs_mapping  # noqa: E402
import taxonomy as taxonomy_module  # noqa: E402

DEFAULT_RELEASE = os.path.join(os.path.dirname(HERE), "docs", "UCS Release")
FULL_LIST = "UCS v8.2.1 Full List.xlsx"
TOP_LIST = "UCS v8.2.1 Top Level Categories.xlsx"

# The Rust tokenizer's floor and stop list, mirrored so a word that could never
# fire is never written. Keep in sync with `classify::MIN_TOKEN_LEN` and
# `classify::STOP_WORDS`.
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

# The admission rule. `tools/ucs-report.py` measures what the result does to the
# corpus; these three constants are the whole rule.
PRECISION_CUT = 0.6      # agreement with the curated answer on files carrying the word
HITS_CUT = 10            # classified files needed before that agreement is evidence
FILES_CUT = 250          # above this, a word needs evidence, not silence
CATID_HITS_CUT = 5       # the same rule for CatID mappings
CATID_PRECISION_CUT = 0.6


def xlsx_rows(path):
    """Read the first worksheet with stdlib when openpyxl is unavailable."""
    ns = {"m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}
    with zipfile.ZipFile(path) as archive:
        shared = []
        if "xl/sharedStrings.xml" in archive.namelist():
            root = ET.fromstring(archive.read("xl/sharedStrings.xml"))
            for item in root.findall("m:si", ns):
                shared.append("".join(text.text or "" for text in item.iter("{http://schemas.openxmlformats.org/spreadsheetml/2006/main}t")))
        root = ET.fromstring(archive.read("xl/worksheets/sheet1.xml"))
        rows = []
        for row in root.findall(".//m:row", ns):
            cells = {}
            for cell in row.findall("m:c", ns):
                ref = cell.attrib.get("r", "A1")
                column = 0
                for char in ref:
                    if char.isalpha():
                        column = column * 26 + ord(char.upper()) - ord("A") + 1
                    else:
                        break
                value = cell.find("m:v", ns)
                text = "" if value is None else value.text or ""
                if cell.attrib.get("t") == "s" and text:
                    text = shared[int(text)]
                cells[column - 1] = text
            width = max(cells, default=-1) + 1
            rows.append([cells.get(i, "") for i in range(width)])
        return rows


def read_entries(path):
    try:
        import openpyxl
    except ModuleNotFoundError:
        rows = xlsx_rows(path)
    else:
        wb = openpyxl.load_workbook(path, data_only=True)
        ws = wb.worksheets[0]
        rows = [[None if c is None else str(c) for c in row] for row in ws.iter_rows(values_only=True)]
    header_at = next(i for i, r in enumerate(rows) if r and r[0] == "Category")
    entries = []
    for r in rows[header_at + 1:]:
        if not r or not (r[0] or "").strip():
            continue
        cat, sub, cat_id, _short, explanation, synonyms = (r + [None] * 6)[:6]
        entries.append({
            "category": (cat or "").strip(),
            "subcategory": (sub or "").strip(),
            "cat_id": (cat_id or "").strip(),
            "explanation": (explanation or "").strip(),
            "synonyms": [s.strip() for s in (synonyms or "").split(",") if s.strip()],
        })
    return entries


def read_top_explanations(path):
    try:
        import openpyxl
    except ModuleNotFoundError:
        rows = xlsx_rows(path)
    else:
        wb = openpyxl.load_workbook(path, data_only=True)
        ws = wb.worksheets[0]
        rows = [[None if c is None else str(c) for c in row] for row in ws.iter_rows(values_only=True)]
    return {r[0]: (r[1] or "") for r in rows[2:] if r and r[0]}


def target_for(entry):
    """Our (category, subcategory) for one UCS entry, or None to drop it."""
    key = (entry["category"], entry["subcategory"])
    if key in ucs_mapping.SUB:
        return ucs_mapping.SUB[key]
    return ucs_mapping.TOP.get(entry["category"])


def normalise(text):
    """Lowercase ASCII words, separators to spaces — the Rust tokenizer's view."""
    return " ".join(
        "".join(ch.lower() if ch.isascii() and ch.isalnum() else " " for ch in text).split()
    )


def curated_keyword_owners():
    owners = {}
    for cat, subs in taxonomy_module.TAXONOMY.items():
        for sub, (kws, _) in subs.items():
            target = (cat, None if sub == "__bare__" else sub)
            for kw in kws.split():
                owners.setdefault(kw, target)
    return owners


def load_evidence(path):
    if not os.path.exists(path):
        return None
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def word_is_admitted(evidence, curated_target):
    """Stage 3. `None` evidence means the file is missing, not 'no evidence'."""
    if evidence is None:
        return None
    if curated_target is not None:
        return False
    classified = evidence["same"] + evidence["other"]
    proven = classified >= HITS_CUT and (evidence["precision"] or 0) >= PRECISION_CUT
    if classified >= HITS_CUT and not proven:
        return False
    if evidence["files"] > FILES_CUT and not proven:
        return False
    return True


def build(entries, top_explanations, evidence, catid_evidence):
    curated = curated_keyword_owners()
    excluded = collections.Counter()
    examples = collections.defaultdict(list)

    def exclude(word, reason):
        excluded[reason] += 1
        if len(examples[reason]) < 8:
            examples[reason].append(word)

    # Collect every claim first: a synonym reaches the dictionary only if exactly
    # one of our targets wants it.
    claims = collections.defaultdict(set)
    for entry in entries:
        target = target_for(entry)
        if target is None:
            continue
        for raw in entry["synonyms"]:
            if not raw.isascii():
                continue                      # mojibake in the source list
            word = normalise(raw)
            if word:
                claims[word].add(target)

    keywords = collections.defaultdict(set)
    phrases = collections.defaultdict(set)
    unmeasured = []
    for word, targets in sorted(claims.items()):
        if len(targets) > 1:
            exclude(word, "ambiguous across our categories")
            continue
        (target,) = tuple(targets)
        if word in curated:
            if curated[word] != target:
                exclude(word, "curated keyword owns it")
            continue

        if word in ucs_mapping.DROP_SYNONYMS:
            exclude(word, "listed in DROP_SYNONYMS")
            continue
        if word in ucs_mapping.DROP_PHRASES:
            exclude(word, "listed in DROP_PHRASES")
            continue
        if " " in word:
            phrases[target].add(word)
            continue
        if len(word) < MIN_TOKEN_LEN:
            exclude(word, "shorter than the tokenizer floor")
            continue
        if word in STOP_WORDS:
            exclude(word, "stop word")
            continue
        verdict = word_is_admitted(evidence.get(word), None)
        if verdict is None:
            unmeasured.append(word)
            keywords[target].add(word)
        elif verdict:
            keywords[target].add(word)
        else:
            classified = evidence[word]["same"] + evidence[word]["other"]
            if classified >= HITS_CUT:
                exclude(word, "corpus disagrees with the mapping")
            else:
                exclude(word, "sweeps a pack without evidence")

    if evidence is None:
        print("WARNING: no tools/ucs-evidence.json — every word that stages 1 and 2"
              " allowed is being written. Run tools/ucs-evidence.py and re-run this"
              " script before committing.", file=sys.stderr)

    # CatIDs: declared categories, resolved through the same mapping.
    cat_ids = {}
    native_cat_ids = {
        entry["cat_id"]: (entry["category"], entry["subcategory"] or None)
        for entry in entries
        if entry["cat_id"] and entry["cat_id"].isascii()
    }
    for entry in entries:
        target = target_for(entry)
        if target is None:
            continue
        cid = entry["cat_id"]
        if not cid or not cid.isascii():
            continue
        if catid_evidence is not None:
            e = catid_evidence.get(cid) or {}
            classified = (e.get("same") or 0) + (e.get("other") or 0)
            if classified >= CATID_HITS_CUT and (e.get("precision") or 0) < CATID_PRECISION_CUT:
                exclude(cid, "CatID mapping contradicted by the corpus")
                continue
        cat_ids.setdefault(cid, target)

    top_cats = collections.OrderedDict()
    for entry in entries:
        if entry["category"] not in top_cats:
            top_cats[entry["category"]] = top_explanations.get(entry["category"], "")

    return {
        "keywords": keywords,
        "phrases": phrases,
        "cat_ids": cat_ids,
        "native_cat_ids": native_cat_ids,
        "excluded": excluded,
        "examples": examples,
        "top_cats": top_cats,
        "unmeasured": unmeasured,
    }


def render(result, measured):
    out = []
    out.append('"""UCS 8.2.1 vocabulary, projected onto this project\'s taxonomy.\n')
    out.append("")
    out.append("Generated by `tools/import-ucs.py` — do not hand-edit. The source is a")
    out.append("UCS release unpacked into `docs/UCS Release/` (git-ignored, never")
    out.append("redistributed). UCS itself is a public-domain initiative")
    out.append("<https://www.universalcategorysystem.com/>; what is committed here is the")
    out.append("derived word list, which the generator writes and `tools/gen-category-dict.py`")
    out.append("turns into `crates/mio-index/src/classify/ucs.rs`.")
    out.append("")
    out.append("Every word survived three filters: the projection in `tools/ucs_mapping.py`,")
    out.append("the shape checks in the generator, and the corpus measurement in")
    out.append("`tools/ucs-evidence.json` — a word is kept only when the curated classifier")
    out.append("already agrees with its mapped category, or when there is too little")
    out.append("evidence to disagree.")
    out.append("")
    out.append("Shape: {target: [words]} where target is")
    out.append("`(category, subcategory-or-None)`.")
    out.append('"""')
    out.append("")
    out.append('UCS_VERSION = "8.2.1"')
    out.append("")
    out.append("# Whether stage 3 had corpus measurements to work from. `False` means the")
    out.append("# file was generated without evidence and must not be committed.")
    out.append(f"MEASURED = {measured!r}")
    out.append("")
    out.append("# UCS top-level category -> the sentence UCS uses to describe it. Kept")
    out.append("# because the projection's decisions read better next to the intent.")
    out.append("TOP_CATEGORY_NOTES = {")
    for cat, note in result["top_cats"].items():
        escaped = note.replace("\\", "\\\\").replace('"', '\\"')
        out.append(f'    "{cat}": "{escaped}",')
    out.append("}")
    out.append("")

    def block(name, mapping, comment):
        out.append(comment)
        out.append(f"{name} = {{")
        for target in sorted(mapping, key=lambda t: (t[0], t[1] or "")):
            items = sorted(mapping[target])
            if not items:
                continue
            out.append(f"    {target!r}: [")
            line = "       "
            for item in items:
                piece = f" {item!r},"
                if len(line) + len(piece) > 96:
                    out.append(line)
                    line = "       "
                line += piece
            if line.strip():
                out.append(line)
            out.append("    ],")
        out.append("}")
        out.append("")

    block("KEYWORDS", result["keywords"],
          "# One word per class, matched the way the curated dictionary is matched.")
    block("PHRASES", result["phrases"],
          "# Multi-word terms, matched against the normalised path.")
    out.append("# UCS CatID -> target. A filename that opens with one of these has")
    out.append("# declared its category; see `classify::cat_id_prefix`.")
    out.append("CAT_IDS = {")
    for cid in sorted(result["cat_ids"]):
        cat, sub = result["cat_ids"][cid]
        out.append(f"    {cid!r}: ({cat!r}, {sub!r}),")
    out.append("}")
    out.append("")
    out.append("# UCS CatID -> the native UCS two-level label, retained for A-lite schema storage.")
    out.append("CAT_IDS_NATIVE = {")
    for cid in sorted(result["native_cat_ids"]):
        cat, sub = result["native_cat_ids"][cid]
        out.append(f"    {cid!r}: ({cat!r}, {sub!r}),")
    out.append("}")
    out.append("")
    return "\n".join(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--release", default=DEFAULT_RELEASE)
    ap.add_argument("--out", default=os.path.join(HERE, "ucs_keywords.py"))
    ap.add_argument("--evidence", default=os.path.join(HERE, "ucs-evidence.json"))
    ap.add_argument("--catid-evidence", default=os.path.join(HERE, "ucs-catid-evidence.json"))
    args = ap.parse_args()

    entries = read_entries(os.path.join(args.release, FULL_LIST))
    tops = read_top_explanations(os.path.join(args.release, TOP_LIST))
    evidence = load_evidence(args.evidence)
    catid_evidence = load_evidence(args.catid_evidence)
    print(f"read {len(entries)} UCS entries, {len(tops)} top categories")
    print(f"evidence: {'yes' if evidence else 'MISSING'}"
          f" ({len(evidence) if evidence else 0} word measurements,"
          f" {len(catid_evidence) if catid_evidence else 0} CatID measurements)")

    result = build(entries, tops, evidence, catid_evidence)

    mapped = sum(1 for e in entries if target_for(e) is not None)
    print(f"entries with a target in our taxonomy: {mapped} / {len(entries)}")
    print(f"keywords {sum(len(v) for v in result['keywords'].values())}, "
          f"phrases {sum(len(v) for v in result['phrases'].values())}, "
          f"CatIDs {len(result['cat_ids'])}")
    print("\nnot carried over:")
    for reason, count in result["excluded"].most_common():
        print(f"  {count:5d}  {reason}   e.g. {', '.join(result['examples'][reason])}")

    with open(args.out, "w", encoding="utf-8", newline="\n") as f:
        f.write(render(result, evidence is not None))
    print(f"\nwrote {args.out}")
    print("next: python tools/gen-category-dict.py && cargo test -p mio-index")


if __name__ == "__main__":
    main()
