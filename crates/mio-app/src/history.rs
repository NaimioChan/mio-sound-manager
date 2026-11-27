//! Undo for library edits.
//!
//! Scope is deliberately narrow: the two things a user can change about a sound
//! (favorite flag, manual category) plus the "clear category" inverse. Those are
//! the edits that are easy to trigger by accident — one stray click on a star, or
//! a whole selection categorised by mistake — and the only ones a sample manager
//! needs to take back. Renaming files, deleting, and reindexing are not undoable
//! here and should not pretend to be.
//!
//! Each entry stores the *previous value* of every field it touched, per path. That
//! is enough to invert an edit without replaying the whole database, and it keeps
//! the memory cost proportional to the selection rather than to the library.
//!
//! Not persisted. Undo across a restart is not a promise a tool like this should
//! make: the files may have been reindexed, and a stale inverse could write back a
//! category the user no longer wants.

use std::collections::VecDeque;
use std::path::PathBuf;

use mio_index::Sound;
use serde::Serialize;

use crate::error::AppResult;
use crate::library::Library;

/// How many edits can be taken back. Deep enough for a working session, shallow
/// enough that a 280k-row "select all, categorise" undo stack stays bounded.
const MAX_DEPTH: usize = 50;
const MAX_BYTES: usize = 128 * 1024 * 1024;

/// What an edit was, so the UI can name it in the user's language.
///
/// This used to be a `String` label ("Add to Favorites") produced here and shown
/// verbatim by the renderer. That cannot survive localisation: the backend has no
/// idea which language the window is in, and sending English prose across IPC to be
/// displayed means the undo tooltip is the one string that never translates. The
/// kind is data; the words belong to `ui/src/locales/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EditKind {
    /// Starred one or more sounds.
    Favorite,
    /// Unstarred them.
    Unfavorite,
    /// Assigned a manual category.
    SetCategory,
    /// Dropped a manual category.
    ClearCategory,
    SetUcsCategory,
}

/// One field of one sound, as it was before the edit.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldState {
    pub path: String,
    pub favorite: bool,
    pub category: String,
    pub subcategory: Option<String>,
    pub category_manual: bool,
    pub ucs_category: Option<String>,
    pub ucs_subcategory: Option<String>,
    pub ucs_cat_id: Option<String>,
    pub ucs_manual: bool,
}

impl FieldState {
    fn from_sound(sound: &Sound) -> Self {
        Self {
            path: sound.path.clone(),
            favorite: sound.favorite,
            category: sound.category.clone(),
            subcategory: sound.subcategory.clone(),
            category_manual: sound.category_manual,
            ucs_category: sound.ucs_category.clone(),
            ucs_subcategory: sound.ucs_subcategory.clone(),
            ucs_cat_id: sound.ucs_cat_id.clone(),
            ucs_manual: sound.ucs_manual,
        }
    }

    fn from_edit_state(state: mio_index::EditState) -> Self {
        Self {
            path: state.path,
            favorite: state.favorite,
            category: state.category,
            subcategory: state.subcategory,
            category_manual: state.category_manual,
            ucs_category: state.ucs_category,
            ucs_subcategory: state.ucs_subcategory,
            ucs_cat_id: state.ucs_cat_id,
            ucs_manual: state.ucs_manual,
        }
    }
}

/// A completed edit, stored in the form needed to invert it.
#[derive(Debug, Clone)]
pub struct Edit {
    /// What the user did. The UI turns this into words; see `EditKind`.
    pub kind: EditKind,
    /// The library the edit belongs to. Undo must go back to the same database
    /// even if the user has since switched to another library.
    pub root: PathBuf,
    pub before: Vec<FieldState>,
}

impl Edit {
    fn estimated_bytes(&self) -> usize {
        self.before.iter().fold(
            self.root.as_os_str().len() + self.before.len() * std::mem::size_of::<FieldState>(),
            |bytes, state| {
                bytes
                    + state.path.capacity()
                    + state.category.capacity()
                    + state.subcategory.as_ref().map_or(0, String::capacity)
            },
        )
    }
}

/// Undo and redo stacks, as one unit.
pub struct History {
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    bytes: usize,
}

impl History {
    pub fn new() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            bytes: 0,
        }
    }

    /// Record a completed edit. Any pending redo is dropped: the timeline forked.
    pub fn push(&mut self, edit: Edit) {
        self.bytes = self
            .bytes
            .saturating_sub(self.redo.iter().map(Edit::estimated_bytes).sum::<usize>());
        self.redo.clear();
        self.bytes += edit.estimated_bytes();
        self.undo.push_back(edit);
        while self.undo.len() > MAX_DEPTH || (self.bytes > MAX_BYTES && self.undo.len() > 1) {
            if let Some(oldest) = self.undo.pop_front() {
                self.bytes = self.bytes.saturating_sub(oldest.estimated_bytes());
            }
        }
    }

    /// Only the unit tests ask this; the app decides from the labels.
    #[cfg(test)]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_kind(&self) -> Option<EditKind> {
        self.undo.back().map(|e| e.kind)
    }

    pub fn redo_kind(&self) -> Option<EditKind> {
        self.redo.last().map(|e| e.kind)
    }

    fn take_undo(&mut self) -> Option<Edit> {
        let edit = self.undo.pop_back()?;
        self.bytes = self.bytes.saturating_sub(edit.estimated_bytes());
        Some(edit)
    }

    fn take_redo(&mut self) -> Option<Edit> {
        let edit = self.redo.pop()?;
        self.bytes = self.bytes.saturating_sub(edit.estimated_bytes());
        Some(edit)
    }

    fn push_redo(&mut self, edit: Edit) {
        self.bytes += edit.estimated_bytes();
        self.redo.push(edit);
    }

    fn push_undo_raw(&mut self, edit: Edit) {
        self.bytes += edit.estimated_bytes();
        self.undo.push_back(edit);
    }
}

/// Look up the current state of every path an edit touched.
///
/// Paths that are no longer in the index are skipped: a file deleted since the
/// edit has nothing to restore, and inventing a row for it would be worse than
/// losing that one entry.
pub fn snapshot(library: &Library, paths: &[String]) -> AppResult<Vec<FieldState>> {
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        if let Some(sound) = library.get(path)? {
            out.push(FieldState::from_sound(&sound));
        }
    }
    Ok(out)
}

/// Write `states` back exactly as recorded.
///
/// Done one field at a time through the index's own setters, so the FTS triggers
/// fire the same way a user edit would — an undo that bypassed them would leave
/// search disagreeing with the table.
pub fn restore(library: &Library, states: &[FieldState]) -> AppResult<u64> {
    let mut touched = 0u64;
    // Group by (favorite, category, subcategory, manual) so a 28k-row undo is a
    // handful of transactions rather than one per row.
    let mut favorites_on: Vec<String> = Vec::new();
    let mut favorites_off: Vec<String> = Vec::new();
    for state in states {
        if state.favorite {
            favorites_on.push(state.path.clone());
        } else {
            favorites_off.push(state.path.clone());
        }
    }

    if !favorites_on.is_empty() {
        touched += library.set_favorite(&favorites_on, true)?;
    }
    if !favorites_off.is_empty() {
        touched += library.set_favorite(&favorites_off, false)?;
    }
    let restore_states: Vec<mio_index::EditState> = states
        .iter()
        .map(|state| mio_index::EditState {
            path: state.path.clone(),
            favorite: state.favorite,
            category: state.category.clone(),
            subcategory: state.subcategory.clone(),
            category_manual: state.category_manual,
            ucs_category: state.ucs_category.clone(),
            ucs_subcategory: state.ucs_subcategory.clone(),
            ucs_cat_id: state.ucs_cat_id.clone(),
            ucs_manual: state.ucs_manual,
        })
        .collect();
    touched += library.restore_category_and_ucs(&restore_states)?;
    Ok(touched)
}

/// Apply an edit and return it in invertible form, plus how many rows it changed.
///
/// Reads the previous state first, then runs the edit. The two are not one
/// transaction; a failure between them leaves the edit unapplied and nothing on
/// the undo stack, which is the safe direction (no phantom undo entry).
///
/// The changed count comes from the caller's `UPDATE` rather than `paths.len()`:
/// clicking a star that is already on, or re-applying the category a row already
/// has, reports 0 — and the renderer uses that number to decide whether the rows
/// on screen are still valid.
pub fn run<F>(
    library: &Library,
    kind: EditKind,
    paths: &[String],
    apply: F,
) -> AppResult<(Edit, u64)>
where
    F: FnOnce() -> AppResult<u64>,
{
    let before = snapshot(library, paths)?;
    let changed = apply()?;
    Ok((
        Edit {
            kind,
            root: library.root().to_path_buf(),
            before,
        },
        changed,
    ))
}

pub fn edit_from_snapshot(
    library: &Library,
    kind: EditKind,
    before: Vec<mio_index::EditState>,
) -> Edit {
    Edit {
        kind,
        root: library.root().to_path_buf(),
        before: before
            .into_iter()
            .map(FieldState::from_edit_state)
            .collect(),
    }
}

impl History {
    /// Undo the last edit against `libraries`, returning what kind it was.
    pub fn undo(&mut self, libraries: &[std::sync::Arc<Library>]) -> AppResult<Option<EditKind>> {
        let Some(edit) = self.take_undo() else {
            return Ok(None);
        };
        let Some(library) = libraries.iter().find(|l| l.root() == edit.root.as_path()) else {
            // The library was closed since the edit. Dropping the entry is better
            // than undoing into a different database.
            return Ok(None);
        };

        // Capture the post-edit state so redo can put it back.
        let paths: Vec<String> = edit.before.iter().map(|s| s.path.clone()).collect();
        let after = snapshot(library, &paths)?;
        restore(library, &edit.before)?;

        let kind = edit.kind;
        self.push_redo(Edit {
            kind,
            root: edit.root.clone(),
            before: after,
        });
        Ok(Some(kind))
    }

    /// Redo the last undone edit.
    pub fn redo(&mut self, libraries: &[std::sync::Arc<Library>]) -> AppResult<Option<EditKind>> {
        let Some(edit) = self.take_redo() else {
            return Ok(None);
        };
        let Some(library) = libraries.iter().find(|l| l.root() == edit.root.as_path()) else {
            return Ok(None);
        };

        let paths: Vec<String> = edit.before.iter().map(|s| s.path.clone()).collect();
        let after = snapshot(library, &paths)?;
        restore(library, &edit.before)?;

        let kind = edit.kind;
        // Back onto the undo stack without clearing the rest of the redo stack:
        // redo is being walked forward, not forking.
        self.push_undo_raw(Edit {
            kind,
            root: edit.root.clone(),
            before: after,
        });
        Ok(Some(kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(path: &str, favorite: bool, category: &str) -> FieldState {
        FieldState {
            path: path.into(),
            favorite,
            category: category.into(),
            subcategory: None,
            category_manual: false,
            ucs_category: None,
            ucs_subcategory: None,
            ucs_cat_id: None,
            ucs_manual: false,
        }
    }

    fn edit(kind: EditKind) -> Edit {
        Edit {
            kind,
            root: PathBuf::from(r"C:\lib"),
            before: vec![state("a.wav", false, "Foley")],
        }
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack() {
        let mut history = History::new();
        history.push(edit(EditKind::Favorite));
        // Pretend something was undone, then edited again.
        history.redo.push(edit(EditKind::Favorite));
        assert!(history.can_redo());

        history.push(edit(EditKind::SetCategory));
        assert!(
            !history.can_redo(),
            "editing after an undo forks the timeline"
        );
    }

    #[test]
    fn the_stack_is_bounded() {
        let mut history = History::new();
        for _ in 0..(MAX_DEPTH + 10) {
            history.push(edit(EditKind::Favorite));
        }
        assert_eq!(history.undo.len(), MAX_DEPTH);
    }

    #[test]
    fn the_kind_describes_what_would_happen() {
        let mut history = History::new();
        assert_eq!(history.undo_kind(), None);

        history.push(edit(EditKind::Favorite));
        assert_eq!(history.undo_kind(), Some(EditKind::Favorite));

        let taken = history.take_undo().expect("one entry");
        history.push_redo(taken);
        assert_eq!(history.redo_kind(), Some(EditKind::Favorite));
        assert_eq!(history.undo_kind(), None);
    }
}
