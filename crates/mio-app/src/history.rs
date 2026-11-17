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

use crate::error::AppResult;
use crate::library::Library;

/// How many edits can be taken back. Deep enough for a working session, shallow
/// enough that a 280k-row "select all, categorise" undo stack stays bounded.
const MAX_DEPTH: usize = 50;

/// One field of one sound, as it was before the edit.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldState {
    pub path: String,
    pub favorite: bool,
    pub category: String,
    pub subcategory: Option<String>,
    pub category_manual: bool,
}

impl FieldState {
    fn from_sound(sound: &Sound) -> Self {
        Self {
            path: sound.path.clone(),
            favorite: sound.favorite,
            category: sound.category.clone(),
            subcategory: sound.subcategory.clone(),
            category_manual: sound.category_manual,
        }
    }
}

/// A completed edit, stored in the form needed to invert it.
#[derive(Debug, Clone)]
pub struct Edit {
    /// What the user did, for the menu label ("Favorite", "Set category").
    pub label: String,
    /// The library the edit belongs to. Undo must go back to the same database
    /// even if the user has since switched to another library.
    pub root: PathBuf,
    pub before: Vec<FieldState>,
}

/// Undo and redo stacks, as one unit.
#[derive(Default)]
pub struct History {
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed edit. Any pending redo is dropped: the timeline forked.
    pub fn push(&mut self, edit: Edit) {
        self.redo.clear();
        self.undo.push_back(edit);
        while self.undo.len() > MAX_DEPTH {
            self.undo.pop_front();
        }
    }

    /// Only the unit tests ask this; the app decides from the labels.
    #[cfg(test)]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.back().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    fn take_undo(&mut self) -> Option<Edit> {
        self.undo.pop_back()
    }

    fn take_redo(&mut self) -> Option<Edit> {
        self.redo.pop()
    }

    fn push_redo(&mut self, edit: Edit) {
        self.redo.push(edit);
    }

    fn push_undo_raw(&mut self, edit: Edit) {
        self.undo.push_back(edit);
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
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
    let mut by_category: std::collections::BTreeMap<
        (String, Option<String>, bool),
        Vec<String>,
    > = Default::default();

    for state in states {
        if state.favorite {
            favorites_on.push(state.path.clone());
        } else {
            favorites_off.push(state.path.clone());
        }
        by_category
            .entry((
                state.category.clone(),
                state.subcategory.clone(),
                state.category_manual,
            ))
            .or_default()
            .push(state.path.clone());
    }

    if !favorites_on.is_empty() {
        touched += library.set_favorite(&favorites_on, true)?;
    }
    if !favorites_off.is_empty() {
        touched += library.set_favorite(&favorites_off, false)?;
    }
    for ((category, subcategory, manual), paths) in by_category {
        // Always the exact-restore path, never `set_category`/`clear_category`:
        // those two can only produce "manual with this string" or "Uncategorized",
        // and the state being restored is often neither.
        touched += library.restore_category(&paths, &category, subcategory.as_deref(), manual)?;
    }
    Ok(touched)
}

/// Apply an edit and return it in invertible form.
///
/// Reads the previous state first, then runs the edit. The two are not one
/// transaction; a failure between them leaves the edit unapplied and nothing on
/// the undo stack, which is the safe direction (no phantom undo entry).
pub fn run<F>(library: &Library, label: &str, paths: &[String], apply: F) -> AppResult<Edit>
where
    F: FnOnce() -> AppResult<u64>,
{
    let before = snapshot(library, paths)?;
    apply()?;
    Ok(Edit {
        label: label.to_string(),
        root: library.root().to_path_buf(),
        before,
    })
}

impl History {
    /// Undo the last edit against `libraries`, returning the label that was undone.
    pub fn undo(&mut self, libraries: &[std::sync::Arc<Library>]) -> AppResult<Option<String>> {
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

        let label = edit.label.clone();
        self.push_redo(Edit {
            label: label.clone(),
            root: edit.root.clone(),
            before: after,
        });
        Ok(Some(label))
    }

    /// Redo the last undone edit.
    pub fn redo(&mut self, libraries: &[std::sync::Arc<Library>]) -> AppResult<Option<String>> {
        let Some(edit) = self.take_redo() else {
            return Ok(None);
        };
        let Some(library) = libraries.iter().find(|l| l.root() == edit.root.as_path()) else {
            return Ok(None);
        };

        let paths: Vec<String> = edit.before.iter().map(|s| s.path.clone()).collect();
        let after = snapshot(library, &paths)?;
        restore(library, &edit.before)?;

        let label = edit.label.clone();
        // Back onto the undo stack without clearing the rest of the redo stack:
        // redo is being walked forward, not forking.
        self.push_undo_raw(Edit {
            label: label.clone(),
            root: edit.root.clone(),
            before: after,
        });
        Ok(Some(label))
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
        }
    }

    fn edit(label: &str) -> Edit {
        Edit {
            label: label.into(),
            root: PathBuf::from(r"C:\lib"),
            before: vec![state("a.wav", false, "Foley")],
        }
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack() {
        let mut history = History::new();
        history.push(edit("one"));
        // Pretend something was undone, then edited again.
        history.redo.push(edit("one"));
        assert!(history.can_redo());

        history.push(edit("two"));
        assert!(!history.can_redo(), "editing after an undo forks the timeline");
    }

    #[test]
    fn the_stack_is_bounded() {
        let mut history = History::new();
        for i in 0..(MAX_DEPTH + 10) {
            history.push(edit(&format!("edit {i}")));
        }
        assert_eq!(history.undo.len(), MAX_DEPTH);
        // The oldest entries are the ones dropped.
        assert_eq!(history.undo_label(), Some(format!("edit {}", MAX_DEPTH + 9).as_str()));
    }

    #[test]
    fn labels_describe_what_would_happen() {
        let mut history = History::new();
        assert_eq!(history.undo_label(), None);

        history.push(edit("Favorite"));
        assert_eq!(history.undo_label(), Some("Favorite"));

        let taken = history.take_undo().expect("one entry");
        history.push_redo(taken);
        assert_eq!(history.redo_label(), Some("Favorite"));
        assert_eq!(history.undo_label(), None);
    }
}
