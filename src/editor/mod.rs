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
}
