//! The library: one SQLite index inside the folder the user opened.
//!
//! The index itself is `reslip-index` and unchanged. This module exists for two
//! reasons:
//!
//! * **Indexing is a background job.** A full pass over a 280k-file library is 64
//!   seconds of work (measured, `docs/agent-log.md`), so it must not run inside
//!   the command that opens the folder. `Library::open` returns as soon as the
//!   database is readable and the first page can be painted; `Library::reindex`
//!   runs on its own thread and reports progress through atomic counters the UI
//!   polls. The pass opens its own connection: SQLite allows one writer and many
//!   readers, and WAL is on precisely so the UI keeps querying while it writes.
//! * **Lifecycle is owned here.** A library is replaced when the user opens a
//!   different folder, so `Drop` waits for any running pass before the handle
//!   goes away.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use reslip_index::{
    CategoryFacet, FolderNode, IndexDb, IndexOptions, IndexReport, SearchParams, SortColumn,
    SortDir, Sound, index_folder,
};

use crate::error::{AppError, AppResult};

/// Progress of the running index pass, polled by the UI.
#[derive(Debug, Clone, Default)]
pub struct IndexProgress {
    pub running: bool,
    pub done: usize,
    pub total: usize,
}

#[derive(Default)]
struct Progress {
    running: AtomicBool,
    done: AtomicU64,
    total: AtomicU64,
}

impl Progress {
    fn reset(&self) {
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
    }

    fn snapshot(&self) -> IndexProgress {
        let total = self.total.load(Ordering::Relaxed) as usize;
        let done = (self.done.load(Ordering::Relaxed) as usize).min(total);
        IndexProgress {
            running: self.running.load(Ordering::Acquire),
            done,
            total,
        }
    }
}

/// A library, plus whatever index pass is running against it.
pub struct Library {
    root: PathBuf,
    db: Arc<Mutex<IndexDb>>,
    progress: Arc<Progress>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Library {
    /// Open the index in `root`, creating an empty one if there is none.
    ///
    /// Fast: this only migrates the schema and repairs a half-finished bulk load.
    /// It does not scan the folder.
    pub fn open(root: impl AsRef<Path>) -> AppResult<Self> {
        let root = root.as_ref().to_path_buf();
        let db = IndexDb::open(&root)?;
        Ok(Self {
            root,
            db: Arc::new(Mutex::new(db)),
            progress: Arc::new(Progress::default()),
            worker: Mutex::new(None),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn db(&self) -> AppResult<std::sync::MutexGuard<'_, IndexDb>> {
        self.db
            .lock()
            .map_err(|_| AppError::internal("index lock poisoned"))
    }

    pub fn count(&self) -> AppResult<u64> {
        Ok(self.db()?.count()?)
    }

    pub fn progress(&self) -> IndexProgress {
        self.progress.snapshot()
    }

    /// Start an incremental pass on a background thread. Returns `false` when a
    /// pass was already running: two indexers writing the same database would
    /// fight over the write lock and both be slower.
    pub fn reindex(self: &Arc<Self>) -> AppResult<bool> {
        if self
            .progress
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(false);
        }
        self.progress.reset();

        let root = self.root.clone();
        let progress = Arc::clone(&self.progress);
        let handle = std::thread::Builder::new()
            .name("reslip-indexer".into())
            .spawn(move || {
                let options = IndexOptions {
                    incremental: true,
                    prune: true,
                    ..Default::default()
                };
                match index_folder(&root, &options, |done, total| {
                    progress.done.store(done as u64, Ordering::Relaxed);
                    progress.total.store(total as u64, Ordering::Relaxed);
                }) {
                    Ok(report) => log::info!(
                        "index pass: {} seen, {} probed, {} written, {} pruned in {:.0}ms",
                        report.files_seen,
                        report.files_probed,
                        report.rows_written,
                        report.rows_pruned,
                        report.timings.total_ms()
                    ),
                    Err(e) => log::warn!("index pass failed: {e}"),
                }
                progress.running.store(false, Ordering::Release);
            })
            .map_err(|e| AppError::internal(format!("spawn indexer: {e}")))?;

        if let Ok(mut slot) = self.worker.lock() {
            *slot = Some(handle);
        }
        Ok(true)
    }

    /// Run a pass synchronously and return its report.
    ///
    /// `progress` is called with `(done, total)` as the pass advances, which is how
    /// the blocking open screen draws its bar. `full` re-probes every file and
    /// prunes vanished ones; the incremental form only touches what changed.
    ///
    /// This blocks the calling thread for the whole pass — 64 seconds on a 280k
    /// library, measured. The caller decides whether that is acceptable: the UI's
    /// open screen shows a bar and the window is unusable until it returns, which
    /// is the behaviour the user asked for (an index you can browse half of is
    /// worse than a bar that tells you how long it will be).
    pub fn index_blocking_with<F>(&self, full: bool, progress: F) -> AppResult<IndexReport>
    where
        F: FnMut(usize, usize),
    {
        let options = IndexOptions {
            incremental: !full,
            prune: full,
            ..Default::default()
        };
        Ok(index_folder(&self.root, &options, progress)?)
    }

    /// Run a pass synchronously and return its report, without progress.
    pub fn index_blocking(&self, full: bool) -> AppResult<IndexReport> {
        self.index_blocking_with(full, |_, _| {})
    }

    pub fn search(&self, params: &SearchParams) -> AppResult<Vec<Sound>> {
        Ok(self.db()?.search(params)?)
    }

    /// One sound by absolute path, or `None` if it is not indexed.
    pub fn get(&self, path: &str) -> AppResult<Option<Sound>> {
        Ok(self.db()?.get_by_path(path)?)
    }

    pub fn count_matching(&self, params: &SearchParams) -> AppResult<u64> {
        Ok(self.db()?.count_matching(params)?)
    }

    pub fn facets(&self) -> AppResult<Vec<CategoryFacet>> {
        Ok(self.db()?.facets()?)
    }

    /// Favorite count and total, for the sidebar's two summary rows.
    ///
    /// Read in one statement rather than two round trips: the sidebar redraws on
    /// every edit, and a second query would make the two numbers disagree for a
    /// frame after a star click.
    pub fn summary(&self) -> AppResult<(u64, u64)> {
        Ok(self.db()?.summary()?)
    }

    /// Set the favorite flag on many paths in one transaction.
    pub fn set_favorite(&self, paths: &[String], favorite: bool) -> AppResult<u64> {
        Ok(self.db()?.set_favorite_many(paths, favorite)?)
    }

    /// Assign one category to many paths in one transaction.
    pub fn set_category(
        &self,
        paths: &[String],
        category: &str,
        subcategory: Option<&str>,
    ) -> AppResult<u64> {
        Ok(self
            .db()?
            .set_category_many(paths, category, subcategory)?)
    }

    /// Drop the manual category on many paths, letting the next pass reclassify.
    pub fn clear_category(&self, paths: &[String]) -> AppResult<u64> {
        Ok(self.db()?.clear_category_many(paths)?)
    }

    /// Put a category back exactly as recorded, including whether it was manual.
    ///
    /// Undo needs this rather than `set_category`: a row the classifier filled in
    /// must go back to being the classifier's, not become a manual choice with the
    /// same string.
    pub fn restore_category(
        &self,
        paths: &[String],
        category: &str,
        subcategory: Option<&str>,
        manual: bool,
    ) -> AppResult<u64> {
        Ok(self
            .db()?
            .restore_category_many(paths, category, subcategory, manual)?)
    }

    /// Folder tree with recursive counts. Built from indexed paths, never from
    /// `readdir` (see `docs/index.md`).
    pub fn folder_tree(&self) -> AppResult<FolderNode> {
        Ok(self.db()?.folder_tree()?)
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // A pass against a folder that is about to be replaced must stop writing
        // before the handle goes away.
        if let Ok(mut slot) = self.worker.lock() {
            if let Some(handle) = slot.take() {
                let _ = handle.join();
            }
        }
    }
}

/// The sort columns the UI may ask for, by wire name. Whitelisted: an unknown
/// name is `None`, never interpolated into SQL.
pub fn sort_column(name: &str) -> Option<SortColumn> {
    Some(match name {
        "favorite" => SortColumn::Favorite,
        "filename" => SortColumn::Filename,
        "category" => SortColumn::Category,
        "duration" => SortColumn::Duration,
        "channels" => SortColumn::Channels,
        "sample_rate" => SortColumn::SampleRate,
        "filesize" => SortColumn::Filesize,
        _ => return None,
    })
}

pub fn sort_dir(name: &str) -> SortDir {
    if name == "desc" {
        SortDir::Desc
    } else {
        SortDir::Asc
    }
}
