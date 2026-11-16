// Indexing throughput probe.
//
// Measures the three stages that dominate a first-time library index:
//   1. directory walk (readdir recursion)
//   2. per-file metadata: stat + read the header bytes needed for duration,
//      channels, sample rate
//   3. SQLite insert, single transaction vs WAL batches
//
// The point is to find out whether the work is IO bound (so more threads do
// not help) or CPU bound, and what the 170k-file target should be.
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rayon::prelude::*;
use rusqlite::{params, Connection};

/// Read just enough of a file to learn duration / channels / rate, without
/// decoding. symphonia probes the container header; we stop at the first packet.
fn probe_meta(path: &Path) -> (Option<f64>, Option<u32>, Option<u32>, u64) {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let hint = symphonia::core::io::MediaSourceStream::new(
        Box::new(match File::open(path) {
            Ok(f) => f,
            Err(_) => return (None, None, None, size),
        }),
        Default::default(),
    );
    let fmt_opts = symphonia::core::formats::FormatOptions::default();
    let meta_opts = symphonia::core::meta::MetadataOptions::default();
    let codec_opts = symphonia::core::codecs::DecoderOptions::default();

    let mut probed = match symphonia::default::get_probe().format(
        &Default::default(),
        hint,
        &fmt_opts,
        &meta_opts,
    ) {
        Ok(p) => p,
        Err(_) => return (None, None, None, size),
    };

    let track = probed.format.default_track().cloned();
    let Some(track) = track else {
        return (None, None, None, size);
    };
    let sr = track.codec_params.sample_rate;
    let ch = track.codec_params.channels.map(|c| c.count() as u32);
    let duration = track
        .codec_params
        .n_frames
        .or(track.codec_params.n_frames)
        .map(|n| sr.map(|s| n as f64 / s as f64))
        .flatten();
    (duration, ch, sr, size)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = args.get(1).cloned().unwrap_or_else(|| ".".into());
    let root = PathBuf::from(root);
    if !root.exists() {
        eprintln!("no such dir: {}", root.display());
        return;
    }

    // 1. Walk.
    let t = Instant::now();
    let files: Vec<PathBuf> = walkdir::WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let ext = e
                .path()
                .extension()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .to_lowercase();
            matches!(
                ext.as_str(),
                "wav" | "aif" | "aiff" | "flac" | "mp3" | "ogg" | "m4a" | "aac" | "opus"
            )
        })
        .map(|e| e.into_path())
        .collect();
    let walk_ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "walk: {} audio files in {:.1}ms ({:.0} files/s)",
        files.len(),
        walk_ms,
        files.len() as f64 / (walk_ms / 1000.0).max(1e-9)
    );
    if files.is_empty() {
        return;
    }

    // 2. Metadata, single threaded vs parallel.
    for threads in [1usize, 2, 4, 8, 16] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let t = Instant::now();
        let counter = AtomicUsize::new(0);
        pool.install(|| {
            files.par_iter().for_each(|p| {
                let (d, _, _, _) = probe_meta(p);
                if d.is_some() {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            });
        });
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "meta probe ({threads:>2} threads): {:.1}ms  {:.0} files/s  (duration parsed: {}/{})",
            ms,
            files.len() as f64 / (ms / 1000.0).max(1e-9),
            counter.load(Ordering::Relaxed),
            files.len()
        );
    }

    // 3. SQLite: one transaction vs per-row, and WAL vs delete.
    let db_path = std::env::temp_dir().join("index-probe-a.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE sounds (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT UNIQUE NOT NULL, filename TEXT NOT NULL,
            duration REAL, channels INTEGER, sample_rate INTEGER, filesize INTEGER,
            category TEXT NOT NULL DEFAULT 'Uncategorized', subcategory TEXT,
            confidence REAL NOT NULL DEFAULT 0, matched_terms TEXT,
            favorite INTEGER NOT NULL DEFAULT 0, category_manual INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX idx_duration ON sounds(duration);
        CREATE INDEX idx_channels ON sounds(channels);",
    )
    .unwrap();

    let t = Instant::now();
    {
        let mut stmt = conn
            .prepare(
                "INSERT OR REPLACE INTO sounds (path, filename, duration, channels, sample_rate, filesize)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .unwrap();
        conn.execute_batch("BEGIN").unwrap();
        for p in &files {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            stmt.execute(params![
                p.to_string_lossy(),
                name,
                Some(1.5f64),
                Some(2i64),
                Some(44100i64),
                Some(1234i64)
            ])
            .unwrap();
        }
        conn.execute_batch("COMMIT").unwrap();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "sqlite insert (1 txn, default journal): {:.1}ms  {:.0} rows/s",
        ms,
        files.len() as f64 / (ms / 1000.0).max(1e-9)
    );

    // WAL + one txn, the shape a real indexer would use.
    let db_path = std::env::temp_dir().join("index-probe-b.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
         CREATE TABLE sounds (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT UNIQUE NOT NULL, filename TEXT NOT NULL,
            duration REAL, channels INTEGER, sample_rate INTEGER, filesize INTEGER,
            category TEXT NOT NULL DEFAULT 'Uncategorized', subcategory TEXT,
            confidence REAL NOT NULL DEFAULT 0, matched_terms TEXT,
            favorite INTEGER NOT NULL DEFAULT 0, category_manual INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX idx_duration ON sounds(duration);
        CREATE INDEX idx_channels ON sounds(channels);",
    )
    .unwrap();
    let t = Instant::now();
    {
        let mut stmt = conn
            .prepare(
                "INSERT OR REPLACE INTO sounds (path, filename, duration, channels, sample_rate, filesize)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .unwrap();
        conn.execute_batch("BEGIN").unwrap();
        for p in &files {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            stmt.execute(params![
                p.to_string_lossy(),
                name,
                Some(1.5f64),
                Some(2i64),
                Some(44100i64),
                Some(1234i64)
            ])
            .unwrap();
        }
        conn.execute_batch("COMMIT").unwrap();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "sqlite insert (1 txn, WAL): {:.1}ms  {:.0} rows/s",
        ms,
        files.len() as f64 / (ms / 1000.0).max(1e-9)
    );

    // 4. Batched commits: 5000 rows per transaction, the shape that lets the
    // UI query mid-index.
    let db_path = std::env::temp_dir().join("index-probe-c.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
         CREATE TABLE sounds (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT UNIQUE NOT NULL, filename TEXT NOT NULL,
            duration REAL, channels INTEGER, sample_rate INTEGER, filesize INTEGER,
            category TEXT NOT NULL DEFAULT 'Uncategorized', subcategory TEXT,
            confidence REAL NOT NULL DEFAULT 0, matched_terms TEXT,
            favorite INTEGER NOT NULL DEFAULT 0, category_manual INTEGER NOT NULL DEFAULT 0
        );",
    )
    .unwrap();
    let t = Instant::now();
    {
        let mut stmt = conn
            .prepare(
                "INSERT OR REPLACE INTO sounds (path, filename, duration, channels, sample_rate, filesize)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .unwrap();
        for chunk in files.chunks(5000) {
            conn.execute_batch("BEGIN").unwrap();
            for p in chunk {
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                stmt.execute(params![
                    p.to_string_lossy(),
                    name,
                    Some(1.5f64),
                    Some(2i64),
                    Some(44100i64),
                    Some(1234i64)
                ])
                .unwrap();
            }
            conn.execute_batch("COMMIT").unwrap();
        }
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "sqlite insert (5k-row txns, WAL, no indexes): {:.1}ms  {:.0} rows/s",
        ms,
        files.len() as f64 / (ms / 1000.0).max(1e-9)
    );

    let t = Instant::now();
    conn.execute_batch("CREATE INDEX idx_duration ON sounds(duration); CREATE INDEX idx_channels ON sounds(channels);")
        .unwrap();
    println!(
        "index build after load ({} rows): {:.1}ms",
        files.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );

    drop(conn);
    for p in ["a", "b", "c"] {
        let _ = std::fs::remove_file(std::env::temp_dir().join(format!("index-probe-{p}.db")));
        let _ = std::fs::remove_file(std::env::temp_dir().join(format!("index-probe-{p}.db-wal")));
        let _ = std::fs::remove_file(std::env::temp_dir().join(format!("index-probe-{p}.db-shm")));
    }
}
