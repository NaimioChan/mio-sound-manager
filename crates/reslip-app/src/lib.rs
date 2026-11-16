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

mod commands;
mod error;
mod library;
mod state;

pub use commands::{
    AppStatus, FacetDto, FolderNodeDto, LibraryStatus, PlaybackPosition, SearchRequest, SidebarDto,
    SoundDto, WaveformDto,
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
            let state = AppState::boot();
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::play,
            commands::stop,
            commands::pause,
            commands::resume,
            commands::get_playback_position,
            commands::get_waveform,
            commands::open_library,
            commands::search,
            commands::get_library,
            commands::list_folders,
            commands::list_matching_paths,
            commands::count_matching,
            commands::get_sidebar,
            commands::set_favorite,
            commands::set_category,
            commands::clear_category,
            commands::pick_library_folder,
            commands::reveal_in_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ReSlip");
}
