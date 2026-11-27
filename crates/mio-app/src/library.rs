//! The library: one SQLite index inside the folder the user opened.
//!
//! The base pass runs before the invoking command returns. The metadata pass
//! uses a separate WAL connection so the UI can query during enrichment.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use mio_index::{
    index_folder_with_events, CategoryFacet, EditState, FolderNode, IndexDb,
    IndexOptions, IndexPhase, IndexProgress as CoreIndexProgress, IndexReport, MetadataUpdate,
    SearchParams, SkippedDir, SortColumn, SortDir, Sound,
};

use crate::error::{AppError, AppResult};

/// Progress of the running index pass, polled by the UI.
#[derive(Debug, Clone, Default)]
pub struct IndexProgress {
    pub running: bool,
    pub phase: IndexPhase,
    pub done: usize,
    pub total: usize,
    pub files_seen: usize,
    pub files_changed: usize,
    pub files_written: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MetadataStatus {
    Idle,
    Indexing,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct MetadataProgress {
    pub indexing: bool,
    pub indexed: usize,
    pub total: usize,
    pub failed: usize,
    pub status: MetadataStatus,
    pub generation: u64,
}

impl Default for MetadataProgress {
    fn default() -> Self {
        Self {
            indexing: false,
            indexed: 0,
            total: 0,
            failed: 0,
            status: MetadataStatus::Idle,
            generation: 0,
        }
    }
}

#[derive(Default)]
struct Progress {
    running: AtomicBool,
    phase: AtomicU8,
    done: AtomicU64,
    total: AtomicU64,
    files_seen: AtomicU64,
    files_changed: AtomicU64,
    files_written: AtomicU64,
}

impl Progress {
    fn reset(&self) {
        self.phase.store(phase_code(IndexPhase::Scanning), Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        self.files_seen.store(0, Ordering::Relaxed);
        self.files_changed.store(0, Ordering::Relaxed);
        self.files_written.store(0, Ordering::Relaxed);
    }

    fn snapshot(&self) -> IndexProgress {
        let total = self.total.load(Ordering::Relaxed) as usize;
        let done = (self.done.load(Ordering::Relaxed) as usize).min(total);
        IndexProgress {
            running: self.running.load(Ordering::Acquire),
            phase: phase_from_code(self.phase.load(Ordering::Relaxed)),
            done,
            total,
            files_seen: self.files_seen.load(Ordering::Relaxed) as usize,
            files_changed: self.files_changed.load(Ordering::Relaxed) as usize,
            files_written: self.files_written.load(Ordering::Relaxed) as usize,
        }
    }

    fn update(&self, event: CoreIndexProgress) {
        self.phase.store(phase_code(event.phase), Ordering::Relaxed);
        self.done.store(event.done as u64, Ordering::Relaxed);
        self.total.store(event.total as u64, Ordering::Relaxed);
        self.files_seen.store(event.files_seen as u64, Ordering::Relaxed);
        self.files_changed.store(event.files_changed as u64, Ordering::Relaxed);
        self.files_written.store(event.files_written as u64, Ordering::Relaxed);
    }
}

fn phase_code(phase: IndexPhase) -> u8 {
    match phase {
        IndexPhase::Scanning => 0,
        IndexPhase::Comparing => 1,
        IndexPhase::Indexing => 2,
        IndexPhase::Finalizing => 3,
        IndexPhase::Ready => 4,
        IndexPhase::Failed => 5,
    }
}

fn phase_from_code(code: u8) -> IndexPhase {
    match code {
        1 => IndexPhase::Comparing,
        2 => IndexPhase::Indexing,
        3 => IndexPhase::Finalizing,
        4 => IndexPhase::Ready,
        5 => IndexPhase::Failed,
        _ => IndexPhase::Scanning,
    }
}

struct MetadataTask {
    cancel: Arc<AtomicBool>,
    status: Arc<Mutex<MetadataProgress>>,
    worker: Option<JoinHandle<()>>,
}

impl Default for MetadataTask {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(MetadataProgress::default())),
            worker: None,
        }
    }
}

impl MetadataTask {
    fn cancel_and_join(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

/// A library, plus whatever index pass is running against it.
pub struct Library {
    root: PathBuf,
    db: Arc<Mutex<IndexDb>>,
    progress: Arc<Progress>,
    worker: Mutex<Option<JoinHandle<()>>>,
    index_cancel: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    metadata: Mutex<MetadataTask>,
    folder_tree_cache: Mutex<Option<(i64, FolderNode)>>,
    last_skipped_dirs: Arc<Mutex<Vec<SkippedDir>>>,
}

impl Library {
    /// Open the index in `root`, creating an empty one if there is none.
    ///
    /// Fast: this only migrates the schema and repairs a half-finished bulk load.
    /// It does not scan the folder.
    pub fn open(root: impl AsRef<Path>) -> AppResult<Self> {
        let root = root.as_ref().to_path_buf();
        let db = IndexDb::open_fast(&root)?;
        Ok(Self {
            root,
            db: Arc::new(Mutex::new(db)),
            progress: Arc::new(Progress::default()),
            worker: Mutex::new(None),
            index_cancel: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
            metadata: Mutex::new(MetadataTask::default()),
            folder_tree_cache: Mutex::new(None),
            last_skipped_dirs: Arc::new(Mutex::new(Vec::new())),
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

    pub fn metadata_progress(&self) -> MetadataProgress {
        self.metadata
            .lock()
            .ok()
            .and_then(|task| task.status.lock().ok().map(|status| status.clone()))
            .unwrap_or_default()
    }

    fn cancel_metadata(&self) {
        if let Ok(mut task) = self.metadata.lock() {
            task.cancel_and_join();
        }
    }

    fn start_metadata(&self) {
        let Ok(mut task) = self.metadata.lock() else { return };
        if self.index_cancel.load(Ordering::Acquire) {
            return;
        }
        task.cancel_and_join();
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let root = self.root.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(MetadataProgress {
            indexing: true,
            indexed: 0,
            total: 0,
            failed: 0,
            status: MetadataStatus::Indexing,
            generation,
        }));
        let cancel_for_worker = Arc::clone(&cancel);
        let generation_clock = Arc::clone(&self.generation);
        let status_for_worker = Arc::clone(&status);
        let handle = std::thread::Builder::new()
            .name("mio-metadata".into())
            .spawn(move || {
                let result = run_metadata_worker(
                    &root,
                    generation,
                    generation_clock.as_ref(),
                    cancel_for_worker.as_ref(),
                    &status_for_worker,
                );
                if let Ok(mut state) = status_for_worker.lock() {
                    state.indexing = false;
                    state.status = match result {
                        Ok(()) if state.failed > 0 => MetadataStatus::Failed,
                        Ok(()) => MetadataStatus::Done,
                        Err(mio_index::IndexError::Cancelled) => MetadataStatus::Cancelled,
                        Err(_) => MetadataStatus::Failed,
                    };
                }
            });
        let handle = match handle {
            Ok(handle) => Some(handle),
            Err(_) => {
                if let Ok(mut state) = status.lock() {
                    state.indexing = false;
                    state.status = MetadataStatus::Failed;
                }
                None
            }
        };
        task.cancel = cancel;
        task.status = status;
        task.worker = handle;
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
        self.cancel_metadata();
        self.generation.fetch_add(1, Ordering::AcqRel);

        let root = self.root.clone();
        let skipped_dirs = Arc::clone(&self.last_skipped_dirs);
        let progress = Arc::clone(&self.progress);
        let cancel = Arc::clone(&self.index_cancel);
        let library = Arc::downgrade(self);
        let handle = std::thread::Builder::new()
            .name("mio-indexer".into())
            .spawn(move || {
                let options = IndexOptions {
                    incremental: true,
                    prune: true,
                    ..Default::default()
                };
                match index_folder_with_events(&root, &options, Some(cancel.as_ref()), |event| {
                    progress.update(event);
                }) {
                    Ok(report) => {
                        if let Ok(mut skipped) = skipped_dirs.lock() {
                            *skipped = report.skipped_dirs.clone();
                        }
                        log::info!(
                            "index pass: {} seen, {} probed, {} written, {} pruned, {} skipped dirs in {:.0}ms",
                            report.files_seen,
                            report.files_probed,
                            report.rows_written,
                            report.rows_pruned,
                            report.skipped_dirs.len(),
                            report.timings.total_ms()
                        );
                        if let Some(library) = library.upgrade() {
                            library.start_metadata();
                        }
                    }
                    Err(e) => log::warn!("index pass failed: {e}"),
                }
                progress.running.store(false, Ordering::Release);
            });
        let handle = handle.map_err(|e| {
            self.progress.running.store(false, Ordering::Release);
            AppError::internal(format!("spawn indexer: {e}"))
        })?;

        if let Ok(mut slot) = self.worker.lock() {
            *slot = Some(handle);
        }
        Ok(true)
    }

    /// Commit the base index, start the metadata worker, and return the base report.
    pub fn index_blocking_with_events<F>(
        &self,
        full: bool,
        mut progress: F,
    ) -> AppResult<IndexReport>
    where
        F: FnMut(IndexProgress),
    {
        if self
            .progress
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(AppError::internal("library indexing is already running"));
        }
        self.cancel_metadata();
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.progress.reset();
        let options = IndexOptions {
            incremental: !full,
            prune: full,
            read_metadata: false,
            ..Default::default()
        };
        let result = index_folder_with_events(
            &self.root,
            &options,
            Some(self.index_cancel.as_ref()),
            |event| {
                self.progress.update(event);
                progress(IndexProgress {
                    running: true,
                    phase: event.phase,
                    done: event.done,
                    total: event.total,
                    files_seen: event.files_seen,
                    files_changed: event.files_changed,
                    files_written: event.files_written,
                });
            },
        );
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                self.progress.phase.store(phase_code(IndexPhase::Failed), Ordering::Release);
                self.progress.running.store(false, Ordering::Release);
                let snapshot = self.progress.snapshot();
                progress(IndexProgress {
                    running: false,
                    phase: IndexPhase::Failed,
                    done: snapshot.done,
                    total: snapshot.total,
                    files_seen: snapshot.files_seen,
                    files_changed: snapshot.files_changed,
                    files_written: snapshot.files_written,
                });
                return Err(error.into());
            }
        };
        if let Ok(mut skipped) = self.last_skipped_dirs.lock() {
            *skipped = report.skipped_dirs.clone();
        }
        self.start_metadata();
        self.progress.running.store(false, Ordering::Release);
        Ok(report)
    }

    pub fn index_blocking_with<F>(&self, full: bool, mut progress: F) -> AppResult<IndexReport>
    where
        F: FnMut(usize, usize),
    {
        let report = self.index_blocking_with_events(full, |event| {
            if matches!(event.phase, IndexPhase::Indexing | IndexPhase::Ready) {
                progress(event.done, event.total);
            }
        })?;
        Ok(report)
    }

    pub fn skipped_dirs(&self) -> Vec<SkippedDir> {
        self.last_skipped_dirs
            .lock()
            .map(|skipped| skipped.clone())
            .unwrap_or_default()
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

    pub fn set_favorite_matching(
        &self,
        params: &SearchParams,
        excluded: &[String],
        favorite: bool,
    ) -> AppResult<(Vec<EditState>, u64)> {
        Ok(self
            .db()?
            .set_favorite_matching(params, excluded, favorite)?)
    }

    pub fn set_category_matching(
        &self,
        params: &SearchParams,
        excluded: &[String],
        category: &str,
        subcategory: Option<&str>,
    ) -> AppResult<(Vec<EditState>, u64)> {
        Ok(self
            .db()?
            .set_category_matching(params, excluded, category, subcategory)?)
    }

    pub fn set_ucs_category_matching(
        &self,
        params: &SearchParams,
        excluded: &[String],
        category: &str,
        subcategory: Option<&str>,
        cat_id: &str,
        projected_category: &str,
        projected_subcategory: Option<&str>,
    ) -> AppResult<(Vec<EditState>, u64)> {
        Ok(self.db()?.set_ucs_category_matching(
            params,
            excluded,
            category,
            subcategory,
            cat_id,
            projected_category,
            projected_subcategory,
        )?)
    }

    pub fn clear_category_matching(
        &self,
        params: &SearchParams,
        excluded: &[String],
    ) -> AppResult<(Vec<EditState>, u64)> {
        Ok(self.db()?.clear_category_matching(params, excluded)?)
    }

    pub fn facets(&self) -> AppResult<Vec<CategoryFacet>> {
        Ok(self.db()?.facets()?)
    }

    pub fn ucs_facets(&self) -> AppResult<Vec<mio_index::UcsFacet>> {
        Ok(self.db()?.ucs_facets()?)
    }

    pub fn ucs_cat_counts(&self) -> AppResult<std::collections::HashMap<String, u64>> {
        Ok(self.db()?.ucs_cat_counts()?)
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
        Ok(self.db()?.set_category_many(paths, category, subcategory)?)
    }

    pub fn set_ucs_category(
        &self,
        paths: &[String],
        category: &str,
        subcategory: Option<&str>,
        cat_id: &str,
        projected_category: &str,
        projected_subcategory: Option<&str>,
    ) -> AppResult<u64> {
        Ok(self.db()?.set_ucs_category_many(
            paths,
            category,
            subcategory,
            cat_id,
            projected_category,
            projected_subcategory,
        )?)
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

    pub fn restore_category_and_ucs(&self, states: &[EditState]) -> AppResult<u64> {
        Ok(self.db()?.restore_category_and_ucs_many(states)?)
    }

    /// Folder tree with recursive counts. Built from indexed paths, never from
    /// `readdir` (see `docs/index.md`).
    pub fn folder_tree(&self) -> AppResult<FolderNode> {
        let db = self.db()?;
        let version = db.index_version()?;
        let mut cache = self
            .folder_tree_cache
            .lock()
            .map_err(|_| AppError::internal("folder tree cache lock poisoned"))?;
        if let Some((cached_version, tree)) = cache.as_ref() {
            if *cached_version == version {
                return Ok(tree.clone());
            }
        }
        let tree = db.folder_tree()?;
        *cache = Some((version, tree.clone()));
        Ok(tree)
    }

    pub(crate) fn close(&self) {
        self.index_cancel.store(true, Ordering::Release);
        self.cancel_metadata();
        // Release the slot before joining. The startup worker may be dropping
        // its last library reference, which calls close again.
        let handle = self.worker.lock().ok().and_then(|mut slot| slot.take());
        if let Some(handle) = handle {
            if handle.thread().id() != std::thread::current().id() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        self.close();
    }
}

fn run_metadata_worker(
    root: &Path,
    generation: u64,
    generation_clock: &AtomicU64,
    cancel: &AtomicBool,
    status: &Mutex<MetadataProgress>,
) -> mio_index::Result<()> {
    let started = std::time::Instant::now();
    let cancelled = || cancel.load(Ordering::Acquire) || generation_clock.load(Ordering::Acquire) != generation;
    if cancelled() {
        return Err(mio_index::IndexError::Cancelled);
    }
    let mut db = IndexDb::open_fast(root)?;
    db.prepare_metadata_pass()?;
    let total = db.metadata_candidate_count()?;
    if let Ok(mut state) = status.lock() {
        state.total = total;
    }
    let classifier = mio_index::Classifier::new();
    let mut indexed = 0usize;
    let mut failed = 0usize;
    const BATCH: usize = 5_000;
    let mut after_path: Option<String> = None;

    loop {
        if cancelled() {
            return Err(mio_index::IndexError::Cancelled);
        }
        let candidates = db.metadata_candidates_after(after_path.as_deref(), BATCH)?;
        if candidates.is_empty() {
            break;
        }
        after_path = candidates.last().map(|candidate| candidate.path.clone());
        let mut updates = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            if cancelled() {
                return Err(mio_index::IndexError::Cancelled);
            }
            let path = PathBuf::from(&candidate.path);
            let metadata = match mio_index::metadata_only_with_meta_checked(
                &path,
                candidate.filesize,
                candidate.mtime_ns,
            ) {
                Ok(Some(metadata)) => metadata,
                Ok(None) => {
                    indexed += 1;
                    if let Ok(mut state) = status.lock() {
                        state.indexed = indexed;
                    }
                    continue;
                }
                Err(_) => {
                    if db.mark_metadata_failed(&candidate)? {
                        failed += 1;
                        indexed += 1;
                    }
                    if let Ok(mut state) = status.lock() {
                        state.indexed = indexed;
                        state.failed = failed;
                        state.generation = generation;
                    }
                    continue;
                }
            };
            let classification = classifier.classify_with_metadata(
                &candidate.path,
                metadata.description.as_deref(),
                metadata.ucs_category.as_deref(),
                metadata.ucs_subcategory.as_deref(),
            );
            updates.push(MetadataUpdate {
                path: candidate.path,
                filesize: candidate.filesize,
                mtime_ns: candidate.mtime_ns,
                description: metadata.description,
                classification,
            });
            indexed += 1;
            if let Ok(mut state) = status.lock() {
                state.indexed = indexed;
                state.failed = failed;
                state.generation = generation;
            }
        }
        if cancelled() {
            return Err(mio_index::IndexError::Cancelled);
        }
        db.apply_metadata_batch(&updates)?;
    }
    log::info!("metadata pass: {indexed}/{total} processed, {failed} failed in {:.0}ms", started.elapsed().as_secs_f64() * 1000.0);
    Ok(())
}

/// The sort columns the UI may ask for, by wire name. Whitelisted: an unknown
/// name is `None`, never interpolated into SQL.
pub fn sort_column(name: &str) -> Option<SortColumn> {
    Some(match name {
        "favorite" => SortColumn::Favorite,
        "filename" => SortColumn::Filename,
        "category" => SortColumn::Category,
        "duration" => SortColumn::Duration,
        "bpm" => SortColumn::Bpm,
        "channels" => SortColumn::Channels,
        "sample_rate" => SortColumn::SampleRate,
        "filesize" => SortColumn::Filesize,
        "description" => SortColumn::Description,
        "ucs_category" => SortColumn::UcsCategory,
        "ucs_subcategory" => SortColumn::UcsSubcategory,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(path: &Path, description: &str) {
        let mut bytes = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x80\xbb\0\0\0\x77\x01\0\x02\0\x10\0data\x02\0\0\0\0\0bext".to_vec();
        bytes.extend_from_slice(&(description.len() as u32).to_le_bytes());
        bytes.extend_from_slice(description.as_bytes());
        if description.len() % 2 != 0 {
            bytes.push(0);
        }
        let size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn metadata_resumes_pending_rows_preserves_edits_and_skips_changed_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let missing = root.join("a_missing.wav");
        let changed = root.join("b_changed.wav");
        let edited = root.join("c_edited.wav");
        wav(&missing, "A short metallic clang");
        wav(&changed, "A short metallic clang");
        wav(&edited, "A rusty gate creaking softly");

        let mut db = IndexDb::open_fast(root).unwrap();
        mio_index::index_folder(root, &IndexOptions::default(), |_, _| {}).unwrap();
        let path = mio_index::folders::store_path(&edited);
        let paths = std::slice::from_ref(&path);
        db.set_ucs_category_many(paths, "AIRCRAFT", Some("JET"), "AEROJet", "Vehicles", Some("Aircraft")).unwrap();
        db.set_favorite_many(paths, true).unwrap();
        assert_eq!(db.metadata_candidate_count().unwrap(), 3);
        assert!(db.get_by_path(&path).unwrap().unwrap().description.is_none());
        assert_eq!(db.search(&SearchParams::new("edited")).unwrap().len(), 1);
        let base_version = db.index_version().unwrap();

        std::fs::remove_file(missing).unwrap();
        wav(&changed, "Thunder rolling over the valley after sunset");
        let generation = AtomicU64::new(1);
        let status = Mutex::new(MetadataProgress::default());
        let cancel = AtomicBool::new(false);
        run_metadata_worker(root, 1, &generation, &cancel, &status).unwrap();
        let first = status.into_inner().unwrap();
        assert_eq!((first.indexed, first.total, first.failed), (3, 3, 1));
        assert!(db.index_version().unwrap() > base_version);
        let row = db.search(&SearchParams::new("rusty")).unwrap().pop().unwrap();
        assert_eq!(row.path, path);
        assert!(row.favorite && row.category_manual && row.ucs_manual);
        assert_eq!(row.ucs_cat_id.as_deref(), Some("AEROJet"));
        assert_eq!(row.category, "Vehicles");
        assert_eq!(db.metadata_candidate_count().unwrap(), 1);
        assert!(db.search(&SearchParams::new("thunder")).unwrap().is_empty());

        let report = mio_index::index_folder(root, &IndexOptions::default(), |_, _| {}).unwrap();
        assert_eq!(report.files_probed, 1);
        let resumed = Mutex::new(MetadataProgress::default());
        run_metadata_worker(root, 1, &generation, &cancel, &resumed).unwrap();
        assert_eq!(resumed.into_inner().unwrap().total, 1);
        assert_eq!(db.metadata_candidate_count().unwrap(), 0);
        assert_eq!(db.search(&SearchParams::new("thunder")).unwrap().len(), 1);
        assert_eq!(db.search(&SearchParams::new("rusty")).unwrap()[0].ucs_cat_id, row.ucs_cat_id);

        db.set_meta(mio_index::db::METADATA_BACKFILL_KEY, "old-rule").unwrap();
        db.prepare_metadata_pass().unwrap();
        assert_eq!(db.metadata_candidate_count().unwrap(), 3);
        db.mark_metadata_failed(&db.metadata_candidates(1).unwrap()[0]).unwrap();
        db.prepare_metadata_pass().unwrap();
        assert_eq!(db.metadata_candidate_count().unwrap(), 2);
    }

    #[test]
    fn closing_a_library_joins_its_metadata_worker() {
        let dir = tempfile::tempdir().unwrap();
        let library = Arc::new(Library::open(dir.path()).unwrap());
        let mut task = library.metadata.lock().unwrap();
        let cancel = Arc::clone(&task.cancel);
        let (started, ready) = std::sync::mpsc::channel();
        let (closed, stopped) = std::sync::mpsc::channel();
        let root = dir.path().to_path_buf();
        task.worker = Some(std::thread::spawn(move || {
            let db = IndexDb::open_fast(root).unwrap();
            started.send(()).unwrap();
            while !cancel.load(Ordering::Acquire) {
                std::thread::park_timeout(std::time::Duration::from_millis(1));
            }
            drop(db);
            closed.send(()).unwrap();
        }));
        drop(task);
        ready.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        library.close();
        stopped.try_recv().unwrap();
        assert!(library.metadata.lock().unwrap().worker.is_none());
    }
}
