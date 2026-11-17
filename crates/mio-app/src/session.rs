//! Remembering which libraries the user has opened.
//!
//! A sample manager is opened on the same folder every day; making the user pick
//! it again on every launch is the single most annoying thing a browser like this
//! can do. So the set of open libraries and which one was active is written to a
//! small JSON file next to the app's own config, and read back at startup.
//!
//! Deliberately *not* stored in the library folder itself: the index lives there
//! and belongs to the folder, while "which folders are open" is a property of the
//! user's session. Copying a library folder to another machine should not carry
//! the fact that it was open on this one.
//!
//! Failure to read or write this file is never fatal. A missing config means
//! "no libraries open yet"; a corrupt one is logged and replaced. Losing the
//! recent list costs the user one folder pick, which is much better than an app
//! that will not start.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What is persisted between runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Session {
    /// Libraries the user has open, in the order they were added. Absolute paths.
    #[serde(default)]
    pub libraries: Vec<PathBuf>,
    /// The library whose rows the window was showing. `None` means the first one.
    #[serde(default)]
    pub active: Option<PathBuf>,
}

impl Session {
    /// Drop entries whose folder no longer exists.
    ///
    /// A library on a removable drive is the normal case for a sample manager, so
    /// the folder can legitimately be missing at startup. It stays in the config
    /// (the drive may come back) but must not be opened, or every launch would
    /// spend time on an error the user already knows about.
    pub fn present(&self) -> Vec<PathBuf> {
        self.libraries
            .iter()
            .filter(|p| p.is_dir())
            .cloned()
            .collect()
    }

    /// The library to show first: the remembered one if it is still open and
    /// still on disk, otherwise the first that exists.
    pub fn first_open(&self) -> Option<PathBuf> {
        let present = self.present();
        match &self.active {
            Some(active) if present.contains(active) => Some(active.clone()),
            _ => present.into_iter().next(),
        }
    }
}

/// The on-disk session file.
pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    /// `dir` is the app's config directory (`%APPDATA%\com.naimio.miosound` on
    /// Windows). Tauri supplies it; tests pass a scratch dir.
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            path: dir.as_ref().join("session.json"),
        }
    }

    /// Only the tests need the file path; the app goes through `load`/`save`.
    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read the session, or a default one if there is nothing usable.
    pub fn load(&self) -> Session {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Session::default(),
            Err(e) => {
                log::warn!("session file unreadable ({e}); starting with no libraries");
                return Session::default();
            }
        };
        match serde_json::from_str(&raw) {
            Ok(session) => session,
            Err(e) => {
                // A corrupt file is replaced on the next save rather than crashing
                // the app: the only thing lost is the list of open folders.
                log::warn!("session file is not valid JSON ({e}); starting with no libraries");
                Session::default()
            }
        }
    }

    /// Write the session, creating the directory if needed.
    ///
    /// Written to a temporary file and renamed into place so a crash mid-write
    /// cannot leave a half-written file that the next launch refuses to parse.
    pub fn save(&self, session: &Session) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(session)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, SessionStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SessionStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn a_missing_session_file_is_an_empty_session_not_an_error() {
        let (_dir, store) = store();
        assert_eq!(store.load(), Session::default());
    }

    #[test]
    fn a_session_round_trips() {
        let (_dir, store) = store();
        let session = Session {
            libraries: vec![PathBuf::from(r"C:\libs\A"), PathBuf::from(r"C:\libs\B")],
            active: Some(PathBuf::from(r"C:\libs\B")),
        };
        store.save(&session).expect("save");
        assert_eq!(store.load(), session);
    }

    #[test]
    fn a_corrupt_session_file_falls_back_instead_of_panicking() {
        let (_dir, store) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), b"{ this is not json").unwrap();
        assert_eq!(store.load(), Session::default());
    }

    #[test]
    fn folders_that_are_gone_are_skipped_but_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("present");
        std::fs::create_dir_all(&present).unwrap();
        let gone = dir.path().join("on-a-removable-drive");

        let session = Session {
            libraries: vec![gone.clone(), present.clone()],
            active: Some(gone.clone()),
        };

        // The missing folder is not opened...
        assert_eq!(session.present(), vec![present.clone()]);
        // ...and because it was the active one, the first existing library wins.
        assert_eq!(session.first_open(), Some(present));
        // ...but it is still in the list, because the drive may come back.
        assert_eq!(session.libraries.len(), 2);
    }

    #[test]
    fn the_remembered_library_wins_when_it_still_exists() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();

        let session = Session {
            libraries: vec![a, b.clone()],
            active: Some(b.clone()),
        };
        assert_eq!(session.first_open(), Some(b));
    }
}
