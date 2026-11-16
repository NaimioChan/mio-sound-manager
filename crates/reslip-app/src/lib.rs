//! ReSlip's Tauri shell: the window, and the IPC surface the renderer calls.
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
//! them contain audio code: that lives in `reslip-engine` and is unchanged by
//! anything the UI does.

mod audio_config;
mod commands;
mod error;
mod history;
mod library;
mod session;
mod state;

pub use commands::{
    AppStatus, AudioDeviceDto, FacetDto, FolderNodeDto, HistoryState, IndexProgress,
    LibraryStatus, PlaybackPosition, SearchRequest, SidebarDto, SoundDto, WaveformDto,
};
pub use error::AppError;
pub use library::Library;
pub use state::AppState;

use tauri::Manager;

/// Build and run the desktop window.
pub fn run() {
    tauri::Builder::default()
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
            commands::get_playback_position,
            commands::get_waveform,
            commands::add_library,
            commands::list_libraries,
            commands::remove_library,
            commands::set_active_library,
            commands::reindex_library,
            commands::search,
            commands::get_library,
            commands::list_folders,
            commands::list_matching_paths,
            commands::count_matching,
            commands::get_sidebar,
            commands::set_favorite,
            commands::set_category,
            commands::clear_category,
            commands::undo,
            commands::redo,
            commands::get_history,
            commands::pick_library_folder,
            commands::reveal_in_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ReSlip");
}
