use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The single currently-open document. Milestone 1 supports editing one file at a
/// time — no tabs, no split view.
#[derive(Debug, Default)]
pub struct EditorState {
    pub open_path: Option<PathBuf>,
    pub buffer: String,
    pub dirty: bool,
    /// Every document edited at least once since the app launched, kept even after
    /// it's saved (e.g. by switching away) — backs the "Modified Files" search scope,
    /// which otherwise has nothing to point at since only one document is ever open
    /// (and thus `dirty`) at a time.
    pub modified_paths: BTreeSet<PathBuf>,
    /// Byte offset of the text cursor in `buffer`, refreshed every frame the
    /// editor panel renders (see `editor_panel::show`). Lets document-history
    /// navigation (`SmaragdApp::document_history`) record "where was I" in the
    /// outgoing document without needing a live `egui::Context` at every
    /// `open_document` call site.
    pub cursor_byte: usize,
    /// A byte offset the editor panel should move the cursor to on its next
    /// render, then clear — set right after `open` whenever the caller knows
    /// (or wants to reset) where the cursor belongs, e.g. restoring the last
    /// known position for a document reopened via Back/Forward.
    pub pending_cursor: Option<usize>,
}

impl EditorState {
    /// Open `path` for editing, first saving the previously open file if it was dirty.
    pub fn open(&mut self, path: &Path) -> io::Result<()> {
        self.save_if_dirty()?;
        let contents = fs::read_to_string(path)?;
        self.open_path = Some(path.to_path_buf());
        self.buffer = contents;
        self.dirty = false;
        Ok(())
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        if let Some(path) = &self.open_path {
            self.modified_paths.insert(path.clone());
        }
    }

    /// Write the buffer to `open_path`. A no-op (not an error) if nothing is open.
    pub fn save(&mut self) -> io::Result<()> {
        if let Some(path) = &self.open_path {
            fs::write(path, &self.buffer)?;
            self.dirty = false;
        }
        Ok(())
    }

    fn save_if_dirty(&mut self) -> io::Result<()> {
        if self.dirty { self.save() } else { Ok(()) }
    }

    /// Close the currently open document, if any — saving first if dirty (same
    /// silent-autosave convention as `open`, no discard/cancel prompt). A no-op if
    /// nothing is open.
    pub fn close(&mut self) -> io::Result<()> {
        self.save_if_dirty()?;
        self.open_path = None;
        self.buffer.clear();
        self.dirty = false;
        self.cursor_byte = 0;
        self.pending_cursor = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_loads_file_contents_and_clears_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "Once upon a time.").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();

        assert_eq!(state.buffer, "Once upon a time.");
        assert_eq!(state.open_path.as_deref(), Some(path.as_path()));
        assert!(!state.dirty);
    }

    #[test]
    fn save_writes_buffer_to_disk_and_clears_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "original").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();
        state.buffer = "edited content".to_string();
        state.mark_dirty();
        state.save().unwrap();

        assert!(!state.dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "edited content");
    }

    #[test]
    fn save_without_open_path_is_a_no_op() {
        let mut state = EditorState {
            buffer: "orphan text".to_string(),
            dirty: true,
            ..Default::default()
        };

        assert!(state.save().is_ok());
        assert!(
            state.dirty,
            "nothing was written, so dirty should remain true"
        );
        assert_eq!(state.buffer, "orphan text");
    }

    #[test]
    fn opening_a_new_file_saves_the_previously_dirty_file_first() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        fs::write(&first, "first original").unwrap();
        fs::write(&second, "second original").unwrap();

        let mut state = EditorState::default();
        state.open(&first).unwrap();
        state.buffer = "first edited".to_string();
        state.mark_dirty();

        state.open(&second).unwrap();

        assert_eq!(fs::read_to_string(&first).unwrap(), "first edited");
        assert_eq!(state.buffer, "second original");
        assert!(!state.dirty);
    }

    #[test]
    fn opening_a_nonexistent_file_returns_err_and_leaves_previous_state_saved() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("existing.md");
        fs::write(&existing, "existing content").unwrap();
        let missing = dir.path().join("missing.md");

        let mut state = EditorState::default();
        state.open(&existing).unwrap();
        state.buffer = "edited".to_string();
        state.mark_dirty();

        let result = state.open(&missing);

        assert!(result.is_err());
        // The dirty file was flushed before the failed open attempt.
        assert_eq!(fs::read_to_string(&existing).unwrap(), "edited");
    }

    #[test]
    fn mark_dirty_records_the_open_path_as_modified() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "original").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();
        state.mark_dirty();

        assert!(state.modified_paths.contains(&path));
    }

    #[test]
    fn modified_paths_persists_across_saving_and_switching_files() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        fs::write(&first, "first original").unwrap();
        fs::write(&second, "second original").unwrap();

        let mut state = EditorState::default();
        state.open(&first).unwrap();
        state.mark_dirty();
        state.open(&second).unwrap(); // auto-saves and clears `dirty` for `first`

        assert!(
            state.modified_paths.contains(&first),
            "switching away shouldn't forget that a file was edited this session"
        );
        assert!(!state.modified_paths.contains(&second));
    }

    #[test]
    fn close_saves_a_dirty_document_first_then_clears_editor_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "original").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();
        state.buffer = "edited content".to_string();
        state.mark_dirty();

        state.close().unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "edited content");
        assert_eq!(state.open_path, None);
        assert_eq!(state.buffer, "");
        assert!(!state.dirty);
    }

    #[test]
    fn close_with_nothing_open_is_a_no_op() {
        let mut state = EditorState::default();

        assert!(state.close().is_ok());
        assert_eq!(state.open_path, None);
        assert_eq!(state.buffer, "");
    }
}
