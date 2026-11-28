"""Read-only probe: real tag values from the copy, for a live FTS check."""
import sqlite3
import sys

conn = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
q = lambda sql: conn.execute(sql).fetchall()

# 33 distinct cover hashes, the size of one viewport of covered rows.
hashes = [r[0] for r in q(
    "select distinct cover_hash from sounds where cover_hash is not null limit 33"
)]
print("HASHES", ",".join(hashes))

# A few searchable tag values that must be findable through sounds_fts.tags.
rows = q("""
    select s.id, s.tags, d.tags_json
    from sounds s join sound_details d on d.sound_id = s.id
    where s.tags is not null and length(s.tags) > 0
    limit 3
""")
for sound_id, tags, tags_json in rows:
    first = tags.split(" ")[0]
    print("TAGVALUE", first, "|", tags[:80].replace("\n", " "))
