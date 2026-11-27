//! Mio Sound Manager's Tauri shell: the window, and the IPC surface the renderer calls.
//!
//! Everything here is boundary code. The rules it exists to enforce come from
//! `docs/architecture.md` section 7:
//!
//! * **The renderer never touches PCM.** Waveform data crosses the IPC boundary
//!   as ~1000 floats; decoded audio stays in Rust and is published straight to
//!   the audio thread.
//! * **A command never blocks the UI thread.** Decoding happens on a worker; the
//!   Tauri command awaits it and the webview keeps painting.
//! * **A command never steals latency from playback.** The playhead command is a
//!   handful of atomic loads, not a request to the control thread.
//!
//! The playback side is deliberately thin. `state.rs` owns the processes,
//! `library.rs` owns the index, `commands.rs` is the wire format, and none of
//! them contain audio code: that lives in `mio-engine` and is unchanged by
//! anything the UI does.

mod audio_config;
mod commands;
mod error;
mod history;
mod library;
mod session;
mod state;

pub use commands::{
    AppStatus, AudioDeviceDto, FacetDto, FolderNodeDto, HistoryState, IndexProgress, LibraryStatus,
    MatchingEditRequest, PlaybackErrorDto, PlaybackPosition, SearchRequest, SidebarDto, SoundDto,
    UcsFacetDto, WaveformDto,
};
pub use error::AppError;
pub use history::EditKind;
pub use library::Library;
pub use state::{AppState, LoopState};

use tauri::Manager;

/// Build and run the desktop window.
pub fn run() {
    tauri::Builder::default()
        // A file logger, because release builds are `windows_subsystem = "windows"`:
        // with no `set_logger` every `log::warn!` in this crate and the engine went
        // straight to /dev/null, so "this driver would not open", "audio.json is
        // corrupt" and "the index pass failed" left no trace for the user or the
        // next person debugging. Writes below the app config dir, at WARN and up by
        // default, rotating at 2 MB with one old file kept.
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(2 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("mio".into()),
                    },
                ))
                .build(),
        )
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // The engine is opened once, at startup, and the stream stays live
            // for the life of the process (architecture section 4). If the device
            // is missing, the app still starts — playback commands then report
            // the device error instead of taking the window down with it.
            let mut state = AppState::boot();

            // Reopen whatever libraries were open last time. The databases are
            // opened here (schema migration only); the UI asks for an index pass
            // and shows its bar, so startup never blocks for a minute.
            match app.path().app_config_dir() {
                Ok(dir) => state.attach_session(dir),
                Err(e) => log::warn!("no config directory, session will not persist: {e}"),
            }

            match app.path().app_cache_dir() {
                Ok(cache) => {
                    let exports = cache.join("exports");
                    if let Err(e) = cleanup_exports(&exports) {
                        log::warn!("could not clean export cache {}: {e}", exports.display());
                    }
                }
                Err(e) => log::warn!("no cache directory, export cleanup skipped: {e}"),
            }

            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::list_audio_devices,
            commands::asio_compiled,
            commands::set_audio_device,
            commands::play,
            commands::stop,
            commands::pause,
            commands::resume,
            commands::set_gain,
            commands::get_gain,
            commands::set_loop,
            commands::get_loop,
            commands::export_region,
            commands::get_playback_position,
            commands::take_playback_error,
            commands::get_waveform,
            commands::prefetch_neighbors,
            commands::add_library,
            commands::list_libraries,
            commands::remove_library,
            commands::set_active_library,
            commands::reindex_library,
            commands::search,
            commands::get_library,
            commands::list_folders,
            commands::count_matching,
            commands::get_sidebar,
            commands::set_favorite,
            commands::set_category,
            commands::list_ucs_categories,
            commands::set_ucs_category,
            commands::set_ucs_category_matching,
            commands::clear_category,
            commands::set_favorite_matching,
            commands::set_category_matching,
            commands::clear_category_matching,
            commands::undo,
            commands::redo,
            commands::get_history,
            commands::pick_library_folder,
            commands::reveal_in_folder,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Mio Sound Manager")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                app.state::<AppState>().close_libraries();
            }
        });
}

fn cleanup_exports(dir: &std::path::Path) -> std::io::Result<()> {
    cleanup_exports_with_limits(
        dir,
        512 * 1024 * 1024,
        std::time::Duration::from_secs(7 * 24 * 60 * 60),
    )
}

fn cleanup_exports_with_limits(
    dir: &std::path::Path,
    max_bytes: u64,
    max_age: std::time::Duration,
) -> std::io::Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    let now = std::time::SystemTime::now();
    let mut files = Vec::new();
    let mut total = 0u64;
    for entry in entries {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
        if now.duration_since(modified).unwrap_or_default() > max_age {
            std::fs::remove_file(entry.path())?;
            continue;
        }
        total = total.saturating_add(metadata.len());
        files.push((modified, metadata.len(), entry.path()));
    }
    if total > max_bytes {
        files.sort_by_key(|(modified, _, _)| *modified);
        for (_, size, path) in files {
            if total <= max_bytes {
                break;
            }
            std::fs::remove_file(path)?;
            total = total.saturating_sub(size);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn export_cleanup_evicts_oldest_files_until_under_limit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.wav"), vec![0; 8]).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.path().join("b.wav"), vec![0; 8]).unwrap();

        cleanup_exports_with_limits(dir.path(), 8, Duration::from_secs(60)).unwrap();
        assert!(!dir.path().join("a.wav").exists());
        assert!(dir.path().join("b.wav").exists());
    }
}
