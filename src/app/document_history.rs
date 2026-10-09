use std::path::{Path, PathBuf};

/// One tab's own Back/Forward lineage — independent browser-style navigation
/// per open tab (see `editor::OpenDocument::history` and issue #40), seeded
/// by cloning whichever tab was active when this one was opened as a fresh
/// navigation, then extended with its own path (see `EditorState::open`).
#[derive(Debug, Default, Clone)]
pub(crate) struct DocumentHistory {
    /// Visited documents in order. A document can appear more than once if
    /// revisited independently of Back/Forward (e.g. clicking it again in the
    /// Binder) — this deliberately mirrors a browser's history stack rather
    /// than deduplicating, so Back always undoes the most recent navigation.
    entries: Vec<PathBuf>,
    /// Index into `entries` for the document currently considered "here".
    /// `None` exactly when `entries` is empty.
    position: Option<usize>,
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
    pub(crate) fn visit(&mut self, path: &Path) {
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
    /// `EditorState::go_back` to know the target before committing to the
    /// move.
    pub(crate) fn previous(&self) -> Option<&Path> {
        let index = self.position?;
        (index > 0).then(|| self.entries[index - 1].as_path())
    }

    /// The document Forward would move to — see `previous`.
    pub(crate) fn next(&self) -> Option<&Path> {
        let index = self.position?;
        self.entries.get(index + 1).map(PathBuf::as_path)
    }

    /// Visited documents, most-recently-visited first, deduplicated (a document
    /// revisited later moves to the front rather than appearing twice) — the
    /// "Opened" mode data source for the Recent Files switcher
    /// (`ShortcutAction::RecentFiles`). Capped at `limit`.
    pub(crate) fn recent_documents(&self, limit: usize) -> Vec<&Path> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for entry in self.entries.iter().rev() {
            if seen.insert(entry.as_path()) {
                result.push(entry.as_path());
                if result.len() == limit {
                    break;
                }
            }
        }
        result
    }

    pub(crate) fn can_go_back(&self) -> bool {
        self.previous().is_some()
    }

    pub(crate) fn can_go_forward(&self) -> bool {
        self.next().is_some()
    }

    /// Move one step back. A no-op if `previous()` is `None`.
    pub(crate) fn go_back(&mut self) {
        if let Some(index) = self.position
            && index > 0
        {
            self.position = Some(index - 1);
        }
    }

    /// Move one step forward. A no-op if `next()` is `None`.
    pub(crate) fn go_forward(&mut self) {
        if let Some(index) = self.position
            && index + 1 < self.entries.len()
        {
            self.position = Some(index + 1);
        }
    }

    /// Replace every entry pointing at `old` with `new` — called when the
    /// currently open document is renamed, so its history entry keeps
    /// following it under the new path rather than going stale.
    pub(crate) fn rename_path(&mut self, old: &Path, new: &Path) {
        for entry in &mut self.entries {
            if entry.as_path() == old {
                *entry = new.to_path_buf();
            }
        }
    }

    /// Rewrite every entry under `old_root` to sit under `new_root` instead —
    /// called when a file or folder is moved (a binder drag-and-drop),
    /// mirroring the same `strip_prefix`/`join` rebase `SmaragdApp::move_item`
    /// already applies to `selected_path`.
    pub(crate) fn rebase_subtree(&mut self, old_root: &Path, new_root: &Path) {
        for entry in &mut self.entries {
            if let Some(rest) = entry.strip_prefix(old_root).ok().map(Path::to_path_buf) {
                *entry = new_root.join(rest);
            }
        }
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
    fn recent_documents_is_most_recent_first_and_deduplicated() {
        let mut history = DocumentHistory::default();
        history.visit(Path::new("a.md"));
        history.visit(Path::new("b.md"));
        history.visit(Path::new("a.md")); // revisit "a.md", should move to the front

        assert_eq!(
            history.recent_documents(10),
            vec![Path::new("a.md"), Path::new("b.md")]
        );
    }

    #[test]
    fn recent_documents_respects_the_limit() {
        let mut history = DocumentHistory::default();
        history.visit(Path::new("a.md"));
        history.visit(Path::new("b.md"));
        history.visit(Path::new("c.md"));

        assert_eq!(
            history.recent_documents(2),
            vec![Path::new("c.md"), Path::new("b.md")]
        );
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
    fn rename_path_updates_matching_entries() {
        let mut history = DocumentHistory::default();
        history.visit(Path::new("old.md"));

        history.rename_path(Path::new("old.md"), Path::new("new.md"));

        assert_eq!(history.current(), Some(Path::new("new.md")));
    }

    #[test]
    fn rebase_subtree_rewrites_entries_under_the_moved_root() {
        let mut history = DocumentHistory::default();
        history.visit(Path::new("folder/a.md"));
        history.visit(Path::new("other.md"));

        history.rebase_subtree(Path::new("folder"), Path::new("moved"));

        assert_eq!(history.previous(), Some(Path::new("moved/a.md")));
    }
}
