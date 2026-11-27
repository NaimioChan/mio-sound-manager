//! Index commands for the acceptance harness.
//!
//! `P2` in `docs/architecture.md` section 8 asks for two things: run the indexer
//! over a real library and print the per-stage timings so they can be compared
//! against section 5's numbers, and prove the ported classifier agrees with the
//! reference implementation.
//!
//!   mio-cli index <folder> [--full] [--threads N] [--batch N]
//!   mio-cli index-stats <folder>
//!   mio-cli search <folder> <query> [--limit N]
//!   mio-cli classify-diff <folder> <reference-db>
//!
//! `classify-diff` re-classifies every path in an existing database with the
//! current dictionary and reports every row whose category, subcategory,
//! confidence or matched terms differ. It was written as a parity check against
//! the reference implementation's index; now that the vocabulary is this
//! project's own, it is the tool for asking "what did the new dictionary change"
//! — useful before and after a vocabulary revision.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};

use mio_index::{
    Classifier, IndexDb, IndexOptions, SearchParams, SortColumn, SortDir, index_folder,
};

pub struct IndexArgs {
    pub folder: PathBuf,
    pub full: bool,
    pub threads: usize,
    pub batch_size: usize,
    pub scan_batch_size: usize,
    pub basic: bool,
    pub limit: u32,
    pub query: String,
    pub reference_db: Option<PathBuf>,
}

pub fn run_index(args: &IndexArgs) -> Result<()> {
    let options = IndexOptions {
        threads: args.threads,
        batch_size: args.batch_size,
        scan_batch_size: args.scan_batch_size,
        // `--full` means "re-probe everything and prune"; the default is the
        // incremental pass a UI would run on startup.
        incremental: !args.full,
        prune: args.full,
        read_metadata: !args.basic,
    };

    println!("folder    {}", args.folder.display());
    println!(
        "mode      {} (threads {}, batch {}, scan batch {}, embedded metadata {})",
        if args.full { "full" } else { "incremental" },
        options.threads,
        options.batch_size,
        options.scan_batch_size,
        if options.read_metadata { "on" } else { "off" },
    );

    let started = Instant::now();
    let mut last_pct = 0u32;
    let report = index_folder(&args.folder, &options, |done, total| {
        if total == 0 {
            return;
        }
        let pct = (done * 100 / total) as u32;
        if pct >= last_pct + 10 {
            last_pct = pct;
            eprint!("\r  probing {pct:>3}% ({done}/{total})");
        }
    })?;
    eprintln!("\r{:60}", "");

    let wall = started.elapsed().as_secs_f64() * 1000.0;
    let t = &report.timings;

    println!();
    println!("files on disk        {}", report.files_seen);
    println!("files probed         {}", report.files_probed);
    println!("files skipped        {}", report.files_skipped);
    println!("rows written         {}", report.rows_written);
    println!("rows pruned          {}", report.rows_pruned);
    println!("directories skipped  {}", report.skipped_dirs.len());
    for skipped in &report.skipped_dirs {
        println!("  skipped {}: {}", skipped.path, skipped.detail);
    }
    println!();
    println!("stage timings");
    println!("  walk               {:>9.1} ms", t.walk_ms);
    println!("  scan table setup   {:>9.1} ms", t.scan_setup_ms);
    println!("  scan table writes  {:>9.1} ms", t.scan_write_ms);
    println!("  compare            {:>9.1} ms", t.compare_ms);
    println!("  scan table cleanup {:>9.1} ms", t.scan_cleanup_ms);
    println!("  probe + classify   {:>9.1} ms (wall)", t.probe_ms);
    println!("  classify           {:>9.1} ms (worker sum)", t.classify_ms);
    println!("  write              {:>9.1} ms", t.write_ms);
    println!("  prune              {:>9.1} ms", t.prune_ms);
    println!("  bulk recovery      {:>9.1} ms", t.index_ms);
    println!("    secondary indexes {:>7.1} ms", t.bulk.secondary_indexes_ms);
    println!("    triggers          {:>7.1} ms", t.bulk.triggers_ms);
    println!("    prefix FTS        {:>7.1} ms", t.bulk.fts_ms);
    println!("    trigram FTS       {:>7.1} ms", t.bulk.trigram_ms);
    println!("  ------------------------------");
    println!("  total (stages)     {:>9.1} ms", t.total_ms());
    println!("  total (wall)       {:>9.1} ms", wall);
    if t.total_ms() > 0.0 {
        println!(
            "  throughput         {:>9.0} files/s",
            report.rows_written as f64 / (t.total_ms() / 1000.0)
        );
    }

    // The reference numbers from `docs/architecture.md` section 5.1, for the
    // comparison the stage exists to make. The comparison is not favourable and
    // the reason is recorded in `docs/agent-log.md`: the 9 s figure extrapolates
    // from a synthetic 20k-file library of pre-generated files on an NVMe drive,
    // while this is 280k real files on a DRAM-less SSD.
    println!();
    println!("architecture.md 5.1 targets (2万文件 synthetic, NVMe)");
    println!("  walk 108ms · probe 795ms (8 threads) · write 128ms · index 17ms ≈ 1.1s total");
    println!("  extrapolated 17万: walk ~1.0s · probe ~6.8s · write ~1.2s · index ~0.15s ≈ 9s");
    println!("  see docs/agent-log.md for why a real library does not match");

    Ok(())
}

pub fn run_index_stats(args: &IndexArgs) -> Result<()> {
    let db = IndexDb::open(&args.folder)?;
    println!("database  {}", db.path().display());
    println!("rows      {}", db.count()?);

    let (described, shared, native_ucs, explicit_cat_ids) = db.connection().query_row(
        "SELECT
             COALESCE(SUM(description IS NOT NULL), 0),
             COALESCE((SELECT SUM(sample_count) FROM (
                 SELECT COUNT(*) AS sample_count
                 FROM sounds
                 WHERE description IS NOT NULL
                 GROUP BY description
                 HAVING COUNT(*) > 1
             )), 0),
             COALESCE(SUM(ucs_category IS NOT NULL), 0),
             COALESCE(SUM(ucs_cat_id IS NOT NULL), 0)
         FROM sounds",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        },
    )?;
    let rows = db.count()? as f64;
    println!(
        "metadata description {} / {} ({:.2}%), shared-description rows {}, native UCS category {} / {} ({:.2}%), explicit CatID {}",
        described,
        rows as u64,
        if rows > 0.0 { described as f64 * 100.0 / rows } else { 0.0 },
        shared,
        native_ucs,
        rows as u64,
        if rows > 0.0 { native_ucs as f64 * 100.0 / rows } else { 0.0 },
        explicit_cat_ids,
    );

    let facets = db.facets()?;
    println!("facets    {} (category, subcategory) pairs", facets.len());
    for facet in facets.iter().take(15) {
        match &facet.subcategory {
            Some(sub) => println!("  {:<28} {:>8}  ({})", facet.category, facet.count, sub),
            None => println!("  {:<28} {:>8}", facet.category, facet.count),
        }
    }

    let tree = db.folder_tree()?;
    println!(
        "folders   {} top-level under {}",
        tree.children.len(),
        tree.path
    );
    for child in tree.children.iter().take(10) {
        println!("  {:<40} {:>8}", child.name, child.count);
    }
    Ok(())
}

pub fn run_classify_bench(args: &IndexArgs) -> Result<()> {
    let db = IndexDb::open(&args.folder)?;
    let mut stmt = db
        .connection()
        .prepare("SELECT path FROM sounds ORDER BY path")?;
    let paths = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let classifier = Classifier::new();
    let started = Instant::now();
    let mut count = 0usize;
    let mut checksum = 0u64;
    for path in paths {
        let path = path?;
        let result = classifier.classify(&path);
        checksum = checksum.wrapping_add(result.raw_score as u64);
        count += 1;
        if args.limit > 0 && count >= args.limit as usize {
            break;
        }
    }
    let elapsed = started.elapsed();
    println!(
        "classified {count} paths in {:.1} ms",
        elapsed.as_secs_f64() * 1000.0
    );
    println!(
        "throughput {:.0} paths/s",
        count as f64 / elapsed.as_secs_f64()
    );
    println!("checksum {checksum}");
    Ok(())
}

pub fn run_search(args: &IndexArgs) -> Result<()> {
    let db = IndexDb::open(&args.folder)?;
    let mut params = SearchParams::new(&args.query);
    params.limit = args.limit;
    params.sort_by = Some(SortColumn::Filename);
    params.sort_dir = Some(SortDir::Asc);

    let started = Instant::now();
    let hits = db.search(&params)?;
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let total = db.count_matching(&params)?;

    println!(
        "{} match{} for {:?} in {:.2} ms (showing {})",
        total,
        if total == 1 { "" } else { "es" },
        args.query,
        elapsed,
        hits.len()
    );
    for hit in &hits {
        println!(
            "  {:<12} {:<24} {:>7} {:>5}",
            hit.category,
            hit.filename,
            hit.duration_s
                .map(|d| format!("{d:.2}s"))
                .unwrap_or_else(|| "—".into()),
            hit.channels
                .map(|c| format!("{c}ch"))
                .unwrap_or_else(|| "—".into()),
        );
    }
    Ok(())
}

/// Re-classify every path in an existing database and report disagreements.
///
/// This is the check that makes "the classifier is a faithful port" a measured
/// claim instead of an assertion. Rows whose category was assigned by hand are
/// skipped: the reference would not re-derive those either.
pub fn run_classify_diff(args: &IndexArgs) -> Result<()> {
    let reference_db = args
        .reference_db
        .as_ref()
        .context("classify-diff needs a reference database path")?;

    let conn = rusqlite::Connection::open_with_flags(
        reference_db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .with_context(|| format!("opening {}", reference_db.display()))?;

    let classifier = Classifier::new();
    let started = Instant::now();

    let mut stmt = conn.prepare(
        "SELECT path, category, subcategory, confidence, matched_terms, category_manual
         FROM sounds",
    )?;
    let mut rows = stmt.query([])?;

    let mut total = 0u64;
    let mut checked = 0u64;
    let mut manual = 0u64;
    let mut category_mismatch = 0u64;
    let mut subcategory_mismatch = 0u64;
    let mut confidence_mismatch = 0u64;
    let mut terms_mismatch = 0u64;
    let mut examples: Vec<String> = Vec::new();

    while let Some(row) = rows.next()? {
        total += 1;
        let path: String = row.get(0)?;
        let old_category: String = row.get(1)?;
        let old_subcategory: Option<String> = row.get(2)?;
        let old_confidence: f64 = row.get(3)?;
        let old_terms_raw: Option<String> = row.get(4)?;
        let category_manual: i64 = row.get(5)?;

        if category_manual == 1 {
            manual += 1;
            continue;
        }
        checked += 1;

        let new = classifier.classify(&path);
        let old_terms: Vec<String> = old_terms_raw
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        let mut diffs: Vec<String> = Vec::new();
        if new.category != old_category {
            category_mismatch += 1;
            diffs.push(format!("category {:?} -> {:?}", old_category, new.category));
        }
        if new.subcategory != old_subcategory {
            subcategory_mismatch += 1;
            diffs.push(format!(
                "subcategory {:?} -> {:?}",
                old_subcategory, new.subcategory
            ));
        }
        // The reference stored confidence as a float; compare with a tolerance
        // because both sides divide by 8 and JSON round-trips through decimal.
        if (new.confidence - old_confidence).abs() > 1e-9 {
            confidence_mismatch += 1;
            diffs.push(format!(
                "confidence {:.6} -> {:.6}",
                old_confidence, new.confidence
            ));
        }
        if new.matched_terms != old_terms {
            terms_mismatch += 1;
            diffs.push(format!("terms {:?} -> {:?}", old_terms, new.matched_terms));
        }

        if !diffs.is_empty() && examples.len() < 20 {
            examples.push(format!("  {path}\n    {}", diffs.join("\n    ")));
        }
    }

    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    println!("reference   {}", reference_db.display());
    println!("rows        {total} ({checked} checked, {manual} manual, skipped)");
    println!("elapsed     {elapsed:.1} ms");
    println!();
    println!("mismatches");
    println!("  category      {category_mismatch}");
    println!("  subcategory   {subcategory_mismatch}");
    println!("  confidence    {confidence_mismatch}");
    println!("  matched terms {terms_mismatch}");

    if !examples.is_empty() {
        println!();
        println!("first {} disagreements", examples.len());
        for example in &examples {
            println!("{example}");
        }
    }

    if category_mismatch == 0 && subcategory_mismatch == 0 && confidence_mismatch == 0 {
        println!();
        println!("PARITY OK: the Rust classifier agrees with the reference on every checked row");
        Ok(())
    } else {
        anyhow::bail!(
            "parity FAILED: {category_mismatch} category, {subcategory_mismatch} subcategory, \
             {confidence_mismatch} confidence mismatches out of {checked} rows"
        )
    }
}

/// Time `probe_file` on a sample of the library, grouped by extension.
///
/// The full pass reports one aggregate probe number, which cannot say whether a
/// format is slow, the disk is slow, or the antivirus is intercepting every open.
/// This measures three things on the same sample: `stat`, opening the file and
/// reading its first 64 KB (what a probe does before symphonia sees anything),
/// and the full probe. The difference between the second and third is decoding
/// and demuxing; the difference between the first and second is the OS.
pub fn run_probe_bench(args: &IndexArgs) -> Result<()> {
    let files = mio_index::walk_audio_files(&args.folder)?;
    if files.is_empty() {
        anyhow::bail!("no audio files under {}", args.folder.display());
    }

    // Deterministic stride sample, so two runs measure the same files and the
    // numbers are comparable.
    let want = args.limit.max(50) as usize;
    let stride = (files.len() / want).max(1);
    let sample: Vec<&PathBuf> = files.iter().step_by(stride).collect();

    let mut by_ext: std::collections::BTreeMap<String, Vec<f64>> =
        std::collections::BTreeMap::new();
    let mut with_duration = 0usize;
    let mut with_channels = 0usize;
    let started = Instant::now();

    // Where the time goes, measured separately on the same sample: stat, open +
    // read the first 64 KB, then the whole probe. A big gap between (2) and (3)
    // is demuxing; a big gap between (1) and (2) is the filesystem or an
    // antivirus filter.
    let mut stat_ms = 0.0f64;
    let mut read_ms = 0.0f64;

    // Parallel or serial, same sample and same measurement, so the difference
    // between the two numbers is the thread pool's actual scaling on this disk.
    let threads = args.threads.max(1);
    let results: Vec<(String, f64, bool, bool, f64, f64)> = if threads == 1 {
        sample
            .iter()
            .map(|path| {
                let s = Instant::now();
                let _ = std::fs::metadata(path);
                let stat_here = s.elapsed().as_secs_f64() * 1000.0;

                let r = Instant::now();
                read_head(path);
                let read_here = r.elapsed().as_secs_f64() * 1000.0;

                let t = Instant::now();
                let meta = mio_index::probe_file(path);
                let elapsed = t.elapsed().as_secs_f64() * 1000.0;
                (
                    ext_of(path),
                    elapsed,
                    meta.duration_s.is_some(),
                    meta.channels.is_some(),
                    stat_here,
                    read_here,
                )
            })
            .collect()
    } else {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .map_err(|e| anyhow::anyhow!("thread pool: {e}"))?;
        pool.install(|| {
            sample
                .par_iter()
                .map(|path| {
                    let s = Instant::now();
                    let _ = std::fs::metadata(path);
                    let stat_here = s.elapsed().as_secs_f64() * 1000.0;

                    let r = Instant::now();
                    read_head(path);
                    let read_here = r.elapsed().as_secs_f64() * 1000.0;

                    let t = Instant::now();
                    let meta = mio_index::probe_file(path);
                    let elapsed = t.elapsed().as_secs_f64() * 1000.0;
                    (
                        ext_of(path),
                        elapsed,
                        meta.duration_s.is_some(),
                        meta.channels.is_some(),
                        stat_here,
                        read_here,
                    )
                })
                .collect()
        })
    };

    for (ext, elapsed, has_dur, has_ch, stat_here, read_here) in results {
        by_ext.entry(ext).or_default().push(elapsed);
        stat_ms += stat_here;
        read_ms += read_here;
        if has_dur {
            with_duration += 1;
        }
        if has_ch {
            with_channels += 1;
        }
    }

    let total_ms = started.elapsed().as_secs_f64() * 1000.0;
    let per_file = total_ms / sample.len() as f64;
    println!(
        "sampled {} of {} files (stride {stride}), {threads} thread(s)",
        sample.len(),
        files.len()
    );
    println!("wall total {total_ms:.1} ms, {per_file:.3} ms/file");
    println!(
        "throughput {:.0} files/s",
        sample.len() as f64 / (total_ms / 1000.0)
    );
    println!(
        "fields: duration {with_duration}/{} · channels {with_channels}/{}",
        sample.len(),
        sample.len()
    );
    println!();
    println!(
        "time split (summed over the sample, so /{n} for per-file)",
        n = sample.len()
    );
    println!("  stat only          {stat_ms:>9.1} ms");
    println!("  open + 64KB read   {read_ms:>9.1} ms");
    println!("  full probe         {:>9.1} ms", total_ms);
    println!();
    println!(
        "{:<8} {:>7} {:>9} {:>9} {:>9}",
        "ext", "count", "p50 ms", "p90 ms", "max ms"
    );
    for (ext, mut times) in by_ext {
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
        println!(
            "{:<8} {:>7} {:>9.3} {:>9.3} {:>9.3}",
            ext,
            times.len(),
            p(0.5),
            p(0.9),
            times[times.len() - 1]
        );
    }
    Ok(())
}

use rayon::prelude::*;

/// Extension of a path, lowercased, for the per-format breakdown.
fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("?")
        .to_ascii_lowercase()
}

/// Open a file and read its first 64 KB, discarding the result.
///
/// This is the part of a probe that costs whatever the filesystem costs, isolated
/// from demuxing. Measured separately so a slow disk or an antivirus filter can
/// be told apart from slow parsing.
fn read_head(path: &Path) {
    use std::io::Read;
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut buf = vec![0u8; 65_536];
        let _ = file.read(&mut buf);
    }
}

/// Parse the flags the index commands share. Kept separate from the playback
/// argument parser because the two have almost nothing in common.
///
/// `command` decides what the positional arguments mean: `search` takes a query,
/// `classify-diff` takes a reference database path.
pub fn parse(command: &str, folder: PathBuf, rest: &[String]) -> Result<IndexArgs> {
    let mut args = IndexArgs {
        folder,
        full: false,
        threads: 0,
        batch_size: 5_000,
        scan_batch_size: 5_000,
        basic: false,
        limit: 25,
        query: String::new(),
        reference_db: None,
    };
    let mut positional: Vec<String> = Vec::new();
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--full" => args.full = true,
            "--basic" if command == "index" => args.basic = true,
            "--threads" => {
                args.threads = iter
                    .next()
                    .context("--threads needs a value")?
                    .parse()
                    .context("--threads must be a number")?
            }
            "--batch" => {
                args.batch_size = iter
                    .next()
                    .context("--batch needs a value")?
                    .parse()
                    .context("--batch must be a number")?
            }
            "--scan-batch" if command == "index" => {
                args.scan_batch_size = iter
                    .next()
                    .context("--scan-batch needs a value")?
                    .parse()
                    .context("--scan-batch must be a number")?
            }
            "--limit" => {
                args.limit = iter
                    .next()
                    .context("--limit needs a value")?
                    .parse()
                    .context("--limit must be a number")?
            }
            other if other.starts_with("--") => anyhow::bail!("unknown flag {other}"),
            other => positional.push(other.to_string()),
        }
    }
    match command {
        "search" => {
            if let Some(query) = positional.first() {
                args.query = query.clone();
            }
        }
        "classify-diff" => {
            // The reference database is the first positional; the folder is
            // still used as the library root for nothing here, but keeping the
            // same argument shape means one parser for all four commands.
            if let Some(db) = positional.first() {
                args.reference_db = Some(PathBuf::from(db));
            }
        }
        "classify-bench" => {
            args.limit = positional
                .first()
                .map(|n| n.parse())
                .transpose()
                .context("classify-bench limit must be a number")?
                .unwrap_or(0);
        }
        _ => {}
    }
    Ok(args)
}
