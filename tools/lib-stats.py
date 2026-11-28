"""Read-only statistics on a library copy (never writes)."""
import sqlite3
import sys

db = sys.argv[1]
conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
q = lambda sql: conn.execute(sql).fetchone()


def has_column(table, name):
    return q(f"select count(*) from pragma_table_info('{table}') where name='{name}'")[0] > 0


print("rows", q("select count(*) from sounds")[0])
print("schema", q("select value from meta where key='schema_version'"))
print("fts", q("select value from meta where key='fts_version'"))
print("details_rule", q("select value from meta where key='details_rule'"))
if has_column("sounds", "cover_hash"):
    print("with_cover", q("select count(*) from sounds where cover_hash is not null")[0])
    print("details_indexed", q("select count(*) from sounds where details_indexed=1")[0])
    print("distinct_covers", q("select count(distinct cover_hash) from sounds where cover_hash is not null")[0])
print("metadata_indexed", q("select count(*) from sounds where metadata_indexed=1")[0])
for table in ("covers", "sound_details"):
    try:
        print(table, q(f"select count(*) from {table}")[0])
    except sqlite3.Error:
        print(table, "missing")
