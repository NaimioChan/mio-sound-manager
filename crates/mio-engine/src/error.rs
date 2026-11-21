use std::fmt;

/// Everything the engine can fail at. Decode failures carry the path because
/// they surface in the UI as "this file will not play" rather than as a crash.
#[derive(Debug)]
pub enum EngineError {
    Audio(String),
    Decode { path: String, detail: String },
    DecodeTimeout { path: String, timeout_ms: u64 },
    Unsupported(String),
    DeviceNotFound { filter: String, available: String },
    Io(std::io::Error),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Audio(m) => write!(f, "audio backend: {m}"),
            EngineError::Decode { path, detail } => write!(f, "cannot decode {path}: {detail}"),
            EngineError::DecodeTimeout { path, timeout_ms } => {
                write!(f, "decoding {path} exceeded the {timeout_ms}ms time limit")
            }
            EngineError::Unsupported(what) => write!(f, "unsupported: {what}"),
            EngineError::DeviceNotFound { filter, available } => {
                write!(
                    f,
                    "no render device matching {filter:?}; available: {available}"
                )
            }
            EngineError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        EngineError::Io(e)
    }
}

impl From<wasapi::WasapiError> for EngineError {
    fn from(e: wasapi::WasapiError) -> Self {
        EngineError::Audio(e.to_string())
    }
}

impl From<symphonia::core::errors::Error> for EngineError {
    fn from(e: symphonia::core::errors::Error) -> Self {
        EngineError::Unsupported(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;
