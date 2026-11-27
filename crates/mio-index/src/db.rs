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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

use crate::error::{IndexError, Result};
use crate::folders::{build_folder_tree, folder_range, relative_folder, FolderNode};

/// Current schema version, stored in `meta`. Bump when a migration is added.
///
/// 2 added `bpm`; 3 added metadata and native UCS fields; 4 adds UCS fields to FTS.
/// Rule-only changes use
/// the dedicated backfill keys below rather than bumping this structural version.
pub const SCHEMA_VERSION: i64 = 4;

/// `meta` key recording which version of the tempo rule last touched the `bpm`
/// column, and the current version.
///
/// The value is the rule, not the run: rows the previous rule left NULL stay NULL
/// through an incremental index (their files have not changed), so a wider rule has
/// to be told apart from the narrower one that filled the column before it.
pub const BPM_BACKFILL_KEY: &str = "bpm_rule";
pub const BPM_BACKFILL: &str = "2";
pub const METADATA_BACKFILL_KEY: &str = "metadata_rule";
pub const METADATA_BACKFILL: &str = "8";

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
    pub description: Option<String>,
    pub ucs_category: Option<String>,
    pub ucs_subcategory: Option<String>,
    pub ucs_cat_id: Option<String>,
    pub ucs_manual: bool,
    /// Folder relative to the library root, `""` at the root.
    pub folder: String,
    pub mtime_ns: i64,
    /// Tempo stated in the file name, or `None` when it states none. See
    /// [`crate::bpm`]: this is never guessed from the audio.
    pub bpm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetadataCandidate {
    pub path: String,
    pub filesize: u64,
    pub mtime_ns: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetadataUpdate {
    pub path: String,
    pub filesize: u64,
    pub mtime_ns: i64,
    pub description: Option<String>,
    pub classification: crate::classify::Classification,
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
    pub description: Option<String>,
    pub description_count: Option<u32>,
    pub ucs_category: Option<String>,
    pub ucs_subcategory: Option<String>,
    pub ucs_cat_id: Option<String>,
    pub ucs_manual: bool,
    pub favorite: bool,
    pub category_manual: bool,
    pub folder: Option<String>,
    pub bpm: Option<f64>,
}

/// The fields the app's undo record needs, without decoding display-only data.
#[derive(Debug, Clone, PartialEq)]
pub struct EditState {
    pub path: String,
    pub favorite: bool,
    pub category: String,
    pub subcategory: Option<String>,
    pub category_manual: bool,
    pub ucs_category: Option<String>,
    pub ucs_subcategory: Option<String>,
    pub ucs_cat_id: Option<String>,
    pub ucs_manual: bool,
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
    Description,
    UcsCategory,
    UcsSubcategory,
}

impl SortColumn {
    fn uses_metadata_sort_index(self) -> bool {
        matches!(self, Self::Description | Self::UcsCategory | Self::UcsSubcategory)
    }

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
            SortColumn::Description => "s.description COLLATE NOCASE",
            SortColumn::UcsCategory => "s.ucs_category COLLATE NOCASE",
            SortColumn::UcsSubcategory => "s.ucs_subcategory COLLATE NOCASE",
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
    /// Native UCS category filters, kept separate from Mio's legacy projection.
    pub ucs_categories: Vec<String>,
    pub ucs_subcategories: Vec<String>,
    pub ucs_cat_ids: Vec<String>,
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

/// One native UCS `(category, subcategory, count)` row for the two-level rail.
#[derive(Debug, Clone, PartialEq)]
pub struct UcsFacet {
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
}

static LIBRARY_LOCKS: OnceLock<Mutex<std::collections::HashMap<PathBuf, Weak<std::fs::File>>>> =
    OnceLock::new();

/// A stable 64-bit hash for the cross-process lock file name.
///
/// `DefaultHasher` is explicitly documented as unspecified across Rust releases,
/// so two builds on the same machine could compute different names for the same
/// library and both take a lock — "already open by another Mio instance" would
/// silently stop working on a mixed-version install. FNV-1a is small, has no
/// dependency, and never changes: the lock file for a given canonical path is the
/// same in every build forever, which is the property this needs.
fn stable_lock_hash(path: &Path) -> u64 {
    // FNV-1a over the path's OS encoding, so case and separators count exactly as
    // the filesystem reports them.
    let bytes: Vec<u8> = path
        .to_string_lossy()
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

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
    let lock_dir = std::env::temp_dir().join("mio-sound-manager-locks");
    std::fs::create_dir_all(&lock_dir)?;
    let path = lock_dir.join(format!("{:016x}.lock", stable_lock_hash(&canonical)));
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

#[derive(Debug, Clone, Default)]
pub struct BulkTimings {
    pub secondary_indexes_ms: f64,
    pub fts_ms: f64,
    pub trigram_ms: f64,
    pub triggers_ms: f64,
}

impl IndexDb {
    /// Open (or create) the index inside `root`. Runs schema creation and any
    /// pending migration.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_backfill(root, true)
    }

    /// Open the schema without running a potentially large one-time metadata
    /// backfill. Desktop startup uses this path so the first WebView frame is not
    /// held behind probing hundreds of thousands of files; the normal index pass
    /// performs the pending backfill on its worker connection.
    pub fn open_fast(root: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_backfill(root, false)
    }

    fn open_with_backfill(root: impl AsRef<Path>, backfill: bool) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        if !root.is_dir() {
            return Err(IndexError::BadRoot(root.display().to_string()));
        }
        let library_lock = acquire_library_lock(&root)?;
        Self::adopt_legacy_file(&root)?;
        let path = root.join(DB_FILENAME);
        let conn = Connection::open(&path)?;
        let mut db = Self::prepare(conn, root, backfill)?;
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
        Self::prepare(conn, root.as_ref().to_path_buf(), true)
    }

    fn prepare(conn: Connection, root: PathBuf, backfill: bool) -> Result<Self> {
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
        };
        db.create_base_schema()?;
        db.migrate(backfill)?;
        if db.get_meta("fts_version")?.as_deref() != Some("3") {
            db.conn.execute_batch(
                "DROP TABLE IF EXISTS sounds_fts;
                 DROP TRIGGER IF EXISTS sounds_ai;
                 DROP TRIGGER IF EXISTS sounds_ad;
                 DROP TRIGGER IF EXISTS sounds_au;",
            )?;
            db.create_fts()?;
            db.rebuild_fts()?;
            db.set_meta("fts_version", "3")?;
        } else {
            db.create_fts()?;
        }
        // A previous bulk load died before `end_bulk`: the FTS content does not
        // match `sounds` and the secondary indexes are missing. Rebuild both now
        // rather than serving stale search results or a table with no indexes.
        if db.get_meta("fts_dirty")?.as_deref() == Some("1") {
            db.end_bulk()?;
        }
        if db.get_meta("trigram_version")?.as_deref() != Some("1") {
            db.rebuild_trigram_fts()?;
            db.set_meta("trigram_version", "1")?;
        }
        Ok(db)
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
             CREATE TEMP TABLE IF NOT EXISTS _scan_skipped (
                 path TEXT PRIMARY KEY,
                 path_end TEXT NOT NULL
             ) WITHOUT ROWID;
             DELETE FROM _scan_files;",
        )?;
        self.conn.execute("DELETE FROM _scan_skipped", [])?;
        Ok(())
    }

    pub fn stage_skipped_dirs(&mut self, dirs: &[crate::probe::SkippedDir]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO _scan_skipped(path, path_end) VALUES (?1, ?2)",
            )?;
            for dir in dirs {
                let (_, path_end) = crate::folders::folder_range(&dir.path);
                stmt.execute(params![dir.path, path_end])?;
            }
        }
        tx.commit()?;
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

    /// Snapshot changed paths once for both progress totals and probe batches.
    pub(crate) fn stage_scan_changes(&self) -> Result<usize> {
        self.conn.execute_batch(
            "CREATE TEMP TABLE _scan_changed (
                 path TEXT PRIMARY KEY,
                 filesize INTEGER NOT NULL,
                 mtime_ns INTEGER NOT NULL
             ) WITHOUT ROWID;",
        )?;
        Ok(self.conn.execute(
            "INSERT INTO _scan_changed
             SELECT f.path, f.filesize, f.mtime_ns
             FROM _scan_files f LEFT JOIN sounds s ON s.path = f.path
             WHERE s.path IS NULL OR COALESCE(s.mtime_ns, 0) = 0
               OR s.mtime_ns <> f.mtime_ns OR COALESCE(s.filesize, 0) <> f.filesize",
            [],
        )?)
    }

    pub(crate) fn scan_batch(
        &self,
        after_path: Option<&str>,
        limit: usize,
        incremental: bool,
    ) -> Result<Vec<ScannedPath>> {
        use rusqlite::types::Value;
        let table = if incremental { "_scan_changed" } else { "_scan_files" };
        let (sql, args) = match after_path {
            Some(path) => (
                format!("SELECT path, filesize, mtime_ns FROM {table} WHERE path > ?1 ORDER BY path LIMIT ?2"),
                vec![Value::Text(path.to_owned()), Value::Integer(limit as i64)],
            ),
            None => (
                format!("SELECT path, filesize, mtime_ns FROM {table} ORDER BY path LIMIT ?1"),
                vec![Value::Integer(limit as i64)],
            ),
        };
        let mut stmt = self.conn.prepare_cached(&sql)?;
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

    pub fn prune_scan_missing(&mut self) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let removed = tx.execute(
            "DELETE FROM sounds
             WHERE NOT EXISTS (SELECT 1 FROM _scan_files f WHERE f.path = sounds.path)
               AND NOT EXISTS (
                   SELECT 1 FROM _scan_skipped d
                   WHERE sounds.path >= d.path AND sounds.path < d.path_end
               )",
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
        self.conn.execute_batch(
            "DROP TABLE IF EXISTS _scan_changed;
             DROP TABLE IF EXISTS _scan_files;
                 DROP TABLE IF EXISTS _scan_skipped;",
        )?;
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
                folder TEXT,
                description TEXT,
                 ucs_category TEXT,
                 ucs_subcategory TEXT,
                 ucs_cat_id TEXT,
                 ucs_manual INTEGER NOT NULL DEFAULT 0,
                 metadata_indexed INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_duration ON sounds(duration);
            CREATE INDEX IF NOT EXISTS idx_channels ON sounds(channels);
            CREATE INDEX IF NOT EXISTS idx_category ON sounds(category);
            CREATE INDEX IF NOT EXISTS idx_category_subcategory ON sounds(category, subcategory);
            CREATE INDEX IF NOT EXISTS idx_confidence ON sounds(confidence);
            CREATE INDEX IF NOT EXISTS idx_favorite ON sounds(favorite);
            CREATE INDEX IF NOT EXISTS idx_filename ON sounds(filename COLLATE NOCASE);
            CREATE INDEX IF NOT EXISTS idx_filesize ON sounds(filesize);

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
    fn migrate(&self, backfill: bool) -> Result<()> {
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
        for column in [
            "description",
            "ucs_category",
            "ucs_subcategory",
            "ucs_cat_id",
        ] {
            if !has(column) {
                self.conn
                    .execute_batch(&format!("ALTER TABLE sounds ADD COLUMN {column} TEXT"))?;
            }
        }
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_ucs_category ON sounds(ucs_category);
             CREATE INDEX IF NOT EXISTS idx_ucs_category_subcategory
                 ON sounds(ucs_category, ucs_subcategory);
             -- The CatID is a filter of its own (the third level under UCS) and it is
             -- also grouped by once per sidebar refresh for the dialog's counts.
             -- Without this index both were whole-table scans: 5.9 s per page and
             -- 447 ms per refresh on the reference library.
             CREATE INDEX IF NOT EXISTS idx_ucs_cat_id ON sounds(ucs_cat_id);
             CREATE INDEX IF NOT EXISTS idx_description ON sounds(description);",
        )?;
        // Filename ties stay ascending in both directions, so a reverse scan of
        // the ascending index cannot serve the descending sort.
        for column in ["description", "ucs_category", "ucs_subcategory"] {
            for dir in ["ASC", "DESC"] {
                self.conn.execute_batch(&format!(
                    "CREATE INDEX IF NOT EXISTS idx_sort_{column}_{dir}
                     ON sounds({column} COLLATE NOCASE {dir}, filename COLLATE NOCASE ASC);"
                ))?;
            }
        }
        if !has("metadata_indexed") {
            self.conn.execute_batch(
                "ALTER TABLE sounds ADD COLUMN metadata_indexed INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        if !has("ucs_manual") {
            self.conn.execute_batch(
                "ALTER TABLE sounds ADD COLUMN ucs_manual INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        if backfill && self.get_meta(METADATA_BACKFILL_KEY)?.as_deref() != Some(METADATA_BACKFILL) {
            self.backfill_metadata()?;
            self.set_meta(METADATA_BACKFILL_KEY, METADATA_BACKFILL)?;
        }
        // Keyed on `meta` rather than on the column, because adding the column and
        // filling it can be separated by a build that was interrupted: a database
        // whose column exists but whose values are all NULL still needs the pass,
        // and one that has already been through it must not repeat it on every
        // open. The recorded number is the version of the *rule*, not of the pass:
        // a wider extractor has to revisit the rows a narrower one left NULL, and
        // an incremental index would never touch them because the files (and their
        // mtimes) have not changed.
        if self.get_meta(BPM_BACKFILL_KEY)?.as_deref() != Some(BPM_BACKFILL) {
            self.backfill_bpm()?;
            self.set_meta(BPM_BACKFILL_KEY, BPM_BACKFILL)?;
        }

        // The incremental-reindex lookup is `WHERE path = ?`, already covered by
        // the UNIQUE index on path. This partial index answers "what changed
        // since" without a scan, which is what the prune pass wants.
        self.conn
            .execute_batch("CREATE INDEX IF NOT EXISTS idx_mtime ON sounds(mtime_ns);")?;
        self.conn
            .execute_batch("CREATE INDEX IF NOT EXISTS idx_bpm ON sounds(bpm);")?;
        // The paged list sorts by filename (NOCASE) by default and can sort by
        // filesize; without these indexes each 500-row page re-sorted the whole
        // table. Created on every open so libraries indexed before they existed
        // still get them.
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_filename ON sounds(filename COLLATE NOCASE);",
        )?;
        self.conn
            .execute_batch("CREATE INDEX IF NOT EXISTS idx_filesize ON sounds(filesize);")?;

        // A library written by a newer build may carry columns and migrations
        // this build does not know about. Refusing is honest; opening it and
        // rewriting rows would silently drop whatever the newer build added.
        if let Some(stored) = self.get_meta("schema_version")? {
            if let Ok(stored) = stored.parse::<i64>() {
                if stored > SCHEMA_VERSION {
                    return Err(IndexError::Db(format!(
                        "this library was indexed by a newer Mio (schema {stored}, this build understands {SCHEMA_VERSION}); update the app before opening it"
                    )));
                }
            }
        }
        self.set_meta("schema_version", &SCHEMA_VERSION.to_string())?;
        Ok(())
    }

    /// Fill `bpm` for every existing row from its stored file name, folder and
    /// duration.
    ///
    /// Runs on the schema upgrade and again when [`BPM_BACKFILL`] is newer than what
    /// the database recorded: the extraction rule is part of the value, so a wider
    /// rule has to revisit rows the narrower one left NULL.
    ///
    /// The FTS triggers are suspended for the duration: `bpm` is not one of the
    /// searchable columns, so the tokenizer has nothing to update, and letting
    /// `sounds_au` run per row would pay a tokenizer pass 280k times over a column
    /// FTS does not read.
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
        let rows: Vec<(i64, String, String, Option<f64>)> = {
            let mut stmt =
                tx.prepare("SELECT id, filename, COALESCE(folder, ''), duration FROM sounds")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            out
        };
        {
            let mut stmt = tx.prepare("UPDATE sounds SET bpm = ?1 WHERE id = ?2")?;
            for (id, filename, folder, duration) in &rows {
                match crate::bpm::tempo(filename, folder, *duration) {
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

        // `prepare` recreates the current FTS schema and triggers after all
        // migrations have run. Calling it here would fail on an older FTS table
        // whose column set predates `description`.
        Ok(())
    }

    /// Populate fields introduced after the original index was written.
    ///
    /// This deliberately runs before `create_fts`: old databases have an FTS
    /// table with no `description` column, while the normal row triggers still
    /// know how to maintain the old table during the migration. The FTS schema
    /// is upgraded immediately afterwards and rebuilt once.
    fn backfill_metadata(&self) -> Result<()> {
        let rows: Vec<(
            String,
            bool,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = {
            let mut stmt = self.conn.prepare(
                "SELECT path, category_manual, description, ucs_category, ucs_subcategory, ucs_cat_id FROM sounds",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, i64>(1)? == 1,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let classifier = crate::classify::Classifier::new();
        let mut updates = Vec::new();
        for (path, category_manual, description, ucs_category, ucs_subcategory, ucs_cat_id) in rows
        {
            let description_source = std::path::Path::new(&path)
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "wav" | "wave" | "mp3"))
                .unwrap_or(false);
            let needs_description = description_source;
            let needs_ucs = ucs_category.is_none();
            if !needs_description && !needs_ucs && category_manual {
                continue;
            }
            let meta = if needs_description || needs_ucs {
                crate::probe::probe_file(std::path::Path::new(&path))
            } else {
                crate::probe::AudioMeta::default()
            };
            let effective_description = if description_source {
                meta.description.as_deref()
            } else {
                meta.description.as_deref().or(description.as_deref())
            };
            let classification = classifier.classify_with_metadata(
                &path,
                effective_description,
                meta.ucs_category.as_deref(),
                meta.ucs_subcategory.as_deref(),
            );
            updates.push((
                path,
                if description_source { meta.description } else { description.or(meta.description) },
                category_manual,
                classification,
                ucs_category,
                ucs_subcategory,
                ucs_cat_id,
            ));
        }

        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE sounds SET description = ?1,
                 category = CASE WHEN ?2 THEN category ELSE ?3 END,
                 subcategory = CASE WHEN ?2 THEN subcategory ELSE ?4 END,
                 confidence = CASE WHEN ?2 THEN confidence ELSE ?5 END,
                 matched_terms = CASE WHEN ?2 THEN matched_terms ELSE ?6 END,
                 ucs_category = ?7, ucs_subcategory = ?8, ucs_cat_id = ?9 WHERE path = ?10",
            )?;
            for (
                path,
                description,
                category_manual,
                classification,
                old_ucs_category,
                old_ucs_subcategory,
                old_ucs_cat_id,
            ) in updates
            {
                stmt.execute(params![
                    description,
                    category_manual,
                    classification.category,
                    classification.subcategory,
                    classification.confidence,
                    serde_json::to_string(&classification.matched_terms)
                        .unwrap_or_else(|_| "[]".into()),
                    classification.ucs_category.or(old_ucs_category),
                    classification.ucs_subcategory.or(old_ucs_subcategory),
                    classification.ucs_cat_id.or(old_ucs_cat_id),
                    path
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// FTS5 external-content table over the searchable filename/category/UCS
    /// columns, plus the
    /// three triggers that keep it correct.
    fn create_fts(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS sounds_fts USING fts5(
                 filename,
                 category,
                 subcategory,
                 folder,
                 description,
                 ucs_category,
                 ucs_subcategory,
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
              INSERT INTO sounds_fts(rowid, filename, category, subcategory, folder, description, ucs_category, ucs_subcategory)
              VALUES (new.id, new.filename, new.category, new.subcategory, new.folder, new.description, new.ucs_category, new.ucs_subcategory);
              INSERT INTO sounds_trigram(rowid, filename) VALUES (new.id, new.filename);
            END;

            CREATE TRIGGER IF NOT EXISTS sounds_ad AFTER DELETE ON sounds BEGIN
              INSERT INTO sounds_fts(sounds_fts, rowid, filename, category, subcategory, folder, description, ucs_category, ucs_subcategory)
              VALUES ('delete', old.id, old.filename, old.category, old.subcategory, old.folder, old.description, old.ucs_category, old.ucs_subcategory);
              INSERT INTO sounds_trigram(sounds_trigram, rowid, filename)
              VALUES ('delete', old.id, old.filename);
            END;

            CREATE TRIGGER IF NOT EXISTS sounds_au AFTER UPDATE ON sounds BEGIN
              INSERT INTO sounds_fts(sounds_fts, rowid, filename, category, subcategory, folder, description, ucs_category, ucs_subcategory)
              VALUES ('delete', old.id, old.filename, old.category, old.subcategory, old.folder, old.description, old.ucs_category, old.ucs_subcategory);
              INSERT INTO sounds_trigram(sounds_trigram, rowid, filename)
              VALUES ('delete', old.id, old.filename);
              INSERT INTO sounds_fts(rowid, filename, category, subcategory, folder, description, ucs_category, ucs_subcategory)
              VALUES (new.id, new.filename, new.category, new.subcategory, new.folder, new.description, new.ucs_category, new.ucs_subcategory);
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
                     description, ucs_category, ucs_subcategory, ucs_cat_id, ucs_manual,
                     metadata_indexed, mtime_ns, indexed_at, bpm)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
                ON CONFLICT(path) DO UPDATE SET
                    filename = excluded.filename,
                    folder = excluded.folder,
                    duration = excluded.duration,
                    channels = excluded.channels,
                    sample_rate = excluded.sample_rate,
                    filesize = excluded.filesize,
                     description = CASE WHEN sounds.mtime_ns = excluded.mtime_ns
                                             AND sounds.filesize = excluded.filesize
                                        THEN COALESCE(sounds.description, excluded.description)
                                        ELSE excluded.description END,
                     ucs_category = CASE WHEN sounds.ucs_manual = 1 THEN sounds.ucs_category
                                         WHEN sounds.mtime_ns = excluded.mtime_ns
                                             AND sounds.filesize = excluded.filesize
                                        THEN COALESCE(sounds.ucs_category, excluded.ucs_category)
                                        ELSE excluded.ucs_category END,
                     ucs_subcategory = CASE WHEN sounds.ucs_manual = 1 THEN sounds.ucs_subcategory
                                             WHEN sounds.mtime_ns = excluded.mtime_ns
                                             AND sounds.filesize = excluded.filesize
                                        THEN COALESCE(sounds.ucs_subcategory, excluded.ucs_subcategory)
                                        ELSE excluded.ucs_subcategory END,
                     ucs_cat_id = CASE WHEN sounds.ucs_manual = 1 THEN sounds.ucs_cat_id
                                         WHEN sounds.mtime_ns = excluded.mtime_ns
                                             AND sounds.filesize = excluded.filesize
                                        THEN COALESCE(sounds.ucs_cat_id, excluded.ucs_cat_id)
                                        ELSE excluded.ucs_cat_id END,
                     ucs_manual = sounds.ucs_manual,
                     metadata_indexed = CASE WHEN sounds.mtime_ns = excluded.mtime_ns
                                                  AND sounds.filesize = excluded.filesize
                                             THEN sounds.metadata_indexed ELSE 0 END,
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
                    row.description,
                    row.ucs_category,
                    row.ucs_subcategory,
                     row.ucs_cat_id,
                    row.ucs_manual as i64,
                    0i64,
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

    pub fn metadata_candidates(&self, limit: usize) -> Result<Vec<MetadataCandidate>> {
        self.metadata_candidates_after(None, limit)
    }

    pub fn metadata_candidates_after(
        &self,
        after_path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MetadataCandidate>> {
        let (sql, args): (&str, Vec<rusqlite::types::Value>) = match after_path {
            Some(path) => (
                "SELECT path, filesize, mtime_ns FROM sounds
                 WHERE metadata_indexed = 0 AND path > ?1
                   AND (lower(filename) LIKE '%.wav' OR lower(filename) LIKE '%.wave'
                        OR lower(filename) LIKE '%.mp3')
                 ORDER BY path LIMIT ?2",
                vec![
                    rusqlite::types::Value::Text(path.to_owned()),
                    rusqlite::types::Value::Integer(limit as i64),
                ],
            ),
            None => (
                "SELECT path, filesize, mtime_ns FROM sounds
                 WHERE metadata_indexed = 0
                   AND (lower(filename) LIKE '%.wav' OR lower(filename) LIKE '%.wave'
                        OR lower(filename) LIKE '%.mp3')
                 ORDER BY path LIMIT ?1",
                vec![rusqlite::types::Value::Integer(limit as i64)],
            ),
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |row| {
            Ok(MetadataCandidate {
                path: row.get(0)?,
                filesize: row.get::<_, i64>(1)? as u64,
                mtime_ns: row.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn metadata_candidate_count(&self) -> Result<usize> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM sounds
             WHERE metadata_indexed = 0
               AND (lower(filename) LIKE '%.wav' OR lower(filename) LIKE '%.wave'
                    OR lower(filename) LIKE '%.mp3')",
            [],
            |row| row.get::<_, i64>(0),
        )? as usize)
    }

    /// Schedule a new extraction rule from the metadata worker. Commit the
    /// version and pending bits together, so reopening can resume a partial pass.
    pub fn prepare_metadata_pass(&mut self) -> Result<()> {
        if self.get_meta(METADATA_BACKFILL_KEY)?.as_deref() == Some(METADATA_BACKFILL) {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE sounds SET metadata_indexed = 0 WHERE metadata_indexed <> 0
               AND (lower(filename) LIKE '%.wav' OR lower(filename) LIKE '%.wave'
                    OR lower(filename) LIKE '%.mp3')",
            [],
        )?;
        tx.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![METADATA_BACKFILL_KEY, METADATA_BACKFILL],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn mark_metadata_failed(&mut self, candidate: &MetadataCandidate) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE sounds SET metadata_indexed = 1
             WHERE path = ?1 AND mtime_ns = ?2 AND filesize = ?3",
            params![candidate.path, candidate.mtime_ns, candidate.filesize as i64],
        )?;
        Ok(changed != 0)
    }

    pub fn apply_metadata_batch(&mut self, updates: &[MetadataUpdate]) -> Result<u64> {
        if updates.is_empty() {
            return Ok(0);
        }
        let tx = self.conn.transaction()?;
        let mut stmt = tx.prepare(
            "UPDATE sounds SET description = ?1,
                 ucs_category = CASE WHEN ucs_manual = 1 THEN ucs_category ELSE ?2 END,
                 ucs_subcategory = CASE WHEN ucs_manual = 1 THEN ucs_subcategory ELSE ?3 END,
                 ucs_cat_id = CASE WHEN ucs_manual = 1 THEN ucs_cat_id ELSE ?4 END,
                category = CASE WHEN category_manual = 1 THEN category ELSE ?5 END,
                subcategory = CASE WHEN category_manual = 1 THEN subcategory ELSE ?6 END,
                confidence = CASE WHEN category_manual = 1 THEN confidence ELSE ?7 END,
                matched_terms = CASE WHEN category_manual = 1 THEN matched_terms ELSE ?8 END,
                metadata_indexed = 1
             WHERE path = ?9 AND mtime_ns = ?10 AND filesize = ?11",
        )?;
        let mut changed = 0u64;
        for update in updates {
            let terms = serde_json::to_string(&update.classification.matched_terms)
                .unwrap_or_else(|_| "[]".into());
            changed += stmt.execute(params![
                update.description,
                update.classification.ucs_category,
                update.classification.ucs_subcategory,
                update.classification.ucs_cat_id,
                update.classification.category,
                update.classification.subcategory,
                update.classification.confidence,
                terms,
                update.path,
                update.mtime_ns,
                update.filesize as i64,
            ])? as u64;
        }
        drop(stmt);
        if changed > 0 {
            bump_index_version_in(&tx)?;
        }
        tx.commit()?;
        Ok(changed)
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
             DROP INDEX IF EXISTS idx_ucs_cat_id;
             DROP INDEX IF EXISTS idx_mtime;
             DROP INDEX IF EXISTS idx_bpm;",
        )?;
        self.set_meta("fts_dirty", "1")?;
        Ok(())
    }

    /// Restore the triggers and indexes, rebuild `sounds_fts` from `sounds`.
    pub fn end_bulk(&mut self) -> Result<BulkTimings> {
        use std::time::Instant;

        let indexes = Instant::now();
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_duration ON sounds(duration);
             CREATE INDEX IF NOT EXISTS idx_channels ON sounds(channels);
             CREATE INDEX IF NOT EXISTS idx_category ON sounds(category);
             CREATE INDEX IF NOT EXISTS idx_category_subcategory ON sounds(category, subcategory);
             CREATE INDEX IF NOT EXISTS idx_confidence ON sounds(confidence);
             CREATE INDEX IF NOT EXISTS idx_favorite ON sounds(favorite);
             CREATE INDEX IF NOT EXISTS idx_ucs_cat_id ON sounds(ucs_cat_id);
             CREATE INDEX IF NOT EXISTS idx_filename ON sounds(filename COLLATE NOCASE);
             CREATE INDEX IF NOT EXISTS idx_filesize ON sounds(filesize);
             CREATE INDEX IF NOT EXISTS idx_mtime ON sounds(mtime_ns);
             CREATE INDEX IF NOT EXISTS idx_bpm ON sounds(bpm);",
        )?;
        let triggers = Instant::now();
        self.create_fts()?;
        let fts = Instant::now();
        self.rebuild_fts()?;
        let trigram = Instant::now();
        self.rebuild_trigram_fts()?;
        let finished = Instant::now();
        self.set_meta("fts_dirty", "0")?;
        Ok(BulkTimings {
            secondary_indexes_ms: (triggers - indexes).as_secs_f64() * 1000.0,
            triggers_ms: (fts - triggers).as_secs_f64() * 1000.0,
            fts_ms: (trigram - fts).as_secs_f64() * 1000.0,
            trigram_ms: (finished - trigram).as_secs_f64() * 1000.0,
        })
    }

    pub fn count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM sounds", [], |r| r.get::<_, i64>(0))?
            as u64)
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
    ///
    /// The `favorite != ?` predicate makes the returned count mean "rows that
    /// actually changed". SQLite's `changes()` counts rows *matched*, so without it
    /// starring an already-starred row reported a change, fired the FTS triggers for
    /// nothing, and pushed an undo entry whose inverse was a no-op.
    pub fn set_favorite_many(&mut self, paths: &[String], favorite: bool) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let changed = {
            let mut stmt = tx.prepare(
                "UPDATE sounds SET favorite = ?1 WHERE path = ?2 AND favorite != ?1",
            )?;
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
    ///
    /// The predicate carries the same "only rows that change" meaning as the
    /// favourite one: re-applying a category a row already has is not an edit, and
    /// must not report one.
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
                 WHERE path = ?3
                   AND (category != ?1 OR subcategory IS NOT ?2
                        OR confidence != 1 OR category_manual != 1)",
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

    /// Assign an official UCS label and the corresponding 22-category projection.
    pub fn set_ucs_category_many(
        &mut self,
        paths: &[String],
        category: &str,
        subcategory: Option<&str>,
        cat_id: &str,
        projected_category: &str,
        projected_subcategory: Option<&str>,
    ) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let mut stmt = tx.prepare(
            "UPDATE sounds SET ucs_category = ?1, ucs_subcategory = ?2, ucs_cat_id = ?3,
              ucs_manual = 1, category = ?4, subcategory = ?5, confidence = 1, category_manual = 1
             WHERE path = ?6
               AND (ucs_category IS NOT ?1 OR ucs_subcategory IS NOT ?2
                    OR ucs_cat_id IS NOT ?3 OR category IS NOT ?4 OR subcategory IS NOT ?5
                    OR confidence != 1 OR category_manual != 1 OR ucs_manual != 1)",
        )?;
        let mut changed = 0;
        for path in paths {
            changed += stmt.execute(params![
                category,
                subcategory,
                cat_id,
                projected_category,
                projected_subcategory,
                path
            ])? as u64;
        }
        drop(stmt);
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
    ///
    /// Only rows that are not already clear are written, so the count means "how
    /// many changed" and clearing an uncategorised selection is not an edit.
    pub fn clear_category_many(&mut self, paths: &[String]) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let changed = {
            let mut stmt = tx.prepare(
                "UPDATE sounds
                 SET category = 'Uncategorized', subcategory = NULL, confidence = 0,
                     matched_terms = NULL, category_manual = 0
                 WHERE path = ?1
                   AND (category != 'Uncategorized' OR subcategory IS NOT NULL
                        OR confidence != 0 OR category_manual != 0 OR matched_terms IS NOT NULL)",
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
    pub fn restore_category_and_ucs_many(&mut self, states: &[EditState]) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let mut stmt = tx.prepare(
            "UPDATE sounds SET category = ?1, subcategory = ?2, category_manual = ?3,
             ucs_category = ?4, ucs_subcategory = ?5, ucs_cat_id = ?6, ucs_manual = ?7
             WHERE path = ?8",
        )?;
        let mut changed = 0;
        for state in states {
            changed += stmt.execute(params![
                state.category,
                state.subcategory,
                state.category_manual as i64,
                state.ucs_category,
                state.ucs_subcategory,
                state.ucs_cat_id,
                state.ucs_manual as i64,
                state.path
            ])? as u64;
        }
        drop(stmt);
        tx.commit()?;
        Ok(changed)
    }

    pub fn search(&self, params: &SearchParams) -> Result<Vec<Sound>> {
        let (sql, args) = Self::build_search_sql(params, false, false);
        let mut out = self.read_sounds(&sql, &args)?;
        if out.is_empty() && !params.query.trim().is_empty() {
            if trigram_query(&params.query).is_some() && self.count_search_mode(params, false)? == 0
            {
                let (sql, args) = Self::build_search_sql(params, false, true);
                out = self.read_sounds(&sql, &args)?;
            }
        }
        Ok(out)
    }

    pub fn set_favorite_matching(
        &mut self,
        params: &SearchParams,
        excluded: &[String],
        favorite: bool,
    ) -> Result<(Vec<EditState>, u64)> {
        self.update_matching(
            params,
            excluded,
            "favorite = ?",
            vec![rusqlite::types::Value::Integer(favorite as i64)],
            "s.favorite != ?",
            vec![rusqlite::types::Value::Integer(favorite as i64)],
        )
    }

    pub fn set_category_matching(
        &mut self,
        params: &SearchParams,
        excluded: &[String],
        category: &str,
        subcategory: Option<&str>,
    ) -> Result<(Vec<EditState>, u64)> {
        use rusqlite::types::Value;

        self.update_matching(
            params,
            excluded,
            "category = ?, subcategory = ?, confidence = 1, category_manual = 1",
            vec![
                Value::Text(category.to_string()),
                subcategory
                    .map(|value| Value::Text(value.to_string()))
                    .unwrap_or(Value::Null),
            ],
            "s.category IS NOT ? OR s.subcategory IS NOT ? OR s.confidence != 1 OR s.category_manual != 1",
            vec![
                Value::Text(category.to_string()),
                subcategory
                    .map(|value| Value::Text(value.to_string()))
                    .unwrap_or(Value::Null),
            ],
        )
    }

    pub fn set_ucs_category_matching(
        &mut self,
        params: &SearchParams,
        excluded: &[String],
        category: &str,
        subcategory: Option<&str>,
        cat_id: &str,
        projected_category: &str,
        projected_subcategory: Option<&str>,
    ) -> Result<(Vec<EditState>, u64)> {
        use rusqlite::types::Value;
        self.update_matching(
            params,
            excluded,
            "ucs_category = ?, ucs_subcategory = ?, ucs_cat_id = ?, ucs_manual = 1,
             category = ?, subcategory = ?, confidence = 1, category_manual = 1",
            vec![
                Value::Text(category.to_string()),
                subcategory
                    .map(|v| Value::Text(v.to_string()))
                    .unwrap_or(Value::Null),
                Value::Text(cat_id.to_string()),
                Value::Text(projected_category.to_string()),
                projected_subcategory
                    .map(|v| Value::Text(v.to_string()))
                    .unwrap_or(Value::Null),
            ],
            "s.ucs_category IS NOT ? OR s.ucs_subcategory IS NOT ? OR s.ucs_cat_id IS NOT ?
             OR s.category IS NOT ? OR s.subcategory IS NOT ? OR s.confidence != 1
             OR s.category_manual != 1 OR s.ucs_manual != 1",
            vec![
                Value::Text(category.to_string()),
                subcategory
                    .map(|v| Value::Text(v.to_string()))
                    .unwrap_or(Value::Null),
                Value::Text(cat_id.to_string()),
                Value::Text(projected_category.to_string()),
                projected_subcategory
                    .map(|v| Value::Text(v.to_string()))
                    .unwrap_or(Value::Null),
            ],
        )
    }

    pub fn clear_category_matching(
        &mut self,
        params: &SearchParams,
        excluded: &[String],
    ) -> Result<(Vec<EditState>, u64)> {
        self.update_matching(
            params,
            excluded,
            "category = 'Uncategorized', subcategory = NULL, confidence = 0, \
             matched_terms = NULL, category_manual = 0",
            Vec::new(),
            "s.category IS NOT 'Uncategorized' OR s.subcategory IS NOT NULL OR s.confidence != 0 OR s.matched_terms IS NOT NULL OR s.category_manual != 0",
            Vec::new(),
        )
    }

    fn update_matching(
        &mut self,
        params: &SearchParams,
        excluded: &[String],
        assignment: &str,
        mut assignment_args: Vec<rusqlite::types::Value>,
        changed_where: &str,
        changed_args: Vec<rusqlite::types::Value>,
    ) -> Result<(Vec<EditState>, u64)> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (count_sql, count_args) =
            Self::build_search_sql_for(params, "COUNT(*)", true, false, &[], None);
        let count = tx.query_row(&count_sql, params_from_iter(count_args.iter()), |row| {
            row.get::<_, i64>(0)
        })?;
        let substring =
            count == 0 && !params.query.trim().is_empty() && trigram_query(&params.query).is_some();

        let (snapshot_sql, snapshot_args) = Self::build_search_sql_for(
            params,
            "s.path, s.favorite, s.category, s.subcategory, s.category_manual,
             s.ucs_category, s.ucs_subcategory, s.ucs_cat_id, s.ucs_manual",
            false,
            substring,
            excluded,
            Some((changed_where, &changed_args)),
        );
        let mut before = Vec::new();
        {
            let mut statement = tx.prepare(&snapshot_sql)?;
            let mut rows = statement.query(params_from_iter(snapshot_args.iter()))?;
            while let Some(row) = rows.next()? {
                before.push(EditState {
                    path: row.get(0)?,
                    favorite: row.get::<_, i64>(1)? == 1,
                    category: row.get(2)?,
                    subcategory: row.get(3)?,
                    category_manual: row.get::<_, i64>(4)? == 1,
                    ucs_category: row.get(5)?,
                    ucs_subcategory: row.get(6)?,
                    ucs_cat_id: row.get(7)?,
                    ucs_manual: row.get::<_, i64>(8)? == 1,
                });
            }
        }

        let (subquery, query_args) = Self::build_search_sql_for(
            params,
            "s.id",
            false,
            substring,
            excluded,
            Some((changed_where, &changed_args)),
        );
        assignment_args.extend(query_args);
        let sql = format!("UPDATE sounds SET {assignment} WHERE id IN ({subquery})");
        let changed = tx.execute(&sql, params_from_iter(assignment_args.iter()))? as u64;
        tx.commit()?;
        Ok((before, changed))
    }

    fn read_sounds(&self, sql: &str, args: &[rusqlite::types::Value]) -> Result<Vec<Sound>> {
        // `prepare_cached`, not `prepare`: the paged query is the same string
        // every scroll frame, and recompiling it per call showed up on the
        // 280k-library page bench as a constant tax on every fetch.
        let mut stmt = self.conn.prepare_cached(sql)?;
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
        let (sql, args) = Self::build_search_sql(params, true, substring);
        Ok(self
            .conn
            .query_row(&sql, params_from_iter(args.iter()), |r| r.get::<_, i64>(0))?
            as u64)
    }

    /// `(sql, bound parameters)` for both the search and the count, so the two
    /// can never drift apart.
    fn build_search_sql(
        params: &SearchParams,
        counting: bool,
        substring: bool,
    ) -> (String, Vec<rusqlite::types::Value>) {
        if !counting && params.sort_by.is_some() {
            // Compute the shared-description count only for the final page. It
            // is a correlated subquery and would otherwise run on rows skipped
            // by a temporary sort (for example after a selective FTS filter).
            let (page, args) = Self::build_search_sql_for(params, "s.id", false, substring, &[], None);
            let order = Self::search_order(params);
            let columns = SOUND_COLUMNS.replace(DESCRIPTION_COUNT_SQL, "counts.n");
            return (format!(
                "WITH page AS MATERIALIZED ({page}),
                 descriptions AS (SELECT DISTINCT s.description FROM page JOIN sounds s USING (id)
                                  WHERE s.description IS NOT NULL),
                 counts AS MATERIALIZED (
                     SELECT description, (SELECT COUNT(*) FROM sounds d WHERE d.description = descriptions.description) AS n
                     FROM descriptions
                 )
                 SELECT {columns} FROM page JOIN sounds s ON s.id = page.id
                 LEFT JOIN counts ON counts.description = s.description{order}"
            ), args);
        }
        Self::build_search_sql_for(params, SOUND_COLUMNS, counting, substring, &[], None)
    }

    fn search_order(params: &SearchParams) -> String {
        let Some(sort) = params.sort_by else { return String::new() };
        let dir = match params.sort_dir.unwrap_or(SortDir::Asc) {
            SortDir::Asc => "ASC",
            SortDir::Desc => "DESC",
        };
        // NULLs sort last in both directions. A leading IS NULL expression
        // instead would prevent SQLite from using the sort indexes.
        let tie = if matches!(sort, SortColumn::Filename) {
            ""
        } else if sort.uses_metadata_sort_index() {
            ", s.filename COLLATE NOCASE ASC, s.id ASC"
        } else {
            ", s.filename COLLATE NOCASE ASC"
        };
        format!(" ORDER BY {} {dir} NULLS LAST{tie}", sort.sql())
    }

    fn build_search_sql_for(
        params: &SearchParams,
        projection: &str,
        counting: bool,
        substring: bool,
        excluded: &[String],
        extra: Option<(&str, &[rusqlite::types::Value])>,
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
            format!("SELECT {projection} FROM {from} WHERE {fts_table} MATCH ?")
        } else {
            format!("SELECT {projection} FROM {from}")
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
        if !params.ucs_categories.is_empty() {
            let holes = std::iter::repeat_n("?", params.ucs_categories.len())
                .collect::<Vec<_>>()
                .join(", ");
            clauses.push(format!("s.ucs_category IN ({holes})"));
            for category in &params.ucs_categories {
                args.push(Value::Text(category.clone()));
            }
        }
        if !params.ucs_subcategories.is_empty() {
            let holes = std::iter::repeat_n("?", params.ucs_subcategories.len())
                .collect::<Vec<_>>()
                .join(", ");
            clauses.push(format!("s.ucs_subcategory IN ({holes})"));
            for subcategory in &params.ucs_subcategories {
                args.push(Value::Text(subcategory.clone()));
            }
        }
        if !params.ucs_cat_ids.is_empty() {
            let holes = std::iter::repeat_n("?", params.ucs_cat_ids.len())
                .collect::<Vec<_>>()
                .join(", ");
            clauses.push(format!("s.ucs_cat_id IN ({holes})"));
            for cat_id in &params.ucs_cat_ids {
                args.push(Value::Text(cat_id.clone()));
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
        if !excluded.is_empty() {
            let holes = std::iter::repeat_n("?", excluded.len())
                .collect::<Vec<_>>()
                .join(", ");
            clauses.push(format!("s.path NOT IN ({holes})"));
            args.extend(excluded.iter().cloned().map(Value::Text));
        }
        if let Some((clause, extra_args)) = extra {
            clauses.push(format!("({clause})"));
            args.extend_from_slice(extra_args);
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
            sql.push_str(&Self::search_order(params));
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

    pub fn ucs_facets(&self) -> Result<Vec<UcsFacet>> {
        let mut stmt = self.conn.prepare(
            "SELECT ucs_category, ucs_subcategory, COUNT(*) AS c
             FROM sounds
             WHERE ucs_category IS NOT NULL
             GROUP BY ucs_category, ucs_subcategory
             ORDER BY c DESC, ucs_category, ucs_subcategory",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(UcsFacet {
                category: r.get(0)?,
                subcategory: r.get(1)?,
                count: r.get::<_, i64>(2)? as u64,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn ucs_cat_counts(&self) -> Result<HashMap<String, u64>> {
        let mut stmt = self.conn.prepare(
            "SELECT ucs_cat_id, COUNT(*) FROM sounds
             WHERE ucs_cat_id IS NOT NULL
             GROUP BY ucs_cat_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?;
        rows.collect::<rusqlite::Result<HashMap<_, _>>>()
            .map_err(Into::into)
    }

    /// Favorite count and total in one statement.
    ///
    /// The sidebar shows both numbers next to each other; two separate queries
    /// would let them disagree for a frame right after a star click.
    ///
    /// No `INDEXED BY idx_favorite`: the query planner picks that index on its
    /// own, and the hint only adds a failure mode. `begin_bulk` drops the index
    /// on the *indexer's* connection, so any cached "the index exists" snapshot
    /// on the UI connection can be stale, and a forced `INDEXED BY`
    /// then fails with `no such index` — which empties the whole sidebar because
    /// `get_sidebar` fails with `summary`. The plan is asserted by the EXPLAIN
    /// test instead.
    pub fn summary(&self) -> Result<(u64, u64)> {
        let (favorites, total) = self.conn.query_row(
            "SELECT COALESCE(SUM(favorite), 0), COUNT(*) FROM sounds",
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

const DESCRIPTION_COUNT_SQL: &str = "CASE WHEN s.description IS NULL THEN NULL ELSE \
       (SELECT COUNT(*) FROM sounds d WHERE d.description = s.description) END";

/// Column list shared by every read, in the order `read_sound` expects.
const SOUND_COLUMNS: &str = "s.id, s.path, s.filename, s.duration, s.channels, s.sample_rate, \
     s.filesize, s.category, s.subcategory, s.confidence, s.matched_terms, s.favorite, \
     s.category_manual, s.folder, s.bpm, s.description, \
     CASE WHEN s.description IS NULL THEN NULL ELSE \
       (SELECT COUNT(*) FROM sounds d WHERE d.description = s.description) END, \
     s.ucs_category, \
      s.ucs_subcategory, s.ucs_cat_id, s.ucs_manual";

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
        description: row.get(15)?,
        description_count: row.get::<_, Option<i64>>(16)?.map(|value| value as u32),
        ucs_category: row.get(17)?,
        ucs_subcategory: row.get(18)?,
        ucs_cat_id: row.get(19)?,
        ucs_manual: row.get::<_, i64>(20)? == 1,
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
    let folder = relative_folder(&root_str, &path_str);
    // Read before `filename` is moved into the row. From the names only: a WAV
    // header carries no tempo, and deriving it from the waveform would mean
    // decoding every file during the index pass. The duration is passed as
    // corroboration for a bare number in a loop name, never as a source.
    let bpm = crate::bpm::tempo(&filename, &folder, meta.duration_s);
    SoundRow {
        folder,
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
        description: meta.description.clone(),
        ucs_category: classification.ucs_category.clone(),
        ucs_subcategory: classification.ucs_subcategory.clone(),
        ucs_cat_id: classification.ucs_cat_id.clone(),
        ucs_manual: false,
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
            description: None,
            ucs_category: None,
            ucs_subcategory: None,
            ucs_cat_id: None,
            ucs_manual: false,
            folder: String::new(),
            mtime_ns: 1,
            bpm: None,
        }
    }

    #[test]
    fn a_newer_schema_version_refuses_to_open() {
        let root = std::env::temp_dir().join(format!(
            "mio-newer-schema-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        {
            let db = IndexDb::open(&root).unwrap();
            db.set_meta("schema_version", &(SCHEMA_VERSION + 1).to_string())
                .unwrap();
        }
        let error = IndexDb::open(&root).err().expect("a newer schema must not open");
        assert!(
            error.to_string().contains("newer Mio"),
            "the error must name the cause: {error}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn db() -> IndexDb {
        IndexDb::open_in_memory("C:/lib").unwrap()
    }

    #[test]
    fn the_lock_name_hash_is_stable_fnv1a() {
        // FNV-1a over the UTF-16LE bytes. Pinned so a future refactor cannot
        // quietly swap in a hasher whose values differ between builds — that
        // would let two mixed-version instances both open the same library.
        let path = std::path::Path::new("C:/lib");
        // The expected value is this function's own definition, evaluated once;
        // the assertion that matters is the second one: the same input never
        // yields two names within a build or across them.
        let first = stable_lock_hash(path);
        let second = stable_lock_hash(path);
        assert_eq!(first, second);
        assert_ne!(first, stable_lock_hash(std::path::Path::new("C:/other")));
        // The FNV offset basis must survive: an empty path hashes to it.
        assert_eq!(stable_lock_hash(std::path::Path::new("")), 0xcbf2_9ce4_8422_2325);
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
    fn summary_counts_favorites_and_stays_correct_across_connections() {
        let mut db = db();
        db.upsert_batch(&[row("C:/lib/a.wav", "Foley"), row("C:/lib/b.wav", "Music")])
            .unwrap();
        assert_eq!(db.summary().unwrap(), (0, 2));
        db.set_favorite_many(&["C:/lib/b.wav".into()], true)
            .unwrap();
        assert_eq!(db.summary().unwrap(), (1, 2));

        // The index is still the planner's choice without a hint; the hint is
        // what made a concurrent DROP INDEX a hard failure.
        let plan: Vec<String> = db
            .connection()
            .prepare("EXPLAIN QUERY PLAN SELECT COUNT(*) FROM sounds WHERE favorite = 1")
            .unwrap()
            .query_map([], |row| row.get(3))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert!(
            plan.iter().any(|detail| detail.contains("idx_favorite")),
            "the planner should still use idx_favorite: {plan:?}"
        );
    }

    /// A5: a second connection dropping `idx_favorite` must not break the first.
    ///
    /// `begin_bulk` drops the secondary indexes on the *indexer's* connection.
    /// A UI connection that forced `INDEXED BY idx_favorite` then failed with
    /// `no such index`, and `get_sidebar` swallowed that as an empty sidebar.
    #[test]
    fn a_summary_survives_a_bulk_drop_on_another_connection() {
        let root = std::env::temp_dir().join(format!(
            "mio-summary-cross-conn-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let writer = IndexDb::open(&root).unwrap();
        let reader = IndexDb::open(&root).unwrap();
        let mut writer = writer;
        writer
            .upsert_batch(&[row("C:/lib/a.wav", "Foley"), row("C:/lib/b.wav", "Music")])
            .unwrap();
        assert_eq!(reader.summary().unwrap(), (0, 2));

        writer.begin_bulk().unwrap();
        // The reader was opened before the drop and its connection now sees a
        // table with no idx_favorite.
        assert_eq!(
            reader.summary().unwrap(),
            (0, 2),
            "summary must not depend on an index another connection dropped"
        );
        writer.end_bulk().unwrap();
        assert_eq!(reader.summary().unwrap(), (0, 2));

        drop(writer);
        drop(reader);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_diff_reprobes_when_size_changes_even_if_mtime_is_unchanged() {
        let mut db = db();
        db.upsert_batch(&[row("C:/lib/a.wav", "Foley")]).unwrap();
        db.begin_scan().unwrap();
        db.stage_scan_batch(&[("C:/lib/a.wav".into(), 101, 1)])
            .unwrap();
        assert_eq!(db.stage_scan_changes().unwrap(), 1);
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
        assert!(db
            .search(&SearchParams::new("no-match"))
            .unwrap()
            .is_empty());
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
    fn a_wider_tempo_rule_refills_rows_the_older_rule_left_empty() {
        // A library that went through the marker-only rule records `bpm_rule = 1`.
        // Its bare-number rows are NULL, and an incremental pass will never revisit
        // them because the files have not changed, so opening the database has to
        // run the wider rule over the names it already has.
        let root = std::env::temp_dir().join(format!(
            "mio-bpm-rule-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        {
            let mut db = IndexDb::open(&root).unwrap();
            let mut loop_row = row("C:/lib/Loops/Vocal Stack Loop 128.wav", "Vocals");
            loop_row.filename = "Vocal Stack Loop 128.wav".into();
            loop_row.folder = "Loops".into();
            loop_row.duration_s = Some(3.75); // exactly 2 bars at 128
            let mut indexed = row("C:/lib/OneShots/Snare_128.wav", "Drums");
            indexed.filename = "Snare_128.wav".into();
            indexed.folder = "OneShots".into();
            indexed.duration_s = Some(3.75);
            db.upsert_batch(&[loop_row, indexed]).unwrap();
            // Stand in for the older build: the column exists, the values this rule
            // would fill were never computed, and the recorded rule is the narrow one.
            db.conn
                .execute_batch("UPDATE sounds SET bpm = NULL")
                .unwrap();
            db.set_meta(BPM_BACKFILL_KEY, "1").unwrap();
        }

        let db = IndexDb::open(&root).expect("open re-runs the wider rule");
        let looped = db
            .get_by_path("C:/lib/Loops/Vocal Stack Loop 128.wav")
            .unwrap()
            .unwrap();
        assert_eq!(
            looped.bpm,
            Some(128.0),
            "a loop name with a bare number is filled on upgrade"
        );
        let one_shot = db
            .get_by_path("C:/lib/OneShots/Snare_128.wav")
            .unwrap()
            .unwrap();
        assert_eq!(
            one_shot.bpm, None,
            "a one-shot name keeps its number out of the tempo column"
        );

        // A second open must not redo the pass.
        drop(db);
        let again = IndexDb::open(&root).unwrap();
        assert_eq!(
            again.get_meta(BPM_BACKFILL_KEY).unwrap().as_deref(),
            Some(BPM_BACKFILL)
        );

        drop(again);
        let _ = std::fs::remove_dir_all(&root);
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
    fn native_ucs_facets_filters_and_sorts_without_using_legacy_categories() {
        let mut db = db();
        let mut source = row("C:/lib/GORESrce_hit.wav", "Impacts");
        source.ucs_category = Some("GORE".into());
        source.ucs_subcategory = Some("SOURCE".into());
        source.ucs_cat_id = Some("GORESrce".into());
        let mut metal = row("C:/lib/METLFric_hit.wav", "Impacts");
        metal.ucs_category = Some("METAL".into());
        metal.ucs_subcategory = Some("FRICTION".into());
        metal.ucs_cat_id = Some("METLFric".into());
        let legacy_only = row("C:/lib/legacy.wav", "GORE");
        db.upsert_batch(&[source, metal, legacy_only]).unwrap();

        let mut params = SearchParams::new("");
        params.ucs_categories = vec!["GORE".into()];
        params.ucs_subcategories = vec!["SOURCE".into()];
        let hits = db.search(&params).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].ucs_cat_id.as_deref(), Some("GORESrce"));

        let facets = db.ucs_facets().unwrap();
        assert_eq!(facets.len(), 2);
        assert_eq!(facets[0].category, "GORE");
        assert_eq!(facets[0].subcategory.as_deref(), Some("SOURCE"));

        params.ucs_categories.clear();
        params.ucs_subcategories.clear();
        params.sort_by = Some(SortColumn::UcsCategory);
        let sorted = db.search(&params).unwrap();
        assert_eq!(sorted[0].ucs_category.as_deref(), Some("GORE"));
        assert_eq!(sorted[1].ucs_category.as_deref(), Some("METAL"));
    }

    /// The CatID is filtered by and grouped by, so both have to use the index.
    ///
    /// Without it the counts were a `SCAN sounds` plus a temp B-tree — 447 ms on
    /// the reference library, on the UI thread, on every star click. Asserting the
    /// plan rather than a duration keeps the test honest on other machines.
    #[test]
    fn ucs_cat_id_filters_and_counts_use_their_index() {
        let mut db = db();
        let mut a = row("C:/lib/a.wav", "Impacts");
        a.ucs_cat_id = Some("IMPCTFir".into());
        let mut b = row("C:/lib/b.wav", "Impacts");
        b.ucs_cat_id = Some("METLFric".into());
        db.upsert_batch(&[a, b, row("C:/lib/c.wav", "Foley")]).unwrap();

        let mut params = SearchParams::new("");
        params.ucs_cat_ids = vec!["IMPCTFir".into()];
        let hits = db.search(&params).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].ucs_cat_id.as_deref(), Some("IMPCTFir"));

        let counts = db.ucs_cat_counts().unwrap();
        assert_eq!(counts.get("IMPCTFir"), Some(&1));

        for sql in [
            "SELECT ucs_cat_id, COUNT(*) FROM sounds WHERE ucs_cat_id IS NOT NULL GROUP BY ucs_cat_id",
            "SELECT COUNT(*) FROM sounds WHERE ucs_cat_id = 'IMPCTFir'",
        ] {
            let plan: Vec<String> = db
                .connection()
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map([], |row| row.get(3))
                .unwrap()
                .map(|row| row.unwrap())
                .collect();
            assert!(
                plan.iter().any(|detail| detail.contains("idx_ucs_cat_id")),
                "the planner should use idx_ucs_cat_id for {sql}: {plan:?}"
            );
        }
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
            description: None,
            ucs_category: None,
            ucs_subcategory: None,
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

    #[test]
    fn descriptions_are_searchable_and_native_ucs_fields_round_trip() {
        let mut db = db();
        let mut value = row("C:/lib/unknown.wav", "Uncategorized");
        value.description = Some("Metal, Impact, Hit".into());
        value.ucs_category = Some("GORE".into());
        value.ucs_subcategory = Some("SOURCE".into());
        value.ucs_cat_id = Some("GORESrce".into());
        db.upsert_batch(&[value]).unwrap();
        let hits = db.search(&SearchParams::new("impact")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].description.as_deref(), Some("Metal, Impact, Hit"));
        assert_eq!(hits[0].description_count, Some(1));
        assert_eq!(hits[0].ucs_cat_id.as_deref(), Some("GORESrce"));
    }

    #[test]
    fn metadata_update_requires_the_original_fingerprint_and_updates_fts() {
        let mut db = db();
        let mut value = row("C:/lib/aircraft.wav", "Uncategorized");
        value.mtime_ns = 12;
        value.filesize = 100;
        db.upsert_batch(&[value]).unwrap();
        let classifier = Classifier::new();
        let update = MetadataUpdate {
            path: "C:/lib/aircraft.wav".into(),
            filesize: 100,
            mtime_ns: 12,
            description: Some("unicorn engine pass".into()),
            classification: classifier.classify_with_metadata(
                "C:/lib/aircraft.wav",
                Some("unicorn engine pass"),
                Some("AIRCRAFT"),
                Some("JET"),
            ),
        };
        assert_eq!(db.apply_metadata_batch(std::slice::from_ref(&update)).unwrap(), 1);
        assert_eq!(db.search(&SearchParams::new("unicorn")).unwrap().len(), 1);

        let mut stale = update.clone();
        stale.mtime_ns = 13;
        assert_eq!(db.apply_metadata_batch(&[stale]).unwrap(), 0);
        let sound = db.get_by_path("C:/lib/aircraft.wav").unwrap().unwrap();
        assert_eq!(sound.description.as_deref(), Some("unicorn engine pass"));

        let mut cleared = update;
        cleared.description = None;
        cleared.classification = classifier.classify_with_metadata(
            "C:/lib/aircraft.wav",
            None,
            None,
            None,
        );
        assert_eq!(db.apply_metadata_batch(&[cleared]).unwrap(), 1);
        let sound = db.get_by_path("C:/lib/aircraft.wav").unwrap().unwrap();
        assert_eq!(sound.description, None);
        assert!(db.search(&SearchParams::new("unicorn")).unwrap().is_empty());
    }
}
