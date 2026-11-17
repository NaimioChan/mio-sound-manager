//! Walk → probe → classify → write.
//!
//! The three stages are timed separately because they fail in different ways and
//! are fixed in different ways: a slow walk means too many files, a slow probe
//! means too few threads or a slow disk, a slow write means a bad transaction
//! shape. Reporting one total would hide which.
//!
//! Parallelism is at the probe stage only. The walk is a single pass (it is IO
//! bound on directory entries and already fast), and SQLite writes are serial by
//! definition. Probing is where 8 threads buy 3.2x, because it is 8 independent
//! header reads per millisecond.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use rayon::prelude::*;

use crate::classify::Classifier;
use crate::db::{IndexDb, SoundRow, row_from_parts};
use crate::error::Result;
use crate::probe::{AudioMeta, FileEntry, probe_with_meta, walk_audio_entries};

/// How the indexer should behave on this pass.
#[derive(Debug, Clone)]
pub struct IndexOptions {
    /// Probe threads. 0 means "as many as the machine has logical cores".
    ///
    /// The cap of 8 in the architecture document was based on a synthetic library
    /// on an NVMe drive. On a real 280k-file library on a DRAM-less SSD, going
    /// from 8 to 16 threads still bought 22% (4,347 → 5,331 files/s): the probe is
    /// latency-bound on random header reads, not CPU-bound, so more threads in
    /// flight means more requests the drive can reorder. Do not lower this to 8
    /// without measuring.
    pub threads: usize,
    /// Rows per write transaction. 5000 was measured at 156k rows/s, and it keeps
    /// the WAL small enough that a reader is never blocked for long.
    pub batch_size: usize,
    /// Skip files whose `(mtime, size)` matches what is already indexed. The
    /// first pass must have this off; every later pass wants it on.
    pub incremental: bool,
    /// Delete rows for files that are gone. Off during a partial rescan of a
    /// subfolder, where "not seen" does not mean "deleted".
    pub prune: bool,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            threads: 0,
            batch_size: 5_000,
            incremental: true,
            prune: true,
        }
    }
}

/// Per-stage wall time, plus counts. This is what gets printed and compared
/// against `docs/architecture.md` section 5.
#[derive(Debug, Clone, Default)]
pub struct StageTimings {
    pub walk_ms: f64,
    pub probe_ms: f64,
    pub classify_ms: f64,
    pub write_ms: f64,
    pub prune_ms: f64,
    pub index_ms: f64,
}

impl StageTimings {
    pub fn total_ms(&self) -> f64 {
        self.walk_ms
            + self.probe_ms
            + self.classify_ms
            + self.write_ms
            + self.prune_ms
            + self.index_ms
    }
}

#[derive(Debug, Clone, Default)]
pub struct IndexReport {
    /// Files found on disk.
    pub files_seen: usize,
    /// Files actually probed (incremental passes skip unchanged ones).
    pub files_probed: usize,
    /// Files skipped because `(mtime, size)` matched the index.
    pub files_skipped: usize,
    /// Rows written.
    pub rows_written: usize,
    /// Rows deleted because the file is gone.
    pub rows_pruned: u64,
    pub threads: usize,
    pub timings: StageTimings,
}

impl IndexReport {
    /// Rows per second over the whole pass, the number that is comparable across
    /// library sizes.
    pub fn rows_per_second(&self) -> f64 {
        let secs = self.timings.total_ms() / 1000.0;
        if secs > 0.0 {
            self.rows_written as f64 / secs
        } else {
            0.0
        }
    }
}

/// Index every audio file under `root` into the database at `root/slip-sound.db`.
///
/// `on_progress(done, total)` is called after each batch, so a UI can show a
/// bar; it runs on the calling thread, never inside the parallel section.
pub fn index_folder<F>(
    root: impl AsRef<Path>,
    options: &IndexOptions,
    mut on_progress: F,
) -> Result<IndexReport>
where
    F: FnMut(usize, usize),
{
    let root = root.as_ref().to_path_buf();
    let mut db = IndexDb::open(&root)?;
    let classifier = Arc::new(Classifier::new());
    let mut report = IndexReport {
        threads: effective_threads(options.threads),
        ..Default::default()
    };

    // 1. Walk. The entries carry size and mtime from the directory scan, so the
    //    incremental check and the row both avoid a second `stat`.
    let t = Instant::now();
    let files = walk_audio_entries(&root);
    report.timings.walk_ms = ms(t);
    report.files_seen = files.len();

    // Which files can be skipped: unchanged means the recorded mtime matches.
    // NTFS stores it at 100ns resolution, so an edit that keeps the size still
    // changes it.
    let known = if options.incremental {
        db.path_mtimes()?
    } else {
        Vec::new()
    };
    let known: std::collections::HashMap<String, i64> = known.into_iter().collect();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(report.threads)
        .thread_name(|i| format!("reslip-probe-{i}"))
        .build()
        .map_err(|e| crate::error::IndexError::Db(format!("thread pool: {e}")))?;

    // 2. Probe, in parallel, in chunks so progress is reported and memory stays
    //    bounded regardless of library size.
    let probe_started = Instant::now();
    let classify_started = AtomicU64::new(0);
    let mut classify_ns = 0u64;
    let mut rows_written = 0usize;
    let mut skipped = 0usize;
    let chunk = options.batch_size.max(1);

    // Trigger-maintained FTS costs a tokenizer pass per insert and per delete.
    // On a full pass over a large library that dominated the write stage, so the
    // triggers come off and one rebuild runs at the end. An incremental pass
    // touches few rows and keeps them on.
    let bulk = !options.incremental;
    if bulk {
        db.begin_bulk()?;
    }

    for batch in files.chunks(chunk) {
        let (to_probe, skipped_here): (Vec<&FileEntry>, usize) =
            batch
                .iter()
                .fold((Vec::new(), 0usize), |(mut acc, skipped), entry| {
                    let unchanged = known
                        .get(&crate::folders::store_path(&entry.path))
                        .map(|mtime| *mtime != 0 && entry.mtime_ns == *mtime)
                        .unwrap_or(false);
                    if options.incremental && unchanged {
                        (acc, skipped + 1)
                    } else {
                        acc.push(entry);
                        (acc, skipped)
                    }
                });
        skipped += skipped_here;
        if to_probe.is_empty() {
            continue;
        }

        // Probe in parallel; classify on the same worker right after, since both
        // are CPU work on the same file and moving to another pool would only add
        // a hop.
        let built: Vec<(AudioMeta, crate::classify::Classification, PathBuf)> =
            pool.install(|| {
                to_probe
                    .par_iter()
                    .map(|entry| {
                        // Size and mtime came from the directory scan; passing
                        // them in is what keeps the probe to one file open.
                        let meta = probe_with_meta(&entry.path, entry.filesize, entry.mtime_ns);
                        let cstart = Instant::now();
                        let classification = classifier.classify(&entry.path.to_string_lossy());
                        let _ = classify_started
                            .fetch_add(cstart.elapsed().as_nanos() as u64, Ordering::Relaxed);
                        (meta, classification, entry.path.clone())
                    })
                    .collect()
            });
        classify_ns += classify_started.swap(0, Ordering::Relaxed);

        let rows: Vec<SoundRow> = built
            .iter()
            .map(|(meta, classification, path)| row_from_parts(&root, path, meta, classification))
            .collect();

        // 3. Write.
        let w = Instant::now();
        db.upsert_batch(&rows)?;
        report.timings.write_ms += ms(w);

        rows_written += rows.len();
        report.files_probed += rows.len();
        on_progress(rows_written + skipped, files.len());
    }
    report.timings.probe_ms =
        ms(probe_started) - report.timings.write_ms - classify_ns as f64 / 1e6;
    report.timings.classify_ms = classify_ns as f64 / 1e6;

    report.files_skipped = skipped;
    report.rows_written = rows_written;

    // 4. Prune. Only meaningful on a full pass: a partial scan has not seen the
    //    rest of the library, so absence proves nothing.
    if options.prune && !options.incremental {
        let p = Instant::now();
        let paths: Vec<String> = files
            .iter()
            .map(|entry| crate::folders::store_path(&entry.path))
            .collect();
        report.rows_pruned = db.prune_missing(&paths)?;
        report.timings.prune_ms = ms(p);
    }

    // 5. Rebuild the FTS index that the bulk load bypassed.
    if bulk {
        let i = Instant::now();
        db.end_bulk()?;
        report.timings.index_ms = ms(i);
    }

    Ok(report)
}

fn effective_threads(requested: usize) -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    if requested == 0 { cores } else { requested }
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_lib(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("reslip-index-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_wav(path: &Path, frames: u32) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let data_len = frames * 4;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&44_100u32.to_le_bytes());
        out.extend_from_slice(&176_400u32.to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.resize(44 + data_len as usize, 0);
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(&out).unwrap();
    }

    #[test]
    fn indexes_a_small_library_end_to_end() {
        let root = temp_lib("end-to-end");
        write_wav(&root.join(r"Foley\door_slam.wav"), 4_410);
        write_wav(&root.join(r"Weapons\Firearms\gunshot_01.wav"), 8_820);
        std::fs::write(root.join("readme.txt"), b"not audio").unwrap();

        let report = index_folder(&root, &IndexOptions::default(), |_, _| {}).unwrap();
        assert_eq!(report.files_seen, 2, "the .txt must not be indexed");
        assert_eq!(report.rows_written, 2);

        let db = IndexDb::open(&root).unwrap();
        assert_eq!(db.count().unwrap(), 2);
        let door = db
            .get_by_path(&root.join(r"Foley\door_slam.wav").to_string_lossy())
            .unwrap()
            .unwrap();
        // Both words are index terms and both carry the same weight: `door` is
        // Foley/Doors & Locks and `slam` is Impacts/Hard Hits. A door slam is a
        // door sound, and `door` is the first token in the name, so Foley wins.
        assert_eq!(door.category, "Foley");
        assert_eq!(door.subcategory.as_deref(), Some("Doors & Locks"));
        let duration = door.duration_s.unwrap();
        assert!((duration - 0.1).abs() < 0.001, "duration {duration}");

        let gun = db
            .get_by_path(
                &root
                    .join(r"Weapons\Firearms\gunshot_01.wav")
                    .to_string_lossy(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(gun.category, "Weapons");
        assert_eq!(gun.subcategory.as_deref(), Some("Firearms"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_incremental_pass_probes_nothing() {
        let root = temp_lib("incremental");
        write_wav(&root.join("a.wav"), 1_000);
        write_wav(&root.join("b.wav"), 1_000);

        let first = index_folder(
            &root,
            &IndexOptions {
                incremental: false,
                ..Default::default()
            },
            |_, _| {},
        )
        .unwrap();
        assert_eq!(first.rows_written, 2);

        let second = index_folder(&root, &IndexOptions::default(), |_, _| {}).unwrap();
        assert_eq!(second.files_seen, 2);
        assert_eq!(second.rows_written, 0, "unchanged files must be skipped");
        assert_eq!(second.files_skipped, 2);

        // Touch one file; exactly one must be reprobed.
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_wav(&root.join("b.wav"), 2_000);
        let third = index_folder(&root, &IndexOptions::default(), |_, _| {}).unwrap();
        assert_eq!(third.rows_written, 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_full_pass_prunes_deleted_files() {
        let root = temp_lib("prune");
        write_wav(&root.join("a.wav"), 100);
        write_wav(&root.join("b.wav"), 100);
        index_folder(
            &root,
            &IndexOptions {
                incremental: false,
                ..Default::default()
            },
            |_, _| {},
        )
        .unwrap();

        std::fs::remove_file(root.join("b.wav")).unwrap();
        let report = index_folder(
            &root,
            &IndexOptions {
                incremental: false,
                ..Default::default()
            },
            |_, _| {},
        )
        .unwrap();
        assert_eq!(report.rows_pruned, 1);

        let db = IndexDb::open(&root).unwrap();
        assert_eq!(db.count().unwrap(), 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_cannot_be_probed_is_still_indexed() {
        let root = temp_lib("broken");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("broken.wav"), b"garbage").unwrap();

        let report = index_folder(&root, &IndexOptions::default(), |_, _| {}).unwrap();
        assert_eq!(report.rows_written, 1);
        let db = IndexDb::open(&root).unwrap();
        let sound = db
            .get_by_path(&root.join("broken.wav").to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(sound.duration_s, None);
        assert!(sound.filesize.unwrap() > 0);

        let _ = std::fs::remove_dir_all(&root);
    }
}
