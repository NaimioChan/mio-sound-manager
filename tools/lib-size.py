"""Read-only size breakdown of a library database (never writes)."""
import sqlite3
import sys

db = sys.argv[1]
conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)


def one(sql):
    return conn.execute(sql).fetchone()


def total_pages(table):
    """Bytes occupied by a table/index subtree, from dbstat when available."""
    try:
        return one(f"select sum(pgsize) from dbstat where name='{table}'")[0]
    except sqlite3.Error:
        return None


print("page_size", one("pragma page_size")[0])
print("page_count", one("pragma page_count")[0])
print("freelist", one("pragma freelist_count")[0])
for table in (
    "sounds",
    "sounds_fts_data",
    "sounds_fts_idx",
    "sounds_fts_docsize",
    "sounds_trigram_data",
    "sounds_trigram_idx",
    "sound_details",
    "covers",
):
    print(f"{table:22} {total_pages(table)}")

print("covers_bytes", one("select coalesce(sum(length(thumb)), 0) from covers")[0])
print("covers_max", one("select coalesce(max(length(thumb)), 0) from covers")[0])
print("covers_avg", one("select coalesce(avg(length(thumb)), 0) from covers")[0])
print("details_json_bytes", one("select coalesce(sum(length(tags_json)), 0) from sound_details")[0])
print("tags_text_bytes", one("select coalesce(sum(length(tags)), 0) from sounds")[0])
