//! SQLite schema, upsert, prune and search.
//!
//! The schema is the reference implementation's, plus two columns
//! (`mtime_ns`, `indexed_at`) and a `meta` table. Keeping the table and column
//! names identical is deliberate: an existing Slip Sound index can be opened by
//! this crate, migrated in place, and diffed row by row to prove the Rust
//! classifier agrees with the TypeScript one.
//!
//! Three invariants that are easy to break and expensive to debug:
//!
//! * **The upsert never touches user state.** `favorite` is not in the UPDATE
//!   clause at all, and the classifier columns are only refreshed when
//!   `category_manual = 0`. A reindex must not undo what the user decided.
//! * **FTS is kept in sync by triggers, not by the writer.** Anything that
//!   inserts, updates or deletes a `sounds` row updates `sounds_fts`. That
//!   includes the prune path.
//! * **WAL is mandatory.** The UI queries while the indexer writes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::error::{IndexError, Result};
use crate::folders::{FolderNode, build_folder_tree, folder_range, relative_folder};

/// Current schema version, stored in `meta`. Bump when a migration is added.
///
/// 2 added `bpm`, read from the file name when the indexer sees the file.
pub const SCHEMA_VERSION: i64 = 2;

/// File name of the index inside a library folder.
///
/// Was `slip-sound.db` while this project shared the reference implementation's
/// database; the name is ours now. [`LEGACY_DB_FILENAME`] is the one an existing
/// library still has, and [`IndexDb::open`] renames it in place rather than
/// leaving a user with an empty index and a reindex.
pub const DB_FILENAME: &str = "mio-sm.db";

/// The name earlier builds wrote. Only read, never written.
pub const LEGACY_DB_FILENAME: &str = "slip-sound.db";

/// One indexed file, ready to be written.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundRow {
    pub path: String,
    pub filename: String,
    pub duration_s: Option<f64>,
    pub channels: Option<u32>,
    pub sample_rate: Option<u32>,
    pub filesize: u64,
    pub category: String,
    pub subcategory: Option<String>,
    pub confidence: f64,
    pub matched_terms: Vec<String>,
    /// Folder relative to the library root, `""` at the root.
    pub folder: String,
    pub mtime_ns: i64,
    /// Tempo stated in the file name, or `None` when it states none. See
    /// [`crate::bpm`]: this is never guessed from the audio.
    pub bpm: Option<f64>,
}

/// A row as read back, including the user-editable state.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    pub id: i64,
    pub path: String,
    pub filename: String,
    pub duration_s: Option<f64>,
    pub channels: Option<u32>,
    pub sample_rate: Option<u32>,
    pub filesize: Option<u64>,
    pub category: String,
    pub subcategory: Option<String>,
    pub confidence: f64,
    pub matched_terms: Vec<String>,
    pub favorite: bool,
    pub category_manual: bool,
    pub folder: Option<String>,
    pub bpm: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Favorite,
    Filename,
    Category,
    Duration,
    Bpm,
    Channels,
    SampleRate,
    Filesize,
}

impl SortColumn {
    /// Whitelisted SQL. Sorting never interpolates a caller-supplied string.
    fn sql(self) -> &'static str {
        match self {
            SortColumn::Favorite => "s.favorite",
            SortColumn::Filename => "s.filename COLLATE NOCASE",
            SortColumn::Category => "s.category COLLATE NOCASE",
            SortColumn::Duration => "s.duration",
            SortColumn::Bpm => "s.bpm",
            SortColumn::Channels => "s.channels",
            SortColumn::SampleRate => "s.sample_rate",
            SortColumn::Filesize => "s.filesize",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// Filters the UI can combine. Every field is optional; the default searches
/// everything.
#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub query: String,
    pub max_duration: Option<f64>,
    pub channels: Option<u32>,
    /// Categories to include. Several are OR-ed, so the rail's tags can be
    /// combined: `[Kicks, Snares]` finds both. Empty means no category filter.
    ///
    /// A category filter and a subcategory filter combine as
    /// `(category IN (…)) AND (subcategory IN (…))`, which is what a rail listing
    /// categories and their subcategories means when both rows are lit.
    pub categories: Vec<String>,
    pub subcategories: Vec<String>,
    /// Absolute folder path; matches the folder and everything below it.
    pub folder: Option<String>,
    pub min_confidence: Option<f64>,
    pub favorite_only: bool,
    pub sort_by: Option<SortColumn>,
    pub sort_dir: Option<SortDir>,
    pub limit: u32,
    pub offset: u32,
}

impl SearchParams {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            limit: 50,
            ..Default::default()
        }
    }
}

/// One `(category, subcategory, count)` facet for the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryFacet {
    pub category: String,
    pub subcategory: Option<String>,
    pub count: u64,
}

/// Turn user text into an FTS5 query: every word becomes a prefix term, all
/// required, in any order. Returns `None` when there is nothing searchable, so
/// the caller falls back to an unfiltered query.
///
/// `"door squeak"` → `"door"* "squeak"*`, which finds `Squeaky_Door_02.wav`.
/// Quoting each word keeps FTS5 operators (`AND`, `NEAR`, `*`, `-`) in user input
/// from being interpreted.
pub fn fts_query(raw: &str) -> Option<String> {
    let words: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w.to_lowercase()))
        .collect();
    if words.is_empty() {
        None
    } else {
        Some(words.join(" "))
    }
}

fn trigram_query(raw: &str) -> Option<String> {
    let text = raw.trim();
    (text.chars().count() >= 3).then(|| format!("\"{}\"", text.replace('"', "\"\"")))
}

pub struct IndexDb {
    conn: Connection,
    root: PathBuf,
    _library_lock: Option<Arc<std::fs::File>>,
    favorite_index_available: bool,
}

static LIBRARY_LOCKS: OnceLock<Mutex<std::collections::HashMap<PathBuf, Weak<std::fs::File>>>> =
    OnceLock::new();

fn acquire_library_lock(root: &Path) -> Result<Arc<std::fs::File>> {
    let canonical = std::fs::canonicalize(root)?;
    let locks = LIBRARY_LOCKS.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let mut locks = locks
        .lock()
        .map_err(|_| IndexError::Db("library lock registry poisoned".into()))?;
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(&canonical).and_then(Weak::upgrade) {
        return Ok(lock);
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    canonical.hash(&mut hasher);
    let lock_dir = std::env::temp_dir().join("mio-sound-manager-locks");
    std::fs::create_dir_all(&lock_dir)?;
    let path = lock_dir.join(format!("{:016x}.lock", hasher.finish()));
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .map_err(|e| {
                IndexError::Db(format!(
                    "library is already open by another Mio instance ({}): {e}",
                    canonical.display()
                ))
            })?
    };
    #[cfg(not(windows))]
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)?;
    let lock = Arc::new(file);
    locks.insert(canonical, Arc::downgrade(&lock));
    Ok(lock)
}

#[derive(Debug)]
pub struct ScannedPath {
    pub path: String,
    pub filesize: u64,
    pub mtime_ns: i64,
}

impl IndexDb {
    /// Open (or create) the index inside `root`. Runs schema creation and any
    /// pending migration.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        if !root.is_dir() {
            return Err(IndexError::BadRoot(root.display().to_string()));
        }
        let library_lock = acquire_library_lock(&root)?;
        Self::adopt_legacy_file(&root)?;
        let path = root.join(DB_FILENAME);
        let conn = Connection::open(&path)?;
        let mut db = Self::prepare(conn, root)?;
        db._library_lock = Some(library_lock);
        Ok(db)
    }

    /// Rename a database an earlier build left behind, so the file name change is
    /// not a reindex.
    ///
    /// Only when the new name is absent: a library that has both (a pass was
    /// interrupted mid-migration, or the user ran two builds) keeps the new one and
    /// leaves the old file alone, because guessing which of the two is current is
    /// worse than leaving one stale file on disk.
    ///
    /// The `-wal` and `-shm` siblings move with it. SQLite recovers a WAL by name,
    /// so a renamed database whose journal still carries the old name would be
    /// opened as if it had been closed cleanly — the last committed transaction
    /// would vanish.
    fn adopt_legacy_file(root: &Path) -> Result<()> {
        let legacy = root.join(LEGACY_DB_FILENAME);
        let current = root.join(DB_FILENAME);
        if !legacy.is_file() || current.exists() {
            return Ok(());
        }
        for suffix in ["", "-wal", "-shm"] {
            let from = root.join(format!("{LEGACY_DB_FILENAME}{suffix}"));
            if !from.exists() {
                continue;
            }
            let to = root.join(format!("{DB_FILENAME}{suffix}"));
            std::fs::rename(&from, &to)?;
        }
        log::info!(
            "renamed {} to {} in {}",
            LEGACY_DB_FILENAME,
            DB_FILENAME,
            root.display()
        );
        Ok(())
    }
    /// An index in a temp file next to nothing, for tests and for probing.
    pub fn open_in_memory(root: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::prepare(conn, root.as_ref().to_path_buf())
    }

    fn prepare(conn: Connection, root: PathBuf) -> Result<Self> {
        // WAL before anything else: the UI reads while the indexer writes, and
        // switching journal mode later would need an exclusive lock.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // 32MB of page cache and a temp store in memory: the facet and folder
        // queries group over the whole table, and spilling to disk for that is
        // measurably slower.
        conn.pragma_update(None, "cache_size", -32_768i64)?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;

        let mut db = Self {
            conn,
            root,
            _library_lock: None,
            favorite_index_available: false,
        };
        db.create_base_schema()?;
        db.migrate()?;
        db.create_fts()?;

        // A previous bulk load died before `end_bulk`: the FTS content does not
        // match `sounds` and the secondary indexes are missing. Rebuild both now
        // rather than serving stale search results or a table with no indexes.
        if db.get_meta("fts_dirty")?.as_deref() == Some("1") {
            db.end_bulk()?;
        }
        db.favorite_index_available = db.has_index("idx_favorite")?;
        if db.get_meta("trigram_version")?.as_deref() != Some("1") {
            let tx = db.conn.unchecked_transaction()?;
            tx.execute_batch(
                "DROP TRIGGER IF EXISTS sounds_ai;
                 DROP TRIGGER IF EXISTS sounds_ad;
                 DROP TRIGGER IF EXISTS sounds_au;",
            )?;
            db.create_fts()?;
            db.rebuild_trigram_fts()?;
            db.set_meta("trigram_version", "1")?;
            tx.commit()?;
        }
        Ok(db)
    }

    fn has_index(&self, name: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name=?1)",
            params![name],
            |row| row.get(0),
        )?)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self) -> PathBuf {
        self.root.join(DB_FILENAME)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn begin_scan(&mut self) -> Result<()> {
        self.conn.pragma_update(None, "temp_store", "FILE")?;
        self.conn.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS _scan_files (
                 path TEXT PRIMARY KEY,
                 filesize INTEGER NOT NULL,
                 mtime_ns INTEGER NOT NULL
             ) WITHOUT ROWID;
             DELETE FROM _scan_files;",
        )?;
        Ok(())
    }

    pub fn stage_scan_path(&mut self, path: &Path, filesize: u64, mtime_ns: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO _scan_files(path, filesize, mtime_ns) VALUES (?1, ?2, ?3)",
            params![crate::folders::store_path(path), filesize as i64, mtime_ns],
        )?;
        Ok(())
    }

    pub fn stage_scan_batch(&mut self, paths: &[(String, u64, i64)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO _scan_files(path, filesize, mtime_ns) VALUES (?1, ?2, ?3)",
            )?;
            for (path, filesize, mtime_ns) in paths {
                stmt.execute(params![path, *filesize as i64, mtime_ns])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn scanned_count(&self) -> Result<usize> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM _scan_files", [], |r| {
                r.get::<_, i64>(0)
            })? as usize)
    }

    pub fn scan_batch(
        &self,
        after_path: Option<&str>,
        limit: usize,
        incremental: bool,
    ) -> Result<Vec<ScannedPath>> {
        use rusqlite::types::Value;
        let (sql, args): (&str, Vec<Value>) = match (incremental, after_path) {
            (true, Some(path)) => (
                "SELECT f.path, f.filesize, f.mtime_ns
                 FROM _scan_files f LEFT JOIN sounds s ON s.path = f.path
                 WHERE f.path > ?1 AND (s.path IS NULL OR COALESCE(s.mtime_ns, 0) = 0
                       OR s.mtime_ns <> f.mtime_ns OR COALESCE(s.filesize, 0) <> f.filesize)
                 ORDER BY f.path LIMIT ?2",
                vec![Value::Text(path.to_owned()), Value::Integer(limit as i64)],
            ),
            (true, None) => (
                "SELECT f.path, f.filesize, f.mtime_ns
                 FROM _scan_files f LEFT JOIN sounds s ON s.path = f.path
                 WHERE s.path IS NULL OR COALESCE(s.mtime_ns, 0) = 0
                       OR s.mtime_ns <> f.mtime_ns OR COALESCE(s.filesize, 0) <> f.filesize
                 ORDER BY f.path LIMIT ?1",
                vec![Value::Integer(limit as i64)],
            ),
            (false, Some(path)) => (
                "SELECT path, filesize, mtime_ns FROM _scan_files
                 WHERE path > ?1 ORDER BY path LIMIT ?2",
                vec![Value::Text(path.to_owned()), Value::Integer(limit as i64)],
            ),
            (false, None) => (
                "SELECT path, filesize, mtime_ns FROM _scan_files ORDER BY path LIMIT ?1",
                vec![Value::Integer(limit as i64)],
            ),
        };
        let mut stmt = self.conn.prepare_cached(sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok(ScannedPath {
                path: r.get(0)?,
                filesize: r.get::<_, i64>(1)? as u64,
                mtime_ns: r.get(2)?,
            })
        })?;
        let mut batch = Vec::new();
        for row in rows {
            batch.push(row?);
        }
        Ok(batch)
    }

    pub fn unchanged_scan_count(&self) -> Result<usize> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM _scan_files f JOIN sounds s ON s.path = f.path
             WHERE COALESCE(s.mtime_ns, 0) <> 0 AND s.mtime_ns = f.mtime_ns
               AND COALESCE(s.filesize, 0) = f.filesize",
            [],
            |r| r.get::<_, i64>(0),
        )? as usize)
    }

    pub fn prune_scan_missing(&mut self) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let removed = tx.execute(
            "DELETE FROM sounds
             WHERE NOT EXISTS (SELECT 1 FROM _scan_files f WHERE f.path = sounds.path)",
            [],
        )? as u64;
        if removed > 0 {
            bump_index_version_in(&tx)?;
        }
        tx.commit()?;
        Ok(removed)
    }

    pub fn bump_index_version(&self) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES ('index_version', '1')
             ON CONFLICT(key) DO UPDATE SET value = CAST(value AS INTEGER) + 1",
            [],
        )?;
        Ok(())
    }

    pub fn index_version(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE((SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'index_version'), 0)",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn end_scan(&mut self) -> Result<()> {
        self.conn.execute_batch("DROP TABLE IF EXISTS _scan_files;")?;
        self.conn.pragma_update(None, "temp_store", "MEMORY")?;
        Ok(())
    }

    /// The reference schema, verbatim. Safe to run against an existing database.
    fn create_base_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sounds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT UNIQUE NOT NULL,
                filename TEXT NOT NULL,
                duration REAL,
                channels INTEGER,
                sample_rate INTEGER,
                filesize INTEGER,
                category TEXT NOT NULL DEFAULT 'Uncategorized',
                subcategory TEXT,
                confidence REAL NOT NULL DEFAULT 0,
                matched_terms TEXT,
                favorite INTEGER NOT NULL DEFAULT 0,
                category_manual INTEGER NOT NULL DEFAULT 0,
                folder TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_duration ON sounds(duration);
            CREATE INDEX IF NOT EXISTS idx_channels ON sounds(channels);
            CREATE INDEX IF NOT EXISTS idx_category ON sounds(category);
            CREATE INDEX IF NOT EXISTS idx_category_subcategory ON sounds(category, subcategory);
            CREATE INDEX IF NOT EXISTS idx_confidence ON sounds(confidence);
            CREATE INDEX IF NOT EXISTS idx_favorite ON sounds(favorite);

            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT
            );
            "#,
        )?;
        Ok(())
    }

    /// Add the columns Mio Sound Manager needs on top of the reference schema. Written as
    /// individual `ALTER TABLE`s guarded by a `table_info` check, because SQLite
    /// has no `ADD COLUMN IF NOT EXISTS`.
    fn migrate(&self) -> Result<()> {
        let mut existing: Vec<String> = Vec::new();
        {
            let mut stmt = self.conn.prepare("PRAGMA table_info(sounds)")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            for row in rows {
                existing.push(row?);
            }
        }
        let has = |name: &str| existing.iter().any(|c| c == name);

        if !has("mtime_ns") {
            self.conn
                .execute_batch("ALTER TABLE sounds ADD COLUMN mtime_ns INTEGER")?;
        }
        if !has("indexed_at") {
            self.conn
                .execute_batch("ALTER TABLE sounds ADD COLUMN indexed_at INTEGER")?;
        }
        // An older index has no tempo column. The value comes from the file name,
        // which has not changed, so an incremental pass would skip every row and
        // leave the column empty until each file happened to be touched. The
        // upgrade therefore fills it in once, here, from the names already stored.
        if !has("bpm") {
            self.conn
                .execute_batch("ALTER TABLE sounds ADD COLUMN bpm REAL")?;
        }
        // Keyed on `meta` rather than on the column, because adding the column and
        // filling it can be separated by a build that was interrupted: a database
        // whose column exists but whose values are all NULL still needs the pass,
        // and one that has already been through it must not repeat it on every
        // open.
        if self.get_meta("bpm_backfilled")?.as_deref() != Some("1") {
            self.backfill_bpm()?;
            self.set_meta("bpm_backfilled", "1")?;
        }

        // The incremental-reindex lookup is `WHERE path = ?`, already covered by
        // the UNIQUE index on path. This partial index answers "what changed
        // since" without a scan, which is what the prune pass wants.
        self.conn
            .execute_batch("CREATE INDEX IF NOT EXISTS idx_mtime ON sounds(mtime_ns);")?;
        self.conn
            .execute_batch("CREATE INDEX IF NOT EXISTS idx_bpm ON sounds(bpm);")?;

        self.set_meta("schema_version", &SCHEMA_VERSION.to_string())?;
        Ok(())
    }

    /// Fill `bpm` for every existing row from its stored file name.
    ///
    /// Runs once, on the schema upgrade. The FTS triggers are suspended for the
    /// duration: `bpm` is not one of the searchable columns, so the tokenizer has
    /// nothing to update, and letting `sounds_au` run per row would pay a
    /// tokenizer pass 280k times over a column FTS does not read.
    ///
    /// A name the parser cannot read leaves the column NULL, which is the same
    /// state a fresh index would produce — unmarked, not zero.
    fn backfill_bpm(&self) -> Result<()> {
        self.conn.execute_batch(
            "DROP TRIGGER IF EXISTS sounds_ai;
             DROP TRIGGER IF EXISTS sounds_ad;
             DROP TRIGGER IF EXISTS sounds_au;",
        )?;

        let tx = self.conn.unchecked_transaction()?;
        let pairs: Vec<(i64, String)> = {
            let mut stmt = tx.prepare("SELECT id, filename FROM sounds")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            out
        };
        {
            let mut stmt = tx.prepare("UPDATE sounds SET bpm = ?1 WHERE id = ?2")?;
            for (id, filename) in &pairs {
                match crate::bpm::from_filename(filename) {
                    Some(bpm) => {
                        stmt.execute(params![bpm, id])?;
                    }
                    None => {
                        stmt.execute(params![Option::<f64>::None, id])?;
                    }
                }
            }
        }
        tx.commit()?;

        // `create_fts` is idempotent and recreates the three triggers; the FTS
        // content is untouched, so no rebuild is needed.
        self.create_fts()?;
        Ok(())
    }

    /// FTS5 external-content table over the four searchable columns, plus the
    /// three triggers that keep it correct.
    fn create_fts(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS sounds_fts USING fts5(
                filename,
                category,
                subcategory,
                folder,
                content='sounds',
                content_rowid='id',
                tokenize = "unicode61 remove_diacritics 2"
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS sounds_trigram USING fts5(
                filename,
                content='sounds',
                content_rowid='id',
                tokenize='trigram'
            );

            CREATE TRIGGER IF NOT EXISTS sounds_ai AFTER INSERT ON sounds BEGIN
              INSERT INTO sounds_fts(rowid, filename, category, subcategory, folder)
              VALUES (new.id, new.filename, new.category, new.subcategory, new.folder);
              INSERT INTO sounds_trigram(rowid, filename) VALUES (new.id, new.filename);
            END;

            CREATE TRIGGER IF NOT EXISTS sounds_ad AFTER DELETE ON sounds BEGIN
              INSERT INTO sounds_fts(sounds_fts, rowid, filename, category, subcategory, folder)
              VALUES ('delete', old.id, old.filename, old.category, old.subcategory, old.folder);
              INSERT INTO sounds_trigram(sounds_trigram, rowid, filename)
              VALUES ('delete', old.id, old.filename);
            END;

            CREATE TRIGGER IF NOT EXISTS sounds_au AFTER UPDATE ON sounds BEGIN
              INSERT INTO sounds_fts(sounds_fts, rowid, filename, category, subcategory, folder)
              VALUES ('delete', old.id, old.filename, old.category, old.subcategory, old.folder);
              INSERT INTO sounds_trigram(sounds_trigram, rowid, filename)
              VALUES ('delete', old.id, old.filename);
              INSERT INTO sounds_fts(rowid, filename, category, subcategory, folder)
              VALUES (new.id, new.filename, new.category, new.subcategory, new.folder);
              INSERT INTO sounds_trigram(rowid, filename) VALUES (new.id, new.filename);
            END;
            "#,
        )?;
        Ok(())
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get::<_, Option<String>>(0)
            })
            .optional()?
            .flatten())
    }

    /// Write a batch of rows in one transaction.
    ///
    /// This is the only hot write path. The statement is prepared once and
    /// reused; the transaction is the caller's unit of atomicity, so a crash
    /// mid-index leaves the previous state intact rather than a half-written
    /// library.
    pub fn upsert_batch(&mut self, rows: &[SoundRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let tx = self.conn.transaction()?;
        {
            // `favorite` is absent from the UPDATE clause on purpose: a reindex
            // refreshes metadata and the classifier's guess, never user state.
            // The classifier columns are conditional so a manual assignment
            // survives.
            let mut stmt = tx.prepare_cached(
                r#"
                INSERT INTO sounds
                    (path, filename, duration, channels, sample_rate, filesize,
                     category, subcategory, confidence, matched_terms, folder,
                     mtime_ns, indexed_at, bpm)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                ON CONFLICT(path) DO UPDATE SET
                    filename = excluded.filename,
                    folder = excluded.folder,
                    duration = excluded.duration,
                    channels = excluded.channels,
                    sample_rate = excluded.sample_rate,
                    filesize = excluded.filesize,
                    mtime_ns = excluded.mtime_ns,
                    indexed_at = excluded.indexed_at,
                    bpm = excluded.bpm,
                    category = CASE WHEN sounds.category_manual = 1
                                    THEN sounds.category ELSE excluded.category END,
                    subcategory = CASE WHEN sounds.category_manual = 1
                                       THEN sounds.subcategory ELSE excluded.subcategory END,
                    confidence = CASE WHEN sounds.category_manual = 1
                                      THEN sounds.confidence ELSE excluded.confidence END,
                    matched_terms = CASE WHEN sounds.category_manual = 1
                                         THEN sounds.matched_terms ELSE excluded.matched_terms END
                "#,
            )?;

            for row in rows {
                let terms =
                    serde_json::to_string(&row.matched_terms).unwrap_or_else(|_| "[]".into());
                stmt.execute(params![
                    row.path,
                    row.filename,
                    row.duration_s,
                    row.channels,
                    row.sample_rate,
                    row.filesize as i64,
                    row.category,
                    row.subcategory,
                    row.confidence,
                    terms,
                    row.folder,
                    row.mtime_ns,
                    now,
                    row.bpm,
                ])?;
            }
        }
        bump_index_version_in(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Delete rows for files that no longer exist, given the complete set of
    /// paths seen on this pass.
    ///
    /// Deliberately not "delete everything, then reinsert": the upsert above
    /// preserves favorites and manual categories in place, so only genuinely
    /// vanished files need removing.
    ///
    /// `NOT EXISTS` rather than `NOT IN (SELECT ...)`: SQLite turns the latter
    /// into a correlated scan that measured 36 s on a 280k-row library. The
    /// correlated form hits the primary key on `_current_paths` once per row and
    /// measured under a second.
    pub fn prune_missing(&mut self, current_paths: &[String]) -> Result<u64> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS _current_paths (path TEXT PRIMARY KEY);
             DELETE FROM _current_paths;",
        )?;
        {
            let mut insert =
                tx.prepare("INSERT OR IGNORE INTO _current_paths(path) VALUES (?1)")?;
            for path in current_paths {
                insert.execute(params![path])?;
            }
        }
        let removed = tx.execute(
            "DELETE FROM sounds
             WHERE NOT EXISTS (SELECT 1 FROM _current_paths c WHERE c.path = sounds.path)",
            [],
        )? as u64;
        tx.execute_batch("DROP TABLE _current_paths;")?;
        if removed > 0 {
            bump_index_version_in(&tx)?;
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Suspend the FTS triggers for a bulk load, then rebuild the index once.
    ///
    /// The three triggers keep `sounds_fts` correct row by row, which is right
    /// for single edits but wrong for a 280k-row index pass: each insert pays a
    /// tokenizer pass and each delete pays a second one. Measured on the real
    /// library, trigger-maintained FTS accounted for 40 s of a 165 s pass.
    /// Dropping the triggers and running one `rebuild` at the end is the
    /// documented way to bulk-load an external-content FTS5 table.
    ///
    /// Must be paired with [`Self::end_bulk`]; if the process dies in between,
    /// the next `open` recreates the triggers but the FTS content will be stale
    /// until a rebuild, so `open` marks the schema as needing one.
    pub fn begin_bulk(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "DROP TRIGGER IF EXISTS sounds_ai;
             DROP TRIGGER IF EXISTS sounds_ad;
             DROP TRIGGER IF EXISTS sounds_au;",
        )?;
        self.favorite_index_available = false;
        // Secondary indexes are maintained per row, which is the right trade for
        // single edits and the wrong one for a 280k-row load: seven of them cost
        // 16 s of a 82 s pass. Dropping and rebuilding once is the documented
        // bulk-load pattern and measured 4 s for the whole rebuild.
        //
        // The UNIQUE index on `path` stays: the upsert's `ON CONFLICT(path)`
        // clause needs it, and without it every insert would be a table scan.
        self.conn.execute_batch(
            "DROP INDEX IF EXISTS idx_duration;
             DROP INDEX IF EXISTS idx_channels;
             DROP INDEX IF EXISTS idx_category;
             DROP INDEX IF EXISTS idx_category_subcategory;
             DROP INDEX IF EXISTS idx_confidence;
             DROP INDEX IF EXISTS idx_favorite;
             DROP INDEX IF EXISTS idx_mtime;
             DROP INDEX IF EXISTS idx_bpm;",
        )?;
        self.set_meta("fts_dirty", "1")?;
        Ok(())
    }

    /// Restore the triggers and indexes, rebuild `sounds_fts` from `sounds`.
    pub fn end_bulk(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_duration ON sounds(duration);
             CREATE INDEX IF NOT EXISTS idx_channels ON sounds(channels);
             CREATE INDEX IF NOT EXISTS idx_category ON sounds(category);
             CREATE INDEX IF NOT EXISTS idx_category_subcategory ON sounds(category, subcategory);
             CREATE INDEX IF NOT EXISTS idx_confidence ON sounds(confidence);
             CREATE INDEX IF NOT EXISTS idx_favorite ON sounds(favorite);
             CREATE INDEX IF NOT EXISTS idx_mtime ON sounds(mtime_ns);
             CREATE INDEX IF NOT EXISTS idx_bpm ON sounds(bpm);",
        )?;
        self.create_fts()?;
        self.rebuild_fts()?;
        self.rebuild_trigram_fts()?;
        self.set_meta("fts_dirty", "0")?;
        self.favorite_index_available = true;
        Ok(())
    }

    pub fn count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM sounds", [], |r| r.get::<_, i64>(0))?
            as u64)
    }

    /// `(path, mtime_ns)` for every row. The incremental pass compares this
    /// against the filesystem walk to decide what to probe.
    pub fn path_mtimes(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, COALESCE(mtime_ns, 0) FROM sounds")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_by_path(&self, path: &str) -> Result<Option<Sound>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {SOUND_COLUMNS} FROM sounds s WHERE s.path = ?1"
        ))?;
        let mut rows = stmt.query(params![path])?;
        match rows.next()? {
            Some(row) => Ok(Some(read_sound(row)?)),
            None => Ok(None),
        }
    }

    pub fn set_favorite(&self, path: &str, favorite: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE sounds SET favorite = ?1 WHERE path = ?2",
            params![favorite as i64, path],
        )?;
        Ok(())
    }

    /// Set the favorite flag on many paths in one transaction.
    ///
    /// The single-row form is fine for a star click; this one exists because the
    /// table lets the user star a whole multi-selection, and one transaction per
    /// row would commit — and fsync under WAL — once per sound.
    pub fn set_favorite_many(&mut self, paths: &[String], favorite: bool) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let changed = {
            let mut stmt = tx.prepare("UPDATE sounds SET favorite = ?1 WHERE path = ?2")?;
            let mut changed = 0u64;
            for path in paths {
                changed += stmt.execute(params![favorite as i64, path])? as u64;
            }
            changed
        };
        tx.commit()?;
        Ok(changed)
    }

    /// Assign a category by hand. Sets `category_manual`, so the next reindex
    /// keeps the choice instead of overwriting it with the classifier's guess.
    pub fn set_category(
        &self,
        path: &str,
        category: &str,
        subcategory: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE sounds
             SET category = ?1, subcategory = ?2, confidence = 1, category_manual = 1
             WHERE path = ?3",
            params![category, subcategory, path],
        )?;
        Ok(())
    }

    /// Assign the same category to many paths in one transaction.
    ///
    /// Same reason as [`set_favorite_many`](Self::set_favorite_many): the UI
    /// categorises a multi-selection, and the FTS triggers fire per row, so the
    /// batching has to be at the transaction level.
    pub fn set_category_many(
        &mut self,
        paths: &[String],
        category: &str,
        subcategory: Option<&str>,
    ) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let changed = {
            let mut stmt = tx.prepare(
                "UPDATE sounds
                 SET category = ?1, subcategory = ?2, confidence = 1, category_manual = 1
                 WHERE path = ?3",
            )?;
            let mut changed = 0u64;
            for path in paths {
                changed += stmt.execute(params![category, subcategory, path])? as u64;
            }
            changed
        };
        tx.commit()?;
        Ok(changed)
    }

    /// Undo a manual assignment, so the next reindex re-classifies the file.
    pub fn clear_category(&self, path: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sounds
             SET category = 'Uncategorized', subcategory = NULL, confidence = 0,
                 matched_terms = NULL, category_manual = 0
             WHERE path = ?1",
            params![path],
        )?;
        Ok(())
    }

    /// Clear the manual category on many paths in one transaction.
    pub fn clear_category_many(&mut self, paths: &[String]) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let changed = {
            let mut stmt = tx.prepare(
                "UPDATE sounds
                 SET category = 'Uncategorized', subcategory = NULL, confidence = 0,
                     matched_terms = NULL, category_manual = 0
                 WHERE path = ?1",
            )?;
            let mut changed = 0u64;
            for path in paths {
                changed += stmt.execute(params![path])? as u64;
            }
            changed
        };
        tx.commit()?;
        Ok(changed)
    }

    /// Write a category back exactly as given, including the "the classifier owns
    /// this" flag.
    ///
    /// This exists for undo. `set_category` always marks the row manual and
    /// `clear_category` always resets it to `Uncategorized`, so neither can
    /// reproduce a state that was *neither* — a row the classifier filled in.
    /// Undo needs to put back the exact string and the exact ownership, or it
    /// silently converts a guess into a manual choice.
    pub fn restore_category_many(
        &mut self,
        paths: &[String],
        category: &str,
        subcategory: Option<&str>,
        manual: bool,
    ) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let changed = {
            let mut stmt = tx.prepare(
                "UPDATE sounds
                 SET category = ?1, subcategory = ?2, category_manual = ?3,
                     confidence = CASE WHEN ?3 = 1 THEN 1 ELSE confidence END
                 WHERE path = ?4",
            )?;
            let mut changed = 0u64;
            for path in paths {
                changed +=
                    stmt.execute(params![category, subcategory, manual as i64, path])? as u64;
            }
            changed
        };
        tx.commit()?;
        Ok(changed)
    }

    pub fn search(&self, params: &SearchParams) -> Result<Vec<Sound>> {
        let (sql, args) = self.build_search_sql(params, false, false);
        let mut out = self.read_sounds(&sql, &args)?;
        if out.is_empty() && !params.query.trim().is_empty() {
            if trigram_query(&params.query).is_some()
                && self.count_search_mode(params, false)? == 0
            {
                let (sql, args) = self.build_search_sql(params, false, true);
                out = self.read_sounds(&sql, &args)?;
            }
        }
        Ok(out)
    }

    fn read_sounds(&self, sql: &str, args: &[rusqlite::types::Value]) -> Result<Vec<Sound>> {
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query(params_from_iter(args.iter()))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(read_sound(row)?);
        }
        Ok(out)
    }

    pub fn count_matching(&self, params: &SearchParams) -> Result<u64> {
        let count = self.count_search_mode(params, false)?;
        if count == 0 && !params.query.trim().is_empty() && trigram_query(&params.query).is_some() {
            return self.count_search_mode(params, true);
        }
        Ok(count)
    }

    fn count_search_mode(&self, params: &SearchParams, substring: bool) -> Result<u64> {
        let (sql, args) = self.build_search_sql(params, true, substring);
        Ok(self
            .conn
            .query_row(&sql, params_from_iter(args.iter()), |r| r.get::<_, i64>(0))?
            as u64)
    }

    /// `(sql, bound parameters)` for both the search and the count, so the two
    /// can never drift apart.
    fn build_search_sql(
        &self,
        params: &SearchParams,
        counting: bool,
        substring: bool,
    ) -> (String, Vec<rusqlite::types::Value>) {
        use rusqlite::types::Value;

        let mut clauses: Vec<String> = Vec::new();
        let mut args: Vec<Value> = Vec::new();

        let match_expr = if substring {
            trigram_query(&params.query)
        } else {
            fts_query(&params.query)
        };
        let fts_table = if substring {
            "sounds_trigram"
        } else {
            "sounds_fts"
        };
        let from = if match_expr.is_some() {
            format!("{fts_table} f JOIN sounds s ON s.id = f.rowid")
        } else {
            "sounds s".to_string()
        };
        let mut sql = if counting {
            if match_expr.is_some() {
                format!("SELECT COUNT(*) FROM {from} WHERE {fts_table} MATCH ?")
            } else {
                format!("SELECT COUNT(*) FROM {from}")
            }
        } else if match_expr.is_some() {
            format!("SELECT {SOUND_COLUMNS} FROM {from} WHERE {fts_table} MATCH ?")
        } else {
            format!("SELECT {SOUND_COLUMNS} FROM {from}")
        };

        if let Some(expr) = &match_expr {
            args.push(Value::Text(expr.clone()));
        } else {
            clauses.push("1=1".to_string());
        }

        if let Some(max) = params.max_duration {
            clauses.push("s.duration IS NOT NULL AND s.duration < ?".to_string());
            args.push(Value::Real(max));
        }
        if let Some(ch) = params.channels {
            clauses.push("s.channels = ?".to_string());
            args.push(Value::Integer(ch as i64));
        }
        // A multi-category filter is `IN (?, ?, ?)`. The placeholders are
        // generated from the list length, never from the strings themselves; the
        // values still go through `params_from_iter`.
        if !params.categories.is_empty() {
            let holes = std::iter::repeat_n("?", params.categories.len())
                .collect::<Vec<_>>()
                .join(", ");
            clauses.push(format!("s.category IN ({holes})"));
            for cat in &params.categories {
                args.push(Value::Text(cat.clone()));
            }
        }
        if !params.subcategories.is_empty() {
            let holes = std::iter::repeat_n("?", params.subcategories.len())
                .collect::<Vec<_>>()
                .join(", ");
            clauses.push(format!("s.subcategory IN ({holes})"));
            for sub in &params.subcategories {
                args.push(Value::Text(sub.clone()));
            }
        }
        if let Some(folder) = &params.folder {
            if !folder.is_empty() {
                let (lo, hi) = folder_range(folder);
                clauses.push("s.path >= ? AND s.path < ?".to_string());
                args.push(Value::Text(lo));
                args.push(Value::Text(hi));
            }
        }
        if let Some(min) = params.min_confidence {
            clauses.push("s.confidence >= ?".to_string());
            args.push(Value::Real(min));
        }
        if params.favorite_only {
            clauses.push("s.favorite = 1".to_string());
        }

        if !clauses.is_empty() {
            sql.push_str(if match_expr.is_some() {
                " AND "
            } else {
                " WHERE "
            });
            sql.push_str(&clauses.join(" AND "));
        }

        if !counting {
            if let Some(sort) = params.sort_by {
                let dir = match params.sort_dir.unwrap_or(SortDir::Asc) {
                    SortDir::Asc => "ASC",
                    SortDir::Desc => "DESC",
                };
                let col = sort.sql();
                // NULLs last in both directions: an unset duration must not jump
                // to the top just because the user sorted descending.
                sql.push_str(&format!(
                    " ORDER BY {col} IS NULL, {col} {dir}, s.filename COLLATE NOCASE ASC"
                ));
            }
            sql.push_str(" LIMIT ? OFFSET ?");
            args.push(Value::Integer(
                if params.limit == 0 { 50 } else { params.limit } as i64,
            ));
            args.push(Value::Integer(params.offset as i64));
        }

        (sql, args)
    }

    pub fn facets(&self) -> Result<Vec<CategoryFacet>> {
        let mut stmt = self.conn.prepare(
            "SELECT category, subcategory, COUNT(*) AS c
             FROM sounds GROUP BY category, subcategory ORDER BY c DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(CategoryFacet {
                category: r.get(0)?,
                subcategory: r.get(1)?,
                count: r.get::<_, i64>(2)? as u64,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Favorite count and total in one statement.
    ///
    /// The sidebar shows both numbers next to each other; two separate queries
    /// would let them disagree for a frame right after a star click.
    pub fn summary(&self) -> Result<(u64, u64)> {
        let sql = if self.favorite_index_available {
            "SELECT COALESCE(SUM(favorite), 0), COUNT(favorite)
             FROM sounds INDEXED BY idx_favorite"
        } else {
            "SELECT COALESCE(SUM(favorite), 0), COUNT(*) FROM sounds"
        };
        let (favorites, total) = self.conn.query_row(
            sql,
            [],
            |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)),
        )?;
        Ok((favorites, total))
    }

    /// The folder tree under the library root, with recursive counts.
    ///
    /// Reads only `path`, which the unique index covers, so this is an index
    /// scan rather than a table scan.
    pub fn folder_tree(&self) -> Result<FolderNode> {
        let mut stmt = self.conn.prepare("SELECT path FROM sounds")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut paths = Vec::new();
        for row in rows {
            paths.push(row?);
        }
        // Normalise the root the same way the stored paths were normalised.
        // Without this, a root given as `D:/lib` never prefix-matches a stored
        // `D:\lib\...` path and the tree comes back empty.
        let root = crate::folders::normalize_root(&self.root.to_string_lossy());
        Ok(build_folder_tree(paths.iter().map(|s| s.as_str()), &root))
    }

    /// Rebuild the FTS index from the `sounds` table. Needed after a bulk write
    /// that bypassed the triggers, and after importing a database whose FTS
    /// table was missing or built with a different tokenizer.
    pub fn rebuild_fts(&self) -> Result<()> {
        self.conn
            .execute_batch("INSERT INTO sounds_fts(sounds_fts) VALUES ('rebuild');")?;
        Ok(())
    }

    fn rebuild_trigram_fts(&self) -> Result<()> {
        self.conn
            .execute_batch("INSERT INTO sounds_trigram(sounds_trigram) VALUES ('rebuild');")?;
        Ok(())
    }
}

/// Column list shared by every read, in the order `read_sound` expects.
const SOUND_COLUMNS: &str = "s.id, s.path, s.filename, s.duration, s.channels, s.sample_rate, \
     s.filesize, s.category, s.subcategory, s.confidence, s.matched_terms, s.favorite, \
     s.category_manual, s.folder, s.bpm";

fn bump_index_version_in(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute(
        "INSERT INTO meta(key, value) VALUES ('index_version', '1')
         ON CONFLICT(key) DO UPDATE SET value = CAST(value AS INTEGER) + 1",
        [],
    )?;
    Ok(())
}

fn read_sound(row: &rusqlite::Row<'_>) -> rusqlite::Result<Sound> {
    let terms_raw: Option<String> = row.get(10)?;
    let matched_terms: Vec<String> = terms_raw
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    Ok(Sound {
        id: row.get(0)?,
        path: row.get(1)?,
        filename: row.get(2)?,
        duration_s: row.get(3)?,
        channels: row.get::<_, Option<i64>>(4)?.map(|v| v as u32),
        sample_rate: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
        filesize: row.get::<_, Option<i64>>(6)?.map(|v| v as u64),
        category: row.get(7)?,
        subcategory: row.get(8)?,
        confidence: row.get(9)?,
        matched_terms,
        favorite: row.get::<_, i64>(11)? == 1,
        category_manual: row.get::<_, i64>(12)? == 1,
        folder: row.get(13)?,
        bpm: row.get(14)?,
    })
}

/// Build a row from a probed file and its classification. Kept here so the
/// indexer has no schema knowledge and this module has no probe knowledge.
pub fn row_from_parts(
    root: &Path,
    path: &Path,
    meta: &crate::probe::AudioMeta,
    classification: &crate::classify::Classification,
) -> SoundRow {
    // Both sides of the comparison go through the same normalisation, so a root
    // given as `D:/lib` and a walked path that comes back as `D:\lib\...` still
    // produce a relative folder instead of an empty one.
    let path_str = crate::folders::store_path(path);
    let root_str = crate::folders::normalize_root(&root.to_string_lossy());
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    // Read before `filename` is moved into the row. From the name only: a WAV
    // header carries no tempo, and deriving it from the waveform would mean
    // decoding every file during the index pass.
    let bpm = crate::bpm::from_filename(&filename);
    SoundRow {
        folder: relative_folder(&root_str, &path_str),
        path: path_str,
        filename,
        duration_s: meta.duration_s,
        channels: meta.channels,
        sample_rate: meta.sample_rate,
        filesize: meta.filesize,
        category: classification.category.clone(),
        subcategory: classification.subcategory.clone(),
        confidence: classification.confidence,
        matched_terms: classification.matched_terms.clone(),
        mtime_ns: meta.mtime_ns,
        bpm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::{Classification, Classifier};
    use crate::probe::AudioMeta;

    fn row(path: &str, category: &str) -> SoundRow {
        SoundRow {
            path: path.to_string(),
            filename: path.rsplit(['\\', '/']).next().unwrap().to_string(),
            duration_s: Some(1.0),
            channels: Some(2),
            sample_rate: Some(44_100),
            filesize: 100,
            category: category.to_string(),
            subcategory: None,
            confidence: 0.5,
            matched_terms: vec!["door".into()],
            folder: String::new(),
            mtime_ns: 1,
            bpm: None,
        }
    }

    fn db() -> IndexDb {
        IndexDb::open_in_memory("C:/lib").unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn the_lock_allows_same_process_reuse_but_rejects_another_process() {
        if let Ok(root) = std::env::var("MIO_LOCK_CHILD_ROOT") {
            let error = match IndexDb::open(root) {
                Ok(_) => panic!("a second process unexpectedly opened the locked library"),
                Err(error) => error,
            };
            assert!(error.to_string().contains("already open"));
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "mio-lock-same-process-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let first = IndexDb::open(&root).unwrap();
        let second = IndexDb::open(&root).unwrap();
        assert_eq!(first.count().unwrap(), second.count().unwrap());
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "db::tests::the_lock_allows_same_process_reuse_but_rejects_another_process",
                "--nocapture",
            ])
            .env("MIO_LOCK_CHILD_ROOT", &root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        drop((first, second));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn upsert_is_idempotent_on_path() {
        let mut db = db();
        db.upsert_batch(&[row("C:/lib/a.wav", "Foley")]).unwrap();
        db.upsert_batch(&[row("C:/lib/a.wav", "Impacts")]).unwrap();
        assert_eq!(db.count().unwrap(), 1);
        let sound = db.get_by_path("C:/lib/a.wav").unwrap().unwrap();
        assert_eq!(sound.category, "Impacts");
    }

    #[test]
    fn a_reindex_preserves_favorite_and_manual_category() {
        let mut db = db();
        db.upsert_batch(&[row("C:/lib/a.wav", "Foley")]).unwrap();
        db.set_favorite("C:/lib/a.wav", true).unwrap();
        db.set_category("C:/lib/a.wav", "Weapons", Some("Firearms"))
            .unwrap();

        // The classifier now says something else; the user's choice must win.
        db.upsert_batch(&[row("C:/lib/a.wav", "Ambience")]).unwrap();

        let sound = db.get_by_path("C:/lib/a.wav").unwrap().unwrap();
        assert!(sound.favorite);
        assert_eq!(sound.category, "Weapons");
        assert_eq!(sound.subcategory.as_deref(), Some("Firearms"));
        assert_eq!(sound.confidence, 1.0);
    }

    #[test]
    fn clearing_a_manual_category_lets_the_next_reindex_reclassify() {
        let mut db = db();
        db.upsert_batch(&[row("C:/lib/a.wav", "Foley")]).unwrap();
        db.set_category("C:/lib/a.wav", "Weapons", None).unwrap();
        db.clear_category("C:/lib/a.wav").unwrap();
        db.upsert_batch(&[row("C:/lib/a.wav", "Ambience")]).unwrap();
        let sound = db.get_by_path("C:/lib/a.wav").unwrap().unwrap();
        assert_eq!(sound.category, "Ambience");
        assert!(!sound.category_manual);
    }

    #[test]
    fn prune_removes_only_vanished_paths() {
        let mut db = db();
        db.upsert_batch(&[
            row("C:/lib/a.wav", "Foley"),
            row("C:/lib/b.wav", "Foley"),
            row("C:/lib/c.wav", "Foley"),
        ])
        .unwrap();
        let removed = db
            .prune_missing(&["C:/lib/a.wav".to_string(), "C:/lib/c.wav".to_string()])
            .unwrap();
        assert_eq!(removed, 1);
        assert!(db.get_by_path("C:/lib/b.wav").unwrap().is_none());
        assert_eq!(db.count().unwrap(), 2);
    }

    #[test]
    fn summary_uses_indexed_counts_and_stays_consistent_after_favorite_edits() {
        let mut db = db();
        db.upsert_batch(&[row("C:/lib/a.wav", "Foley"), row("C:/lib/b.wav", "Music")])
            .unwrap();
        assert_eq!(db.summary().unwrap(), (0, 2));
        db.set_favorite_many(&["C:/lib/b.wav".into()], true)
            .unwrap();
        assert_eq!(db.summary().unwrap(), (1, 2));

        let plan: Vec<String> = db
            .connection()
            .prepare("EXPLAIN QUERY PLAN SELECT COALESCE(SUM(favorite), 0), COUNT(favorite) FROM sounds INDEXED BY idx_favorite")
            .unwrap()
            .query_map([], |row| row.get(3))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert!(plan.iter().any(|detail| detail.contains("idx_favorite")));

        db.begin_bulk().unwrap();
        assert_eq!(db.summary().unwrap(), (1, 2));
        db.end_bulk().unwrap();
        assert_eq!(db.summary().unwrap(), (1, 2));
    }

    #[test]
    fn scan_diff_reprobes_when_size_changes_even_if_mtime_is_unchanged() {
        let mut db = db();
        db.upsert_batch(&[row("C:/lib/a.wav", "Foley")]).unwrap();
        db.begin_scan().unwrap();
        db.stage_scan_batch(&[("C:/lib/a.wav".into(), 101, 1)]).unwrap();
        let batch = db.scan_batch(None, 10, true).unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].filesize, 101);
        db.end_scan().unwrap();
    }

    #[test]
    fn search_matches_every_word_as_a_prefix_in_any_order() {
        let mut db = db();
        db.upsert_batch(&[
            row(r"C:\lib\Squeaky_Door_02.wav", "Foley"),
            row(r"C:\lib\door_slam.wav", "Foley"),
            row(r"C:\lib\kick.wav", "Impacts"),
        ])
        .unwrap();

        let hits = db.search(&SearchParams::new("door squeaky")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].filename, "Squeaky_Door_02.wav");

        let hits = db.search(&SearchParams::new("doo")).unwrap();
        assert_eq!(hits.len(), 2, "prefix match should find both doors");
    }

    #[test]
    fn search_falls_back_to_filename_substrings_only_after_prefix_misses() {
        let mut db = db();
        db.upsert_batch(&[
            row(r"C:\lib\Piano_C4.wav", "Music"),
            row(r"C:\lib\Pianist_C4.wav", "Music"),
        ])
        .unwrap();

        let hits = db.search(&SearchParams::new("iano_c4")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].filename, "Piano_C4.wav");
        assert_eq!(db.count_matching(&SearchParams::new("iano_c4")).unwrap(), 1);

        let mut second_page = SearchParams::new("iano_c4");
        second_page.offset = 1;
        assert!(
            db.search(&second_page).unwrap().is_empty(),
            "the trigram fallback must not repeat first-page matches at a nonzero offset"
        );

        let prefix = db.search(&SearchParams::new("pian")).unwrap();
        assert_eq!(prefix.len(), 2, "the existing prefix search stays primary");
        assert!(
            db.search(&SearchParams::new("no-match"))
                .unwrap()
                .is_empty()
        );
        assert!(db.search(&SearchParams::new("ia")).unwrap().is_empty());
    }

    #[test]
    fn trigram_index_tracks_filename_updates_and_deletes() {
        let mut db = db();
        db.upsert_batch(&[row(r"C:\lib\Piano_C4.wav", "Music")])
            .unwrap();
        assert_eq!(db.search(&SearchParams::new("iano")).unwrap().len(), 1);

        let mut renamed = row(r"C:\lib\Piano_C4.wav", "Music");
        renamed.filename = "Violin_C4.wav".into();
        db.upsert_batch(&[renamed]).unwrap();
        let stale = db.search(&SearchParams::new("iano")).unwrap();
        assert!(stale.is_empty(), "stale trigram hits: {stale:?}");
        assert_eq!(db.search(&SearchParams::new("ioli")).unwrap().len(), 1);

        db.prune_missing(&[]).unwrap();
        assert!(db.search(&SearchParams::new("ioli")).unwrap().is_empty());
    }

    #[test]
    fn fts_operators_in_user_input_are_not_interpreted() {
        let mut db = db();
        db.upsert_batch(&[row(r"C:\lib\AND_OR_NEAR.wav", "Foley")])
            .unwrap();
        // A naive implementation would hand "AND OR NEAR" to FTS5 as operators
        // and get a syntax error or a wildly different result set.
        let hits = db.search(&SearchParams::new("AND OR NEAR")).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn folder_filter_includes_subfolders_but_not_siblings() {
        let mut db = db();
        db.upsert_batch(&[
            row(r"C:\lib\Foley\door.wav", "Foley"),
            row(r"C:\lib\Foley\Cloth\rustle.wav", "Foley"),
            row(r"C:\lib\Foley2\creak.wav", "Foley"),
        ])
        .unwrap();
        let mut params = SearchParams::new("");
        params.folder = Some(r"C:\lib\Foley".to_string());
        let hits = db.search(&params).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(db.count_matching(&params).unwrap(), 2);
    }

    #[test]
    fn nulls_sort_last_in_both_directions() {
        let mut db = db();
        let mut with_duration = row("C:/lib/a.wav", "Foley");
        with_duration.duration_s = Some(2.0);
        let mut without = row("C:/lib/b.wav", "Foley");
        without.duration_s = None;
        db.upsert_batch(&[with_duration, without]).unwrap();

        for dir in [SortDir::Asc, SortDir::Desc] {
            let mut params = SearchParams::new("");
            params.sort_by = Some(SortColumn::Duration);
            params.sort_dir = Some(dir);
            let hits = db.search(&params).unwrap();
            assert_eq!(hits[1].duration_s, None, "nulls must sort last ({dir:?})");
        }
    }

    #[test]
    fn an_older_index_gets_its_bpm_filled_in_on_open() {
        // Build a v1-shaped database: no `bpm` column, one row with a tempo in its
        // name. Opening it has to run the migration and backfill, not wait for a
        // reindex that the incremental check would skip.
        let root = std::env::temp_dir().join(format!(
            "mio-bpm-migrate-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        {
            let conn = rusqlite::Connection::open(root.join(DB_FILENAME)).unwrap();
            conn.execute_batch(
                "CREATE TABLE sounds (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     path TEXT UNIQUE NOT NULL,
                     filename TEXT NOT NULL,
                     duration REAL, channels INTEGER, sample_rate INTEGER, filesize INTEGER,
                     category TEXT NOT NULL DEFAULT 'Uncategorized', subcategory TEXT,
                     confidence REAL NOT NULL DEFAULT 0, matched_terms TEXT,
                     favorite INTEGER NOT NULL DEFAULT 0,
                     category_manual INTEGER NOT NULL DEFAULT 0,
                     folder TEXT, mtime_ns INTEGER, indexed_at INTEGER
                 );
                 INSERT INTO sounds (path, filename, category, mtime_ns)
                 VALUES ('C:/lib/Kick_128BPM.wav', 'Kick_128BPM.wav', 'Drums', 99),
                        ('C:/lib/Snare_02.wav', 'Snare_02.wav', 'Drums', 99);",
            )
            .unwrap();
        }

        let db = IndexDb::open(&root).expect("open migrates the schema");
        let kick = db.get_by_path("C:/lib/Kick_128BPM.wav").unwrap().unwrap();
        assert_eq!(
            kick.bpm,
            Some(128.0),
            "backfilled from the stored file name"
        );
        let snare = db.get_by_path("C:/lib/Snare_02.wav").unwrap().unwrap();
        assert_eq!(snare.bpm, None, "a name without a marker stays unmarked");
        // The upgrade must not cost the user their index or their mtime.
        assert_eq!(db.count().unwrap(), 2);
        assert_eq!(kick.path, "C:/lib/Kick_128BPM.wav");

        // The triggers have to be back in place, or edits stop reaching search.
        db.rebuild_fts().unwrap();
        let hits = db.search(&SearchParams::new("kick")).unwrap();
        assert_eq!(hits.len(), 1);

        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_old_database_file_is_renamed_not_reindexed() {
        // A library indexed by an earlier build still has `slip-sound.db`. Opening
        // it has to adopt that file: writing a fresh empty database next to it would
        // throw away the user's index and cost a full pass.
        let root = std::env::temp_dir().join(format!(
            "mio-db-rename-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        {
            let mut db = IndexDb::open(&root).unwrap();
            db.upsert_batch(&[row("C:/lib/Kick.wav", "Drums")]).unwrap();
            db.rebuild_fts().unwrap();
        }
        // Put it back to the old name, journals included, which is what a library
        // from before the rename looks like.
        for suffix in ["", "-wal", "-shm"] {
            let from = root.join(format!("{DB_FILENAME}{suffix}"));
            if from.exists() {
                std::fs::rename(&from, root.join(format!("{LEGACY_DB_FILENAME}{suffix}"))).unwrap();
            }
        }
        assert!(!root.join(DB_FILENAME).exists());

        let db = IndexDb::open(&root).expect("open adopts the legacy file");
        assert!(root.join(DB_FILENAME).is_file(), "the file was renamed");
        assert!(
            !root.join(LEGACY_DB_FILENAME).exists(),
            "the old name is gone, not left behind"
        );
        assert_eq!(db.count().unwrap(), 1, "the rows survived the rename");
        assert!(db.get_by_path("C:/lib/Kick.wav").unwrap().is_some());

        // A second open must be a plain open: no legacy file left to adopt.
        drop(db);
        let again = IndexDb::open(&root).unwrap();
        assert_eq!(again.count().unwrap(), 1);

        drop(again);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn several_categories_are_or_ed_and_narrow_by_subcategory() {
        let mut db = db();
        let mut a = row("C:/lib/a.wav", "Drums");
        a.subcategory = Some("Kicks".into());
        let mut b = row("C:/lib/b.wav", "Bass");
        b.subcategory = Some("Sub & 808".into());
        let c = row("C:/lib/c.wav", "Foley");
        db.upsert_batch(&[a, b, c]).unwrap();

        let mut params = SearchParams::new("");
        params.categories = vec!["Drums".into(), "Bass".into()];
        assert_eq!(db.search(&params).unwrap().len(), 2);
        assert_eq!(db.count_matching(&params).unwrap(), 2);

        // A subcategory filter intersects with the categories rather than
        // replacing them: the rail can have a category and a subcategory lit.
        params.subcategories = vec!["Kicks".into()];
        let hits = db.search(&params).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].filename, "a.wav");

        // Clearing both lists filters nothing.
        let mut all = SearchParams::new("");
        all.categories = Vec::new();
        assert_eq!(db.search(&all).unwrap().len(), 3);
    }

    #[test]
    fn bpm_sorts_with_unmarked_rows_last_and_round_trips() {
        let mut db = db();
        let mut a = row("C:/lib/Kick_90BPM.wav", "Drums");
        a.bpm = Some(90.0);
        let mut b = row("C:/lib/Kick_140BPM.wav", "Drums");
        b.bpm = Some(140.0);
        let c = row("C:/lib/Kick_unnamed.wav", "Drums");
        db.upsert_batch(&[a, b, c]).unwrap();

        let stored = db.get_by_path("C:/lib/Kick_90BPM.wav").unwrap().unwrap();
        assert_eq!(stored.bpm, Some(90.0));

        for dir in [SortDir::Asc, SortDir::Desc] {
            let mut params = SearchParams::new("");
            params.sort_by = Some(SortColumn::Bpm);
            params.sort_dir = Some(dir);
            let hits = db.search(&params).unwrap();
            assert_eq!(hits[2].bpm, None, "unmarked rows sort last ({dir:?})");
            let first = hits[0].bpm.unwrap();
            let second = hits[1].bpm.unwrap();
            match dir {
                SortDir::Asc => assert!(first < second),
                SortDir::Desc => assert!(first > second),
            }
            assert_eq!(hits.len(), 3);
        }
    }

    #[test]
    fn facets_group_by_category_and_subcategory() {
        let mut db = db();
        let mut a = row("C:/lib/a.wav", "Weapons");
        a.subcategory = Some("Firearms".into());
        let mut b = row("C:/lib/b.wav", "Weapons");
        b.subcategory = Some("Firearms".into());
        let c = row("C:/lib/c.wav", "Foley");
        db.upsert_batch(&[a, b, c]).unwrap();
        let facets = db.facets().unwrap();
        assert_eq!(facets[0].category, "Weapons");
        assert_eq!(facets[0].count, 2);
    }

    #[test]
    fn row_from_parts_computes_the_relative_folder() {
        let meta = AudioMeta {
            duration_s: Some(0.5),
            channels: Some(1),
            sample_rate: Some(48_000),
            filesize: 42,
            mtime_ns: 7,
            decoded_head: false,
        };
        let classification = Classifier::new().classify(r"C:\lib\Foley\door_slam.wav");
        let built = row_from_parts(
            Path::new(r"C:\lib"),
            Path::new(r"C:\lib\Foley\door_slam.wav"),
            &meta,
            &classification,
        );
        assert_eq!(built.folder, "Foley");
        assert_eq!(built.filename, "door_slam.wav");
        assert_eq!(built.duration_s, Some(0.5));
        assert_ne!(classification.category, "Uncategorized");
    }

    #[test]
    fn uncategorized_rows_still_round_trip() {
        let mut db = db();
        let mut r = row("C:/lib/zzz.wav", "Uncategorized");
        r.matched_terms = Vec::new();
        r.confidence = 0.0;
        db.upsert_batch(&[r]).unwrap();
        let sound = db.get_by_path("C:/lib/zzz.wav").unwrap().unwrap();
        assert_eq!(sound.category, "Uncategorized");
        assert!(sound.matched_terms.is_empty());
        assert_eq!(sound.confidence, 0.0);
        let _ = Classification::uncategorized();
    }
}
