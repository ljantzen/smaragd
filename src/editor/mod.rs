use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::app::document_history::DocumentHistory;
use crate::project::store::ProjectStore;

/// One open document: an independent, fully resident in-memory buffer with
/// its own dirty flag, cursor and Back/Forward lineage. Switching which tab
/// is active never touches disk — a tab is only written out on close,
/// explicit Save, or app exit (see issue #40).
///
/// `path` is `None` only for the single pathless tab a joined collaboration
/// session gets (see `EditorState::open_collab_tab`): that shared buffer
/// isn't tied to any of the joiner's own files, so it's never saved,
/// deduplicated against another tab, or reachable by path.
#[derive(Debug, Default, Clone)]
pub struct OpenDocument {
    pub path: Option<PathBuf>,
    pub buffer: String,
    pub dirty: bool,
    /// Byte offset of the text cursor in `buffer`, refreshed every frame the
    /// editor panel renders this tab (see `editor_panel::show`). Stays valid
    /// for as long as the tab stays open — unlike the old single-buffer
    /// model, nothing needs to proactively "record" it before switching
    /// tabs, since each tab's buffer and cursor simply stay resident.
    pub cursor_byte: usize,
    /// A byte offset the editor panel should move the cursor to on its next
    /// render, then clear — set right after this tab is created, when the
    /// caller knows (or wants to reset) where the cursor belongs, e.g.
    /// restoring the last known position for a document reopened via
    /// Back/Forward after its tab had been closed.
    pub pending_cursor: Option<usize>,
    /// `path`'s on-disk mtime as of the last time this tab read or wrote it —
    /// compared against the file's *current* mtime by `changed_on_disk` to
    /// notice another program (a sync tool, `git pull`, a hand edit) touched
    /// it since. `None` for a pathless tab, or if the mtime couldn't be read.
    pub disk_mtime: Option<SystemTime>,
    /// This tab's own Back/Forward trail: seeded by cloning whichever tab
    /// was active when this one was opened as a fresh navigation (not a
    /// focus switch to an already-open tab), then extended with this tab's
    /// own path — see `EditorState::open`. Back/Forward steps resolve to a
    /// path, which `EditorState` then focuses (or transparently reopens) as
    /// a tab, rather than ever swapping this tab's own content in place.
    pub(crate) history: DocumentHistory,
}

impl OpenDocument {
    fn save_with_store(&mut self, store: &dyn ProjectStore) -> io::Result<()> {
        if let Some(path) = &self.path {
            store.write(path, self.buffer.as_bytes())?;
            self.dirty = false;
            self.disk_mtime = read_mtime(path);
        }
        Ok(())
    }

    fn save_if_dirty_with_store(&mut self, store: &dyn ProjectStore) -> io::Result<()> {
        if self.dirty {
            self.save_with_store(store)
        } else {
            Ok(())
        }
    }

    fn changed_on_disk(&self) -> bool {
        match &self.path {
            Some(path) => read_mtime(path).is_some_and(|current| Some(current) != self.disk_mtime),
            None => false,
        }
    }

    fn reload_from_disk_with_store(&mut self, store: &dyn ProjectStore) -> io::Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        self.buffer = store.read_to_string(&path)?;
        self.dirty = false;
        self.disk_mtime = read_mtime(&path);
        Ok(())
    }

    fn acknowledge_disk_change(&mut self) {
        if let Some(path) = &self.path {
            self.disk_mtime = read_mtime(path);
        }
    }
}

/// Every document currently open for editing, each in its own tab with a
/// fully resident buffer. Milestone follow-up to the old single-document
/// model (see issue #40): switching the active tab is instant and does no
/// disk I/O; a tab is only saved on close, explicit Save, or app exit.
#[derive(Debug, Default)]
pub struct EditorState {
    pub tabs: Vec<OpenDocument>,
    pub active: Option<usize>,
    /// Every document edited at least once since the app launched, kept even
    /// after it's saved — backs the "Modified Files" search scope. Session-
    /// wide rather than per-tab: a document stays "modified this session"
    /// even after its tab is closed.
    pub modified_paths: BTreeSet<PathBuf>,
    /// Documents edited this session, in edit order (oldest first) — a
    /// document edited again moves to the end rather than appearing twice.
    /// Backs the "Edited" mode of the Recent Files switcher. Session-wide,
    /// like `modified_paths`.
    pub edited_order: Vec<PathBuf>,
}

impl EditorState {
    pub fn active_tab(&self) -> Option<&OpenDocument> {
        self.active.and_then(|index| self.tabs.get(index))
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut OpenDocument> {
        self.active.and_then(|index| self.tabs.get_mut(index))
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn iter_tabs(&self) -> impl Iterator<Item = &OpenDocument> {
        self.tabs.iter()
    }

    pub fn tab_index_for(&self, path: &Path) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.path.as_deref() == Some(path))
    }

    pub fn open_path(&self) -> Option<&Path> {
        self.active_tab().and_then(|tab| tab.path.as_deref())
    }

    pub fn buffer(&self) -> &str {
        self.active_tab().map(|tab| tab.buffer.as_str()).unwrap_or("")
    }

    /// Panics if there's no active tab — only call where that's already
    /// guaranteed (e.g. inside `editor_panel::show`, which early-returns
    /// before reaching any mutation when nothing is open or collaborating).
    pub fn buffer_mut(&mut self) -> &mut String {
        &mut self
            .active_tab_mut()
            .expect("buffer_mut called with no active tab")
            .buffer
    }

    pub fn dirty(&self) -> bool {
        self.active_tab().is_some_and(|tab| tab.dirty)
    }

    pub fn cursor_byte(&self) -> usize {
        self.active_tab().map(|tab| tab.cursor_byte).unwrap_or(0)
    }

    pub fn set_cursor_byte(&mut self, byte: usize) {
        if let Some(tab) = self.active_tab_mut() {
            tab.cursor_byte = byte;
        }
    }

    pub fn pending_cursor(&self) -> Option<usize> {
        self.active_tab().and_then(|tab| tab.pending_cursor)
    }

    pub fn take_pending_cursor(&mut self) -> Option<usize> {
        self.active_tab_mut().and_then(|tab| tab.pending_cursor.take())
    }

    pub fn disk_mtime(&self) -> Option<SystemTime> {
        self.active_tab().and_then(|tab| tab.disk_mtime)
    }

    pub fn mark_dirty(&mut self) {
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        tab.dirty = true;
        if let Some(path) = tab.path.clone() {
            self.modified_paths.insert(path.clone());
            self.edited_order.retain(|p| p != &path);
            self.edited_order.push(path);
        }
    }

    /// The active tab's own Back/Forward trail, most-recently-visited first
    /// — the "Opened" mode data source for the Recent Files switcher. Empty
    /// with no active tab.
    pub fn recent_documents(&self, limit: usize) -> Vec<&Path> {
        self.active_tab()
            .map(|tab| tab.history.recent_documents(limit))
            .unwrap_or_default()
    }

    /// Edited documents, most-recently-edited first. Capped at `limit`.
    pub fn recently_edited(&self, limit: usize) -> Vec<&Path> {
        self.edited_order
            .iter()
            .rev()
            .take(limit)
            .map(PathBuf::as_path)
            .collect()
    }

    /// Open `path` as a navigation action: focuses its tab if it's already
    /// open (never re-reading from disk, so in-memory edits are never
    /// clobbered by clicking the same document again), otherwise reads it
    /// fresh into a brand-new tab and seeds that tab's own Back/Forward
    /// history from whichever tab was active (extended with this path) —
    /// see `OpenDocument::history`.
    pub fn open(&mut self, path: &Path) -> io::Result<()> {
        self.open_with_store(path, &crate::project::store::NativeStore)
    }

    /// [`Self::open`], against an explicitly chosen store rather than always
    /// [`crate::project::store::NativeStore`].
    pub fn open_with_store(&mut self, path: &Path, store: &dyn ProjectStore) -> io::Result<()> {
        if let Some(index) = self.tab_index_for(path) {
            self.active = Some(index);
            return Ok(());
        }
        let contents = store.read_to_string(path)?;
        let mut history = self
            .active_tab()
            .map(|tab| tab.history.clone())
            .unwrap_or_default();
        history.visit(path);
        self.tabs.push(OpenDocument {
            path: Some(path.to_path_buf()),
            buffer: contents,
            dirty: false,
            cursor_byte: 0,
            pending_cursor: None,
            disk_mtime: read_mtime(path),
            history,
        });
        self.active = Some(self.tabs.len() - 1);
        Ok(())
    }

    /// Restores a tab whose buffer had unsaved edits at the last session
    /// save (see `SmaragdApp::capture_session`/`restore_session`) — the
    /// in-memory content takes priority over whatever's on disk, restored
    /// verbatim and marked dirty, rather than silently discarding those
    /// edits by re-reading the file fresh.
    pub fn open_unsaved_tab(&mut self, path: PathBuf, buffer: String, cursor_byte: usize) {
        let disk_mtime = read_mtime(&path);
        self.tabs.push(OpenDocument {
            path: Some(path),
            buffer,
            dirty: true,
            cursor_byte,
            pending_cursor: Some(cursor_byte),
            disk_mtime,
            history: DocumentHistory::default(),
        });
        self.active = Some(self.tabs.len() - 1);
    }

    /// Opens a fresh, path-less tab for a joined collaboration session's
    /// shared buffer (see `collab::start_collab_join`) — never saved, never
    /// deduplicated against another tab.
    pub fn open_collab_tab(&mut self) {
        self.tabs.push(OpenDocument::default());
        self.active = Some(self.tabs.len() - 1);
    }

    /// Updates every tab's record of `old` after it's renamed to `new` on
    /// disk: the tab actually showing it (if any) follows to the new path
    /// rather than going stale, and every tab's own Back/Forward history —
    /// which may remember `old` as a past stop even if it's not that tab's
    /// current document — is rewritten to point at `new` instead.
    pub fn rename_path_everywhere(&mut self, old: &Path, new: &Path) {
        for tab in &mut self.tabs {
            if tab.path.as_deref() == Some(old) {
                tab.path = Some(new.to_path_buf());
                tab.disk_mtime = read_mtime(new);
            }
            tab.history.rename_path(old, new);
        }
    }

    /// Whether `old` is open in some tab, dirty, and should be saved under
    /// its *current* path before an on-disk rename/move of `old` runs.
    pub fn save_if_open_and_dirty_with_store(
        &mut self,
        path: &Path,
        store: &dyn ProjectStore,
    ) -> io::Result<()> {
        for tab in &mut self.tabs {
            if tab.path.as_deref() == Some(path) && tab.dirty {
                tab.save_with_store(store)?;
            }
        }
        Ok(())
    }

    pub fn save(&mut self) -> io::Result<()> {
        self.save_with_store(&crate::project::store::NativeStore)
    }

    /// Write the active tab's buffer to its path. A no-op (not an error) if
    /// there's no active tab, or it has none (a joined collab session).
    pub fn save_with_store(&mut self, store: &dyn ProjectStore) -> io::Result<()> {
        if let Some(tab) = self.active_tab_mut() {
            tab.save_with_store(store)
        } else {
            Ok(())
        }
    }

    /// Best-effort reload of every open, non-dirty tab from disk — used
    /// after a project-wide text rewrite (e.g. a tag rename) that may have
    /// touched any number of open documents, not just the active one.
    /// Dirty tabs are left alone, same reasoning as `reload_from_disk`.
    /// Errors on individual tabs are ignored (mirrors the single-document
    /// behavior this replaces, which silently dropped a reload failure too).
    pub fn reload_all_clean_tabs_with_store(&mut self, store: &dyn ProjectStore) {
        for tab in &mut self.tabs {
            if !tab.dirty {
                let _ = tab.reload_from_disk_with_store(store);
            }
        }
    }

    pub fn reload_from_disk(&mut self) -> io::Result<()> {
        self.reload_from_disk_with_store(&crate::project::store::NativeStore)
    }

    pub fn reload_from_disk_with_store(&mut self, store: &dyn ProjectStore) -> io::Result<()> {
        if let Some(tab) = self.active_tab_mut() {
            tab.reload_from_disk_with_store(store)
        } else {
            Ok(())
        }
    }

    pub fn changed_on_disk(&self) -> bool {
        self.active_tab().is_some_and(OpenDocument::changed_on_disk)
    }

    pub fn acknowledge_disk_change(&mut self) {
        if let Some(tab) = self.active_tab_mut() {
            tab.acknowledge_disk_change();
        }
    }

    pub fn close(&mut self) -> io::Result<()> {
        self.close_with_store(&crate::project::store::NativeStore)
    }

    /// Close the active tab, if any — saving first if dirty (same silent-
    /// autosave convention as `open`, no discard/cancel prompt).
    pub fn close_with_store(&mut self, store: &dyn ProjectStore) -> io::Result<()> {
        let Some(index) = self.active else {
            return Ok(());
        };
        self.close_tab(index, store)
    }

    /// Close a specific tab by index — same autosave convention as `close`.
    /// Fixes up `active` to a sane neighbor (or `None`, if that was the last
    /// tab). A no-op if `index` is out of range.
    pub fn close_tab(&mut self, index: usize, store: &dyn ProjectStore) -> io::Result<()> {
        if index >= self.tabs.len() {
            return Ok(());
        }
        self.tabs[index].save_if_dirty_with_store(store)?;
        self.tabs.remove(index);
        self.active = match self.active {
            Some(active) if active == index => {
                if self.tabs.is_empty() {
                    None
                } else {
                    Some(active.min(self.tabs.len() - 1))
                }
            }
            Some(active) if active > index => Some(active - 1),
            other => other,
        };
        Ok(())
    }

    /// Close every open tab — saving each dirty one first. Stops at the
    /// first save failure, leaving every tab up to that point saved but
    /// nothing removed yet, so a retry doesn't lose anything.
    pub fn close_all_with_store(&mut self, store: &dyn ProjectStore) -> io::Result<()> {
        for tab in &mut self.tabs {
            tab.save_if_dirty_with_store(store)?;
        }
        self.tabs.clear();
        self.active = None;
        Ok(())
    }

    /// Closes every tab except `keep` — saving each dirty one first — and
    /// leaves `keep` as the only (and active) tab. Used to enforce
    /// `Settings::multi_tab_editor` being off: rather than threading that
    /// setting through every tab-creating operation, callers just run this
    /// afterward to collapse back down to the single-document model. A
    /// no-op if `keep` is out of range.
    pub fn close_other_tabs_with_store(
        &mut self,
        keep: usize,
        store: &dyn ProjectStore,
    ) -> io::Result<()> {
        if keep >= self.tabs.len() {
            return Ok(());
        }
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            if index != keep {
                tab.save_if_dirty_with_store(store)?;
            }
        }
        let kept = self.tabs.swap_remove(keep);
        self.tabs.clear();
        self.tabs.push(kept);
        self.active = Some(0);
        Ok(())
    }

    /// Rewrites every tab's path (and Back/Forward history) under
    /// `old_root` to sit under `new_root` instead — called when a file or
    /// folder is moved (a binder drag-and-drop). Mirrors
    /// `rename_path_everywhere`, but for every path under a moved subtree
    /// rather than a single renamed one.
    pub fn rebase_subtree(&mut self, old_root: &Path, new_root: &Path) {
        let rebase = |p: &Path| -> Option<PathBuf> {
            p.strip_prefix(old_root).ok().map(|rest| new_root.join(rest))
        };
        for tab in &mut self.tabs {
            if let Some(rebased) = tab.path.as_deref().and_then(rebase) {
                tab.path = Some(rebased.clone());
                tab.disk_mtime = read_mtime(&rebased);
            }
            tab.history.rebase_subtree(old_root, new_root);
        }
    }

    /// Drops every open tab at or under `root` without saving — called
    /// after the user confirms deleting that file or folder, so there's
    /// nothing left to write it back to. Tabs outside the deleted subtree
    /// are untouched; the active tab stays active if it survives, otherwise
    /// falls back to whichever tab is now first.
    pub fn close_subtree(&mut self, root: &Path) {
        let in_subtree = |p: &Path| p == root || p.starts_with(root);
        let active_path = self.active_tab().and_then(|tab| tab.path.clone());
        self.tabs
            .retain(|tab| !tab.path.as_deref().is_some_and(in_subtree));
        self.active = active_path
            .and_then(|path| self.tab_index_for(&path))
            .or(if self.tabs.is_empty() { None } else { Some(0) });
    }

    pub fn can_go_back(&self) -> bool {
        self.active_tab().is_some_and(|tab| tab.history.can_go_back())
    }

    pub fn can_go_forward(&self) -> bool {
        self.active_tab().is_some_and(|tab| tab.history.can_go_forward())
    }

    /// Step to the document the active tab's own history would go back to —
    /// focusing its tab if it's still open, or transparently reopening it
    /// (fresh from disk, restoring its last known cursor) if it was closed
    /// since. A no-op if the active tab has nothing behind it.
    pub fn go_back(&mut self, store: &dyn ProjectStore) -> io::Result<()> {
        self.step_history(store, true)
    }

    /// [`Self::go_back`]'s forward counterpart.
    pub fn go_forward(&mut self, store: &dyn ProjectStore) -> io::Result<()> {
        self.step_history(store, false)
    }

    fn step_history(&mut self, store: &dyn ProjectStore, backward: bool) -> io::Result<()> {
        let Some(active) = self.active else {
            return Ok(());
        };
        let target = if backward {
            self.tabs[active].history.previous()
        } else {
            self.tabs[active].history.next()
        };
        let Some(target) = target.map(Path::to_path_buf) else {
            return Ok(());
        };
        if backward {
            self.tabs[active].history.go_back();
        } else {
            self.tabs[active].history.go_forward();
        }
        if let Some(index) = self.tab_index_for(&target) {
            self.active = Some(index);
            return Ok(());
        }
        let contents = store.read_to_string(&target)?;
        self.tabs.push(OpenDocument {
            path: Some(target.clone()),
            buffer: contents,
            pending_cursor: None,
            disk_mtime: read_mtime(&target),
            ..Default::default()
        });
        self.active = Some(self.tabs.len() - 1);
        Ok(())
    }
}

fn read_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

#[cfg(test)]
impl EditorState {
    /// Test-only convenience: an `EditorState` with a single active tab, set
    /// directly from the given fields rather than going through `open()`
    /// (which needs a real file on disk) — most callers outside this
    /// module's own tests just want *a* buffer to exercise, not disk I/O.
    pub fn test_with_tab(path: Option<PathBuf>, buffer: impl Into<String>) -> Self {
        let mut state = EditorState::default();
        state.tabs.push(OpenDocument {
            path,
            buffer: buffer.into(),
            ..Default::default()
        });
        state.active = Some(0);
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_loads_file_contents_into_a_new_active_tab() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "Once upon a time.").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();

        assert_eq!(state.buffer(), "Once upon a time.");
        assert_eq!(state.open_path(), Some(path.as_path()));
        assert!(!state.dirty());
        assert_eq!(state.tab_count(), 1);
    }

    #[test]
    fn opening_a_second_path_creates_a_second_tab_without_touching_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        fs::write(&first, "first original").unwrap();
        fs::write(&second, "second original").unwrap();

        let mut state = EditorState::default();
        state.open(&first).unwrap();
        *state.buffer_mut() = "first edited".to_string();
        state.mark_dirty();

        state.open(&second).unwrap();

        assert_eq!(state.tab_count(), 2);
        assert_eq!(state.buffer(), "second original");
        assert!(!state.dirty());
        // Switching tabs did no disk I/O: the first tab's edit is still only
        // in memory, not flushed to its file.
        assert_eq!(fs::read_to_string(&first).unwrap(), "first original");

        state.open(&first).unwrap();
        assert_eq!(state.buffer(), "first edited");
        assert!(state.dirty());
    }

    #[test]
    fn reopening_an_already_open_path_focuses_it_instead_of_rereading_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "original").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();
        *state.buffer_mut() = "unsaved edit".to_string();
        state.mark_dirty();

        state.open(&path).unwrap();

        assert_eq!(state.tab_count(), 1);
        assert_eq!(state.buffer(), "unsaved edit");
    }

    #[test]
    fn save_writes_the_active_tabs_buffer_to_disk_and_clears_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "original").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();
        *state.buffer_mut() = "edited content".to_string();
        state.mark_dirty();
        state.save().unwrap();

        assert!(!state.dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "edited content");
    }

    #[test]
    fn save_with_nothing_open_is_a_no_op() {
        let mut state = EditorState::default();
        assert!(state.save().is_ok());
    }

    #[test]
    fn close_tab_saves_a_dirty_tab_first_then_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "original").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();
        *state.buffer_mut() = "edited content".to_string();
        state.mark_dirty();

        state.close().unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "edited content");
        assert_eq!(state.tab_count(), 0);
        assert_eq!(state.open_path(), None);
    }

    #[test]
    fn closing_the_active_tab_reactivates_a_neighbor() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        let third = dir.path().join("third.md");
        fs::write(&first, "a").unwrap();
        fs::write(&second, "b").unwrap();
        fs::write(&third, "c").unwrap();

        let mut state = EditorState::default();
        state.open(&first).unwrap();
        state.open(&second).unwrap();
        state.open(&third).unwrap();
        // Active is now `third` (index 2); close it and expect `second`
        // (index 1) to remain active rather than `active` dangling.
        state.close().unwrap();

        assert_eq!(state.tab_count(), 2);
        assert_eq!(state.open_path(), Some(second.as_path()));
    }

    #[test]
    fn close_with_nothing_open_is_a_no_op() {
        let mut state = EditorState::default();
        assert!(state.close().is_ok());
        assert_eq!(state.tab_count(), 0);
    }

    #[test]
    fn close_all_saves_every_dirty_tab() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        fs::write(&first, "a").unwrap();
        fs::write(&second, "b").unwrap();

        let mut state = EditorState::default();
        state.open(&first).unwrap();
        *state.buffer_mut() = "a edited".to_string();
        state.mark_dirty();
        state.open(&second).unwrap();
        *state.buffer_mut() = "b edited".to_string();
        state.mark_dirty();

        state
            .close_all_with_store(&crate::project::store::NativeStore)
            .unwrap();

        assert_eq!(fs::read_to_string(&first).unwrap(), "a edited");
        assert_eq!(fs::read_to_string(&second).unwrap(), "b edited");
        assert_eq!(state.tab_count(), 0);
    }

    #[test]
    fn close_other_tabs_saves_the_others_and_keeps_only_the_given_one() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        let third = dir.path().join("third.md");
        fs::write(&first, "a").unwrap();
        fs::write(&second, "b").unwrap();
        fs::write(&third, "c").unwrap();

        let mut state = EditorState::default();
        state.open(&first).unwrap();
        *state.buffer_mut() = "a edited".to_string();
        state.mark_dirty();
        state.open(&second).unwrap(); // the one to keep
        state.open(&third).unwrap();

        state
            .close_other_tabs_with_store(1, &crate::project::store::NativeStore)
            .unwrap();

        assert_eq!(fs::read_to_string(&first).unwrap(), "a edited");
        assert_eq!(state.tab_count(), 1);
        assert_eq!(state.open_path(), Some(second.as_path()));
    }

    #[test]
    fn close_other_tabs_with_an_out_of_range_index_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "content").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();

        state
            .close_other_tabs_with_store(5, &crate::project::store::NativeStore)
            .unwrap();

        assert_eq!(state.tab_count(), 1);
    }

    #[test]
    fn mark_dirty_records_the_active_tabs_path_as_modified() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "original").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();
        state.mark_dirty();

        assert!(state.modified_paths.contains(&path));
    }

    #[test]
    fn modified_paths_persists_across_tabs_closing() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        fs::write(&first, "original").unwrap();

        let mut state = EditorState::default();
        state.open(&first).unwrap();
        state.mark_dirty();
        state.save().unwrap();
        state.close().unwrap();

        assert!(
            state.modified_paths.contains(&first),
            "closing a tab shouldn't forget that it was edited this session"
        );
    }

    #[test]
    fn opening_a_nonexistent_file_returns_err_and_opens_no_tab() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.md");

        let mut state = EditorState::default();
        let result = state.open(&missing);

        assert!(result.is_err());
        assert_eq!(state.tab_count(), 0);
    }

    #[test]
    fn changed_on_disk_is_false_right_after_open_or_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "original").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();
        assert!(!state.changed_on_disk());

        *state.buffer_mut() = "edited".to_string();
        state.mark_dirty();
        state.save().unwrap();
        assert!(!state.changed_on_disk());
    }

    #[test]
    fn changed_on_disk_is_true_after_an_external_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "original").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&path, "changed elsewhere").unwrap();

        assert!(state.changed_on_disk());
    }

    #[test]
    fn reload_from_disk_replaces_the_active_buffer_and_clears_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "original").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();
        *state.buffer_mut() = "unsaved local edit".to_string();
        state.mark_dirty();

        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&path, "changed elsewhere").unwrap();
        state.reload_from_disk().unwrap();

        assert_eq!(state.buffer(), "changed elsewhere");
        assert!(!state.dirty());
        assert!(!state.changed_on_disk());
    }

    #[test]
    fn acknowledge_disk_change_updates_the_mtime_without_touching_the_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.md");
        fs::write(&path, "original").unwrap();

        let mut state = EditorState::default();
        state.open(&path).unwrap();
        *state.buffer_mut() = "unsaved local edit".to_string();
        state.mark_dirty();

        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&path, "changed elsewhere").unwrap();
        assert!(state.changed_on_disk());

        state.acknowledge_disk_change();

        assert_eq!(state.buffer(), "unsaved local edit");
        assert!(state.dirty());
        assert!(!state.changed_on_disk());
    }

    #[test]
    fn go_back_focuses_an_already_open_tab_rather_than_duplicating_it() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        fs::write(&first, "a").unwrap();
        fs::write(&second, "b").unwrap();

        let mut state = EditorState::default();
        state.open(&first).unwrap();
        state.open(&second).unwrap(); // a new tab, history inherited from `first`

        assert!(state.can_go_back());
        state.go_back(&crate::project::store::NativeStore).unwrap();

        assert_eq!(state.tab_count(), 2);
        assert_eq!(state.open_path(), Some(first.as_path()));
    }

    #[test]
    fn go_back_reopens_a_closed_tab_fresh_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        fs::write(&first, "a").unwrap();
        fs::write(&second, "b").unwrap();

        let mut state = EditorState::default();
        state.open(&first).unwrap();
        state.open(&second).unwrap();
        // Close `first`'s tab (index 0); `second` (now index 0) stays active.
        state.close_tab(0, &crate::project::store::NativeStore).unwrap();
        assert_eq!(state.tab_count(), 1);

        state.go_back(&crate::project::store::NativeStore).unwrap();

        assert_eq!(state.tab_count(), 2);
        assert_eq!(state.open_path(), Some(first.as_path()));
        assert_eq!(state.buffer(), "a");
    }

    #[test]
    fn opening_a_tab_leaves_the_origin_tabs_own_history_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        fs::write(&first, "a").unwrap();
        fs::write(&second, "b").unwrap();

        let mut state = EditorState::default();
        state.open(&first).unwrap();
        state.open(&second).unwrap();
        state.open(&first).unwrap(); // focus back to `first`

        // `first`'s own history was never extended by `second` being opened
        // from it, so there's nothing forward of it to go to.
        assert!(!state.can_go_forward());
    }

    #[test]
    fn open_collab_tab_opens_a_pathless_tab() {
        let mut state = EditorState::default();
        state.open_collab_tab();

        assert_eq!(state.tab_count(), 1);
        assert_eq!(state.open_path(), None);
        assert!(!state.dirty());
    }

    #[test]
    fn rename_path_everywhere_updates_the_path_of_a_matching_tab() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.md");
        let new = dir.path().join("new.md");
        fs::write(&old, "content").unwrap();

        let mut state = EditorState::default();
        state.open(&old).unwrap();
        fs::rename(&old, &new).unwrap();
        state.rename_path_everywhere(&old, &new);

        assert_eq!(state.open_path(), Some(new.as_path()));
        assert_eq!(state.buffer(), "content");
    }

    #[test]
    fn rename_path_everywhere_updates_a_non_active_tabs_history_too() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.md");
        let new_path = dir.path().join("new.md");
        let second = dir.path().join("second.md");
        fs::write(&old, "a").unwrap();
        fs::write(&second, "b").unwrap();

        let mut state = EditorState::default();
        state.open(&old).unwrap();
        state.open(&second).unwrap(); // `second`'s history inherits a trail through `old`
        fs::rename(&old, &new_path).unwrap();
        state.rename_path_everywhere(&old, &new_path);

        // `second` is still active; going back should follow its inherited
        // history to `old`'s *renamed* path, not the stale pre-rename one.
        state.go_back(&crate::project::store::NativeStore).unwrap();

        assert_eq!(state.open_path(), Some(new_path.as_path()));
    }
}
