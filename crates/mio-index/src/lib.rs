//! Library indexing for Mio Sound Manager.
//!
//! Three jobs, in the order they run:
//!
//! * [`probe`] reads a file's container header only. No decoding, no PCM: the
//!   index needs duration, channel count and sample rate, and all three are in
//!   the header for every supported format except M4A's channel count.
//! * [`classify`] turns a path into a category/subcategory guess. The vocabulary
//!   and the scoring model are this project's own, authored against a real sample
//!   library; see that module's documentation for why the weights run the way
//!   they do.
//! * [`db`] owns the schema (SQLite + FTS5), the upsert that preserves user
//!   edits across reindexes, and the search queries the UI issues.
//!
//! [`indexer::index_folder`] wires them together: walk, probe in parallel, write
//! in batches, then drop rows for files that disappeared.
//!
//! Measured throughput and the comparison against `docs/architecture.md`
//! section 5 live in `docs/agent-log.md`; the design reasoning is in
//! `docs/index.md`.

pub mod bpm;
pub mod classify;
pub mod db;
pub mod error;
pub mod folders;
pub mod indexer;
pub mod probe;
pub mod wav_header;

pub use classify::{Classification, Classifier};
pub use db::{
    CategoryFacet, EditState, IndexDb, MetadataCandidate, MetadataUpdate, SearchParams,
    SortColumn, SortDir, Sound, SoundRow, UcsFacet,
};
pub use error::{IndexError, Result};
pub use folders::{FolderNode, build_folder_tree, folder_range, relative_folder};
pub use indexer::{
    IndexMessageKind, IndexOptions, IndexPhase, IndexProgress, IndexReport, StageTimings,
    index_folder, index_folder_with_events,
};
pub use probe::{
    AudioMeta, EmbeddedMetadata, FileEntry, SkippedDir, WalkReport, is_audio_path,
    metadata_only_with_meta, metadata_only_with_meta_checked, probe_basic_with_meta, probe_file,
    probe_with_meta, walk_audio_entries,
    walk_audio_entries_with_report, walk_audio_files,
};
