use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Chronological history of documents opened in the editor, with independent
/// Back/Forward navigation like a browser's history stack, plus the last known
/// cursor position within every document ever visited — restored automatically
/// whenever that document is loaded again (see `SmaragdApp::load_document`).
#[derive(Debug, Default)]
pub(super) struct DocumentHistory {
    /// Visited documents in order. A document can appear more than once if
    /// revisited independently of Back/Forward (e.g. clicking it again in the
    /// Binder) — this deliberately mirrors a browser's history stack rather
    /// than deduplicating, so Back always undoes the most recent navigation.
    entries: Vec<PathBuf>,
    /// Index into `entries` for the document currently considered "here".
    /// `None` exactly when `entries` is empty.
    position: Option<usize>,
    /// The last known cursor byte offset for every document ever visited,
    /// updated by `record_cursor` right before navigating away from it.
    cursor_positions: HashMap<PathBuf, usize>,
}

impl DocumentHistory {
    fn current(&self) -> Option<&Path> {
        self.position.map(|index| self.entries[index].as_path())
    }

    /// Record that `path` was just opened as a fresh navigation (not a
    /// Back/Forward step) — pushes it past the current position, discarding
    /// any forward entries, exactly like a browser tab opening a new page
    /// after going back. A no-op if `path` is already the current entry, so
    /// re-clicking the same document (e.g. in the Binder) doesn't grow the
    /// stack.
    pub(super) fn visit(&mut self, path: &Path) {
        if self.current() == Some(path) {
            return;
        }
        let next_position = match self.position {
            Some(index) => {
                self.entries.truncate(index + 1);
                index + 1
            }
            None => 0,
        };
        self.entries.push(path.to_path_buf());
        self.position = Some(next_position);
    }

    /// The document Back would move to, without moving there — used both to
    /// gate whether the Back menu item/shortcut is enabled, and by
    /// `go_back_document` to know the target before committing to the move
    /// (e.g. to ask for collaboration-session confirmation first).
    pub(super) fn previous(&self) -> Option<&Path> {
        let index = self.position?;
        (index > 0).then(|| self.entries[index - 1].as_path())
    }

    /// The document Forward would move to — see `previous`.
    pub(super) fn next(&self) -> Option<&Path> {
        let index = self.position?;
        self.entries.get(index + 1).map(PathBuf::as_path)
    }

    pub(super) fn can_go_back(&self) -> bool {
        self.previous().is_some()
    }

    pub(super) fn can_go_forward(&self) -> bool {
        self.next().is_some()
    }

    /// Move one step back. A no-op if `previous()` is `None`.
    pub(super) fn go_back(&mut self) {
        if let Some(index) = self.position
            && index > 0
        {
            self.position = Some(index - 1);
        }
    }

    /// Move one step forward. A no-op if `next()` is `None`.
    pub(super) fn go_forward(&mut self) {
        if let Some(index) = self.position
            && index + 1 < self.entries.len()
        {
            self.position = Some(index + 1);
        }
    }

    pub(super) fn record_cursor(&mut self, path: &Path, byte_offset: usize) {
        self.cursor_positions
            .insert(path.to_path_buf(), byte_offset);
    }

    pub(super) fn cursor_for(&self, path: &Path) -> Option<usize> {
        self.cursor_positions.get(path).copied()
    }

    /// Drop every entry (and remembered cursor) for `path` or anything inside
    /// it — called when a document or folder is deleted, so Back/Forward
    /// never lands on a file that no longer exists. Shifts `position` to stay
    /// pointing at the same surviving entry it did before the removal (or the
    /// nearest one, if the current entry itself was removed).
    pub(super) fn remove_subtree(&mut self, path: &Path) {
        let removed = |entry: &PathBuf| entry.as_path() == path || entry.starts_with(path);
        self.cursor_positions.retain(|entry, _| !removed(entry));

        let Some(old_position) = self.position else {
            return;
        };
        let mut new_position = None;
        let mut kept = Vec::with_capacity(self.entries.len());
        for (index, entry) in self.entries.drain(..).enumerate() {
            if removed(&entry) {
                continue;
            }
            if index <= old_position {
                new_position = Some(kept.len());
            }
            kept.push(entry);
        }
        self.entries = kept;
        self.position = new_position.or(if self.entries.is_empty() {
            None
        } else {
            Some(0)
        });
    }

    /// Replace every entry pointing at `old` with `new` — called when the
    /// currently open document is renamed, so its history entry and
    /// remembered cursor keep following it under the new path rather than
    /// going stale.
    pub(super) fn rename_path(&mut self, old: &Path, new: &Path) {
        for entry in &mut self.entries {
            if entry.as_path() == old {
                *entry = new.to_path_buf();
            }
        }
        if let Some(cursor) = self.cursor_positions.remove(old) {
            self.cursor_positions.insert(new.to_path_buf(), cursor);
        }
    }

    /// Rewrite every entry (and remembered cursor) under `old_root` to sit
    /// under `new_root` instead — called when a file or folder is moved (a
    /// binder drag-and-drop), mirroring the same `strip_prefix`/`join` rebase
    /// `SmaragdApp::move_item` already applies to `selected_path`/
    /// `editor.open_path`.
    pub(super) fn rebase_subtree(&mut self, old_root: &Path, new_root: &Path) {
        let rebase = |p: &Path| -> Option<PathBuf> {
            p.strip_prefix(old_root)
                .ok()
                .map(|rest| new_root.join(rest))
        };
        for entry in &mut self.entries {
            if let Some(rebased) = rebase(entry) {
                *entry = rebased;
            }
        }
        let rebased_cursors: Vec<(PathBuf, usize)> = self
            .cursor_positions
            .iter()
            .filter_map(|(path, &cursor)| rebase(path).map(|rebased| (rebased, cursor)))
            .collect();
        self.cursor_positions
            .retain(|path, _| rebase(path).is_none());
        self.cursor_positions.extend(rebased_cursors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visiting_documents_builds_a_linear_history() {
        let mut history = DocumentHistory::default();
        history.visit(Path::new("a.md"));
        history.visit(Path::new("b.md"));
        history.visit(Path::new("c.md"));

        assert_eq!(history.previous(), Some(Path::new("b.md")));
        assert!(history.next().is_none());
    }

    #[test]
    fn revisiting_the_current_document_does_not_grow_the_stack() {
        let mut history = DocumentHistory::default();
        history.visit(Path::new("a.md"));
        history.visit(Path::new("a.md"));

        assert!(history.previous().is_none());
    }

    #[test]
    fn back_and_forward_move_the_position_without_losing_forward_entries() {
        let mut history = DocumentHistory::default();
        history.visit(Path::new("a.md"));
        history.visit(Path::new("b.md"));
        history.visit(Path::new("c.md"));

        history.go_back();
        assert_eq!(history.previous(), Some(Path::new("a.md")));
        assert_eq!(history.next(), Some(Path::new("c.md")));

        history.go_forward();
        assert_eq!(history.next(), None);
    }

    #[test]
    fn visiting_a_new_document_after_going_back_discards_the_forward_stack() {
        let mut history = DocumentHistory::default();
        history.visit(Path::new("a.md"));
        history.visit(Path::new("b.md"));
        history.go_back();

        history.visit(Path::new("d.md"));

        assert_eq!(history.previous(), Some(Path::new("a.md")));
        assert!(history.next().is_none());
    }

    #[test]
    fn go_back_and_go_forward_are_no_ops_at_the_ends() {
        let mut history = DocumentHistory::default();
        history.visit(Path::new("a.md"));

        history.go_back();
        assert_eq!(history.current(), Some(Path::new("a.md")));

        history.go_forward();
        assert_eq!(history.current(), Some(Path::new("a.md")));
    }

    #[test]
    fn cursor_positions_round_trip_by_path() {
        let mut history = DocumentHistory::default();
        history.record_cursor(Path::new("a.md"), 42);

        assert_eq!(history.cursor_for(Path::new("a.md")), Some(42));
        assert_eq!(history.cursor_for(Path::new("b.md")), None);
    }

    #[test]
    fn remove_subtree_drops_matching_entries_and_their_cursors() {
        let mut history = DocumentHistory::default();
        history.visit(Path::new("folder/a.md"));
        history.visit(Path::new("b.md"));
        history.record_cursor(Path::new("folder/a.md"), 5);

        history.remove_subtree(Path::new("folder"));

        assert_eq!(history.cursor_for(Path::new("folder/a.md")), None);
        assert!(history.previous().is_none());
        assert_eq!(history.current(), Some(Path::new("b.md")));
    }

    #[test]
    fn remove_subtree_of_the_current_entry_falls_back_to_the_nearest_survivor() {
        let mut history = DocumentHistory::default();
        history.visit(Path::new("a.md"));
        history.visit(Path::new("b.md"));

        history.remove_subtree(Path::new("b.md"));

        assert_eq!(history.current(), Some(Path::new("a.md")));
    }

    #[test]
    fn rename_path_updates_entries_and_cursor_key() {
        let mut history = DocumentHistory::default();
        history.visit(Path::new("old.md"));
        history.record_cursor(Path::new("old.md"), 7);

        history.rename_path(Path::new("old.md"), Path::new("new.md"));

        assert_eq!(history.current(), Some(Path::new("new.md")));
        assert_eq!(history.cursor_for(Path::new("new.md")), Some(7));
        assert_eq!(history.cursor_for(Path::new("old.md")), None);
    }
}
