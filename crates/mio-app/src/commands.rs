//! The IPC surface: wire types the renderer consumes, and one thin command per
//! thing it can ask for.
//!
//! Wire format is camelCase and flat because it is consumed by TypeScript, and
//! the reference implementation's renderer already speaks that shape. The
//! commands themselves decide nothing — each validates the shape of its arguments
//! and calls a method on [`AppState`]. That split is what lets `tests/ipc.rs`
//! exercise play/stop/waveform/search without a window; a shell whose only test
//! is a human clicking is not tested.

use mio_index::Sound;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::history::EditKind;
use crate::library::{self, Library};
use crate::state::{AppState, LoopState};

/// Everything the window needs at startup, in one round trip.
///
/// The alternative — a command per fact — makes the first paint wait on five IPC
/// calls, and turns "the device failed to open" into a state the renderer has to
/// assemble from partial answers.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub audio_available: bool,
    /// Present when `audio_available` is false. The engine's own message, not a
    /// generic one, so it names the device or format it rejected.
    pub audio_error: Option<String>,
    pub device_name: Option<String>,
    /// `"WASAPI"` or `"ASIO"`. Shown in the status bar, because the whole reason
    /// to pick ASIO is the shorter callback period and that number means nothing
    /// without saying which backend produced it.
    pub backend: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<usize>,
    pub sample_format: Option<String>,
    pub endpoint_frames: Option<u32>,
    /// Engine period in ms. The number that has to be quoted whenever a latency
    /// claim is made — `AGENTS.md`: latency is a measured number, not an adjective.
    pub engine_period_ms: Option<f64>,
    pub library: Option<LibraryStatus>,
}

/// One entry in the settings dialog's output-device picker.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceDto {
    /// `"WASAPI"` or `"ASIO"`. The picker groups by this.
    pub backend: String,
    /// The value to send back in `set_audio_device`.
    pub name: String,
    pub label: String,
    pub channels: Option<usize>,
    /// ASIO only: the driver's preferred callback buffer size.
    pub buffer_frames: Option<u32>,
    /// Whether this is the OS default endpoint. Only WASAPI has one.
    pub default: bool,
    /// False when the device exists but cannot be opened (an ASIO driver whose
    /// interface is unplugged). The picker still lists it, labelled.
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStatus {
    pub root: String,
    /// Folder name only. The sidebar groups by library, and a full path in a
    /// 232px rail is unreadable — this is what the header shows, with the path
    /// in the tooltip.
    pub name: String,
    pub count: u64,
    pub indexing: bool,
    pub indexed: usize,
    pub total: usize,
    /// Whether the window is currently showing this library.
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackPosition {
    pub position_s: f64,
    pub duration_s: f64,
    pub playing: bool,
    /// Loaded but frozen: Space resumes instead of restarting, and the playhead
    /// must stay put while this is true.
    pub paused: bool,
    pub finished: bool,
}

/// One `(category, subcategory, count)` row of the sidebar.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FacetDto {
    pub category: String,
    pub subcategory: Option<String>,
    pub count: u64,
}

/// Everything the sidebar draws, in one round trip.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SidebarDto {
    pub facets: Vec<FacetDto>,
    pub favorites: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WaveformDto {
    pub peaks: Vec<f32>,
    pub duration_s: f64,
    pub sample_rate: u32,
    pub channels: usize,
    pub decode_ms: f64,
}

/// A sound as the table renders it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SoundDto {
    pub path: String,
    pub filename: String,
    pub duration_s: Option<f64>,
    /// Pre-formatted for the table cell, so the renderer does not reimplement
    /// `m:ss.cs` per row (and get the rounding wrong in its own way). Numeric, so
    /// it is not a translation.
    pub duration_label: String,
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
    /// Tempo stated in the file name. `None` when the name states none; the table
    /// renders it as an empty cell, and sorting puts unmarked rows last.
    pub bpm: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderNodeDto {
    pub name: String,
    pub path: String,
    pub count: u64,
    pub children: Vec<FolderNodeDto>,
}

/// Filters and paging as the renderer sends them. Every field optional; the
/// defaults search everything.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchRequest {
    pub query: String,
    pub max_duration: Option<f64>,
    pub channels: Option<u32>,
    /// Several categories are OR-ed. Empty means "any category".
    pub categories: Vec<String>,
    pub subcategories: Vec<String>,
    pub folder: Option<String>,
    pub min_confidence: Option<f64>,
    pub favorite_only: bool,
    pub sort_by: Option<String>,
    pub sort_dir: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

impl SearchRequest {
    /// Validate and translate to the index's own parameter type.
    ///
    /// The sort name is checked against a whitelist here rather than in SQL: an
    /// unknown column is an error, never interpolated.
    pub fn into_params(self) -> AppResult<mio_index::SearchParams> {
        let sort_by = match self.sort_by.as_deref() {
            None | Some("") => None,
            Some(name) => Some(
                library::sort_column(name)
                    .ok_or_else(|| AppError::internal(format!("unknown sort column {name}")))?,
            ),
        };
        Ok(mio_index::SearchParams {
            query: self.query,
            max_duration: self.max_duration,
            channels: self.channels,
            categories: self.categories,
            subcategories: self.subcategories,
            folder: self.folder,
            min_confidence: self.min_confidence,
            favorite_only: self.favorite_only,
            sort_by,
            sort_dir: self.sort_dir.as_deref().map(library::sort_dir),
            limit: self.limit.unwrap_or(200).clamp(1, 5_000),
            offset: self.offset.unwrap_or(0),
        })
    }
}

impl From<Sound> for SoundDto {
    fn from(s: Sound) -> Self {
        Self {
            duration_label: duration_label(s.duration_s),
            path: s.path,
            filename: s.filename,
            duration_s: s.duration_s,
            channels: s.channels,
            sample_rate: s.sample_rate,
            filesize: s.filesize,
            category: s.category,
            subcategory: s.subcategory,
            confidence: s.confidence,
            matched_terms: s.matched_terms,
            favorite: s.favorite,
            category_manual: s.category_manual,
            folder: s.folder,
            bpm: s.bpm,
        }
    }
}

impl From<mio_index::FolderNode> for FolderNodeDto {
    fn from(node: mio_index::FolderNode) -> Self {
        Self {
            name: node.name,
            path: node.path,
            count: node.count,
            children: node.children.into_iter().map(Into::into).collect(),
        }
    }
}

/// `m:ss.cs`, or `—` when the header did not give a usable duration.
pub fn duration_label(duration_s: Option<f64>) -> String {
    let Some(seconds) = duration_s else {
        return "—".to_string();
    };
    if !seconds.is_finite() || seconds < 0.0 {
        return "—".to_string();
    }
    let minutes = (seconds / 60.0).floor() as u64;
    let rest = seconds - minutes as f64 * 60.0;
    let whole = rest.floor() as u64;
    let cents = ((rest - whole as f64) * 100.0).round() as u64;
    // Rounding 59.996 up must not print `0:59.100`: carry into the minute.
    if cents >= 100 {
        let (minutes, whole) = if whole + 1 == 60 {
            (minutes + 1, 0)
        } else {
            (minutes, whole + 1)
        };
        return format!("{minutes}:{whole:02}.00");
    }
    format!("{minutes}:{whole:02}.{cents:02}")
}

/// Current library status, or `NoLibrary` when none is open.
pub fn library_status(library: &Library) -> AppResult<LibraryStatus> {
    let progress = library.progress();
    let root = library.root();
    Ok(LibraryStatus {
        root: root.display().to_string(),
        name: root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string()),
        count: library.count()?,
        indexing: progress.running,
        indexed: progress.done,
        total: progress.total,
        active: false,
    })
}

// ---------------------------------------------------------------- commands

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> AppResult<AppStatus> {
    state.status()
}

/// Every output device, WASAPI endpoints and ASIO drivers alike.
#[tauri::command]
pub fn list_audio_devices(state: State<'_, AppState>) -> Vec<AudioDeviceDto> {
    state.list_audio_devices()
}

/// Whether this binary was built with ASIO support.
#[tauri::command]
pub fn asio_compiled(state: State<'_, AppState>) -> bool {
    state.asio_compiled()
}

/// Switch the output device and remember the choice.
///
/// `backend` is `"wasapi"` or `"asio"`. A device that fails to open is not a
/// command error: the returned status carries the reason, the same way a missing
/// device at startup does, so the dialog can show it next to the picker.
#[tauri::command]
pub fn set_audio_device(
    state: State<'_, AppState>,
    backend: String,
    device: Option<String>,
    asio_buffer_frames: Option<u32>,
) -> AppResult<AppStatus> {
    let backend = mio_engine::Backend::parse(&backend)
        .ok_or_else(|| AppError::internal(format!("unknown audio backend {backend:?}")))?;
    state.set_audio_device(backend, device, asio_buffer_frames)
}

/// Start playing `path`, optionally `startAt` seconds in.
#[tauri::command]
pub async fn play(state: State<'_, AppState>, path: String, start_at: Option<f64>) -> AppResult<()> {
    state.play(&path, start_at).await
}

#[tauri::command]
pub fn stop(state: State<'_, AppState>) -> AppResult<()> {
    state.stop_playback()
}

/// Freeze the loaded clip where it is. Space in the table toggles this.
#[tauri::command]
pub fn pause(state: State<'_, AppState>) -> AppResult<()> {
    state.pause_playback()
}

#[tauri::command]
pub fn resume(state: State<'_, AppState>) -> AppResult<()> {
    state.resume_playback()
}

/// Set the output gain. `1.0` is unity, `0.0` silence, and the engine clamps.
///
/// Separate from `play` on purpose: the volume slider must be able to move while
/// a clip is playing without re-publishing it, so this only writes the atomic the
/// render callback reads.
#[tauri::command]
pub fn set_gain(state: State<'_, AppState>, gain: f32) -> AppResult<f32> {
    state.set_gain(gain)
}

/// The gain the engine is currently applying. Read at startup so a slider that
/// was moved last session comes back where the user left it.
#[tauri::command]
pub fn get_gain(state: State<'_, AppState>) -> f32 {
    state.gain()
}

/// Loop the loaded clip, optionally within `[startS, endS)`.
///
/// Separate from `play` for the same reason `set_gain` is: turning a loop on while
/// the clip is already loaded must not re-publish it, or every flip of the switch
/// would click. `region` is `None` for the whole clip, which is what the switch
/// means when nothing is selected on the waveform.
#[tauri::command]
pub fn set_loop(
    state: State<'_, AppState>,
    enabled: bool,
    start_s: Option<f64>,
    end_s: Option<f64>,
) -> AppResult<LoopState> {
    let region = match (start_s, end_s) {
        (Some(from), Some(to)) if to > from => Some((from.max(0.0), to)),
        _ => None,
    };
    state.set_loop(enabled, region)
}

/// The loop switch and its region, as the engine holds them. The dock's toggle
/// mirrors this rather than its own state, so a failed set is visible.
#[tauri::command]
pub fn get_loop(state: State<'_, AppState>) -> LoopState {
    state.loop_state()
}

/// Render a region of a file to a WAV and return its path.
///
/// The waveform dock calls this before starting a drag of a selection: a DAW takes
/// a file, not a frame range. The WAV lands in the app's cache directory, which is
/// where a scratch render belongs — never in the library, because a file the user
/// did not ask to keep must not appear in their own index.
#[tauri::command]
pub async fn export_region(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
    start_s: f64,
    end_s: f64,
) -> AppResult<String> {
    use tauri::Manager;

    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::internal(format!("no cache directory: {e}")))?
        .join("exports");
    state.export_region(&path, start_s, end_s, &dir).await
}

/// The playhead. Called from `requestAnimationFrame`, so it stays allocation free
/// and lock free — which is why it does not touch the decode worker or the library.
#[tauri::command]
pub fn get_playback_position(state: State<'_, AppState>) -> PlaybackPosition {
    state.playback_position()
}

/// Peak envelope for the waveform panel.
#[tauri::command]
pub async fn get_waveform(
    state: State<'_, AppState>,
    path: String,
    buckets: Option<usize>,
) -> AppResult<WaveformDto> {
    state.waveform(&path, buckets).await
}

/// One progress tick from a blocking index pass.
///
/// Separate from `library::IndexProgress`, which also carries `running` and is
/// read by polling. This is the shape that goes over a `Channel` while the pass is
/// happening, so it only needs the two numbers the bar draws.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress {
    pub done: usize,
    pub total: usize,
}

/// Add a library folder and index it before returning.
///
/// **Blocking, by design.** The pass runs on a blocking worker and reports
/// progress through `onProgress`, which the window uses to draw a modal bar. The
/// window stays unusable until this resolves — the user asked for that, because a
/// half-indexed library shows a list where every row might be missing siblings.
///
/// `onProgress` is a required `Channel` of `{done, total}`: Tauri 2 has no
/// `Option<Channel>` (the type is not `Deserialize`), so the caller always passes
/// one. A script that does not care about progress sends an ignored channel.
#[tauri::command]
pub async fn add_library(
    app: tauri::AppHandle,
    root: String,
    full: Option<bool>,
    on_progress: tauri::ipc::Channel<IndexProgress>,
) -> AppResult<LibraryStatus> {
    use tauri::Manager;

    let full = full.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        // `State` borrows the command's stack frame and cannot cross into a
        // blocking task, so the handle is used to reach the managed state instead.
        let state = app.state::<AppState>();
        state.add_library(&root, full, |done, total| {
            // A dropped receiver means the window closed; the pass still has to
            // finish so the database is consistent.
            let _ = on_progress.send(IndexProgress { done, total });
        })
    })
    .await
    .map_err(|e| AppError::internal(format!("index task failed: {e}")))?
}

/// Every open library, for the sidebar's groups.
#[tauri::command]
pub fn list_libraries(state: State<'_, AppState>) -> AppResult<Vec<LibraryStatus>> {
    state.library_list()
}

/// Close a library. The remaining one becomes active if this was the active one.
#[tauri::command]
pub fn remove_library(state: State<'_, AppState>, root: String) -> AppResult<Vec<LibraryStatus>> {
    state.remove_library(&root)?;
    state.library_list()
}

/// Show a different open library.
#[tauri::command]
pub fn set_active_library(
    state: State<'_, AppState>,
    root: String,
) -> AppResult<Vec<LibraryStatus>> {
    state.set_active_library(&root)?;
    state.library_list()
}

/// Reindex one library, blocking, with the same progress channel as opening.
///
/// `root` names the library; `None` means the active one. Naming it explicitly is
/// what lets the sidebar reindex a background library without switching to it.
#[tauri::command]
pub async fn reindex_library(
    app: tauri::AppHandle,
    root: Option<String>,
    full: Option<bool>,
    on_progress: tauri::ipc::Channel<IndexProgress>,
) -> AppResult<LibraryStatus> {
    use tauri::Manager;

    let full = full.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.reindex_library(root.as_deref(), full, |done, total| {
            let _ = on_progress.send(IndexProgress { done, total });
        })
    })
    .await
    .map_err(|e| AppError::internal(format!("index task failed: {e}")))?
}

/// Undo the last library edit. Returns what kind it was, or `None` when there is
/// nothing to undo.
#[tauri::command]
pub fn undo(state: State<'_, AppState>) -> AppResult<Option<EditKind>> {
    state.undo()
}

#[tauri::command]
pub fn redo(state: State<'_, AppState>) -> AppResult<Option<EditKind>> {
    state.redo()
}

/// What undo and redo would do. The kinds are data; the renderer names them.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryState {
    pub undo_kind: Option<EditKind>,
    pub redo_kind: Option<EditKind>,
}

#[tauri::command]
pub fn get_history(state: State<'_, AppState>) -> AppResult<HistoryState> {
    let (undo_kind, redo_kind) = state.history_state();
    Ok(HistoryState { undo_kind, redo_kind })
}

#[tauri::command]
pub fn get_library(state: State<'_, AppState>) -> AppResult<Option<LibraryStatus>> {
    state.library_status()
}

#[tauri::command]
pub fn search(state: State<'_, AppState>, request: SearchRequest) -> AppResult<Vec<SoundDto>> {
    let library = state.library()?;
    let params = request.into_params()?;
    Ok(library.search(&params)?.into_iter().map(SoundDto::from).collect())
}

#[tauri::command]
pub fn list_folders(state: State<'_, AppState>) -> AppResult<FolderNodeDto> {
    let library = state.library()?;
    Ok(library.folder_tree()?.into())
}

/// Every path the current filter matches, for "select all".
///
/// Separate from `search` on purpose: `search` returns full rows and clamps at
/// 5,000, because a table page has no reason to be bigger. Selecting everything
/// is a different question — the answer for a filtered subset of a 280k library
/// is 280k paths — and materialising 280k row DTOs to keep a set of paths would
/// be tens of megabytes of structs the caller throws away.
#[tauri::command]
pub fn list_matching_paths(
    state: State<'_, AppState>,
    request: SearchRequest,
) -> AppResult<Vec<String>> {
    state.matching_paths(request)
}

/// How many rows the current filter matches. The status bar shows it next to the
/// page count so "38 of 27,525" is never a guess.
#[tauri::command]
pub fn count_matching(state: State<'_, AppState>, request: SearchRequest) -> AppResult<u64> {
    state.count_matching(request)
}

/// Facet counts, favorites and total for the sidebar, in one round trip.
#[tauri::command]
pub fn get_sidebar(state: State<'_, AppState>) -> AppResult<SidebarDto> {
    state.sidebar()
}

/// Star or unstar many sounds. One transaction, so a 200-row selection is one
/// commit rather than 200.
#[tauri::command]
pub fn set_favorite(
    state: State<'_, AppState>,
    paths: Vec<String>,
    favorite: bool,
) -> AppResult<u64> {
    state.set_favorite(&paths, favorite)
}

/// Assign a category by hand to many sounds. Marks them `category_manual`, so a
/// later index pass keeps the choice.
#[tauri::command]
pub fn set_category(
    state: State<'_, AppState>,
    paths: Vec<String>,
    category: String,
    subcategory: Option<String>,
) -> AppResult<u64> {
    state.set_category(&paths, &category, subcategory.as_deref())
}

/// Drop the manual category on many sounds, so the next pass reclassifies them.
#[tauri::command]
pub fn clear_category(state: State<'_, AppState>, paths: Vec<String>) -> AppResult<u64> {
    state.clear_category(&paths)
}

/// Native folder picker. Returns `None` when the user cancels.
///
/// The blocking variant is used on purpose: this command is `async`, so Tauri
/// runs it on the async runtime rather than the main thread, and blocking there
/// does not freeze the window. The non-blocking variant would need a channel back
/// into the command, which is more machinery for the same result.
#[tauri::command]
pub async fn pick_library_folder(app: tauri::AppHandle) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;

    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Open a sample library")
            .blocking_pick_folder()
    })
    .await
    .map_err(|e| AppError::internal(format!("folder picker failed: {e}")))?;

    Ok(picked.and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

/// Show a sound in Windows Explorer with the file selected.
///
/// Uses the opener plugin's reveal rather than spawning `explorer /select`: it
/// goes through `SHOpenFolderAndSelectItems`, which reuses an open Explorer
/// window instead of opening a second one on the same folder.
#[tauri::command]
pub fn reveal_in_folder(app: tauri::AppHandle, path: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;

    if !std::path::Path::new(&path).exists() {
        return Err(AppError::internal(format!("file is gone: {path}")));
    }
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| AppError::internal(format!("reveal failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_labels_read_as_minutes_and_hundredths() {
        assert_eq!(duration_label(Some(0.0)), "0:00.00");
        assert_eq!(duration_label(Some(3.0)), "0:03.00");
        assert_eq!(duration_label(Some(3.456)), "0:03.46");
        assert_eq!(duration_label(Some(61.2)), "1:01.20");
        assert_eq!(duration_label(None), "—");
        assert_eq!(duration_label(Some(f64::NAN)), "—");
    }

    #[test]
    fn duration_label_carries_into_the_minute() {
        // 59.999 rounds to 60 hundredths inside the seconds field; the label must
        // carry rather than print `0:59.100`.
        assert_eq!(duration_label(Some(59.999)), "1:00.00");
        assert_eq!(duration_label(Some(119.999)), "2:00.00");
    }

    #[test]
    fn an_unknown_sort_column_is_an_error_not_a_sql_injection() {
        let request = SearchRequest {
            sort_by: Some("; DROP TABLE sounds".to_string()),
            ..Default::default()
        };
        assert!(request.into_params().is_err());
    }

    #[test]
    fn sort_names_map_to_the_whitelist() {
        let request = SearchRequest {
            sort_by: Some("duration".to_string()),
            sort_dir: Some("desc".to_string()),
            limit: Some(1_000_000),
            ..Default::default()
        };
        let params = request.into_params().expect("duration is a valid column");
        assert_eq!(params.sort_by, Some(mio_index::SortColumn::Duration));
        assert_eq!(params.sort_dir, Some(mio_index::SortDir::Desc));
        assert_eq!(params.limit, 5_000, "limit is clamped");
    }

    #[test]
    fn default_paging_is_bounded() {
        let params = SearchRequest::default().into_params().unwrap();
        assert_eq!(params.limit, 200);
        assert_eq!(params.offset, 0);
    }

    #[test]
    fn zero_limit_is_clamped_up_rather_than_meaning_everything() {
        let params = SearchRequest {
            limit: Some(0),
            ..Default::default()
        }
        .into_params()
        .unwrap();
        assert_eq!(params.limit, 1);
    }
}
