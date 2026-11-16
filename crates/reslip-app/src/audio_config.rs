//! Remembering which output device the user chose.
//!
//! A sample manager is opened on the same interface every day, and picking the
//! device again on every launch is the second most annoying thing this app could
//! do (the first being the library). So the choice is written to a small JSON file
//! next to the session and read back at startup.
//!
//! Kept separate from [`crate::session`] because the two have different failure
//! modes: a session file describes which folders are open and is worth preserving
//! through a corrupt write, while a device choice is one line and re-picking it is
//! a two-click fix. Same store shape, same "never fatal" rule.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use reslip_engine::Backend;

/// The device the user last played through.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AudioConfig {
    /// `"wasapi"` or `"asio"`. Stored as text so a config written by an older or
    /// newer build still parses, and so a build without the `asio` feature can
    /// still read a config that names ASIO (and fall back to WASAPI).
    #[serde(default)]
    pub backend: Option<String>,
    /// The device name, matched as a case-insensitive substring by the engine.
    /// `None` means "whatever the system default is".
    #[serde(default)]
    pub device: Option<String>,
    /// ASIO callback buffer in frames. `None` lets the driver pick its preferred
    /// size, which is the only universally safe request.
    #[serde(default)]
    pub asio_buffer_frames: Option<u32>,
}

impl AudioConfig {
    pub fn backend(&self) -> Backend {
        self.backend
            .as_deref()
            .and_then(Backend::parse)
            .unwrap_or(Backend::Wasapi)
    }
}

/// The on-disk device choice.
pub struct AudioStore {
    path: PathBuf,
}

impl AudioStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            path: dir.as_ref().join("audio.json"),
        }
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read the config, or a default one if there is nothing usable.
    ///
    /// A missing or corrupt file means "system default", which is a working
    /// configuration on every machine.
    pub fn load(&self) -> AudioConfig {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return AudioConfig::default(),
            Err(e) => {
                log::warn!("audio config unreadable ({e}); using the system default device");
                return AudioConfig::default();
            }
        };
        match serde_json::from_str(&raw) {
            Ok(config) => config,
            Err(e) => {
                log::warn!("audio config is not valid JSON ({e}); using the system default device");
                AudioConfig::default()
            }
        }
    }

    /// Write the config, creating the directory if needed.
    pub fn save(&self, config: &AudioConfig) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, AudioStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = AudioStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn a_missing_config_means_the_system_default() {
        let (_dir, store) = store();
        let config = store.load();
        assert_eq!(config.backend(), Backend::Wasapi);
        assert_eq!(config.device, None);
    }

    #[test]
    fn an_asio_choice_round_trips() {
        let (_dir, store) = store();
        let config = AudioConfig {
            backend: Some("asio".into()),
            device: Some("TOPPING Pro USB Audio Device".into()),
            asio_buffer_frames: Some(128),
        };
        store.save(&config).expect("save");
        let loaded = store.load();
        assert_eq!(loaded, config);
        assert_eq!(loaded.backend(), Backend::Asio);
    }

    #[test]
    fn a_corrupt_config_falls_back_to_the_default() {
        let (_dir, store) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), b"not json").unwrap();
        assert_eq!(store.load(), AudioConfig::default());
    }

    #[test]
    fn an_unknown_backend_name_reads_as_wasapi() {
        // A config written by a build that knew a backend this one does not must
        // still start the app; falling back beats refusing to open a window.
        let config = AudioConfig {
            backend: Some("coreaudio".into()),
            device: Some("x".into()),
            asio_buffer_frames: None,
        };
        assert_eq!(config.backend(), Backend::Wasapi);
    }
}
