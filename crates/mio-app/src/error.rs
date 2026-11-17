//! One error type for the IPC boundary.
//!
//! Commands return `Result<T, String>` to the webview because that is the only
//! thing serde can hand a JS promise rejection. Everything internal keeps its own
//! error type (`EngineError`, `IndexError`) and is converted once, here, so the
//! message a user sees is written where the cause is known rather than being
//! flattened into "something went wrong".

use std::fmt;

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The engine could not open a render device at startup. Playback is dead for
    /// the life of this process; the window still works.
    #[error("audio device unavailable: {0}")]
    NoAudioDevice(String),

    #[error("{0}")]
    Engine(#[from] mio_engine::EngineError),

    #[error("{0}")]
    Index(#[from] mio_index::IndexError),

    /// No library folder has been opened yet.
    #[error("no library is open")]
    NoLibrary,

    /// The file is not one this app plays.
    #[error("not an audio file: {0}")]
    NotAudio(String),

    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn internal(message: impl fmt::Display) -> Self {
        Self::Internal(message.to_string())
    }
}

/// Tauri turns a failed command into a rejected promise carrying whatever the
/// error serialises to. Serialising as the bare message keeps JS-side handling
/// simple (`catch (e) => e`) and keeps stack-trace noise out of the UI.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = std::result::Result<T, AppError>;
