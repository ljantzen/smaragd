use super::*;
use std::time::{Duration, Instant};

/// How often `check_external_changes` actually does any work — gates both the
/// binder rescan (a directory walk; see `scan::scan_project`) and the open
/// document's mtime check, rather than paying either cost every single frame
/// (egui repaints far more often than that while e.g. the caret is blinking).
/// Short enough that an externally added/changed file shows up promptly;
/// long enough that it's a non-event for CPU/battery.
const EXTERNAL_SCAN_INTERVAL: Duration = Duration::from_secs(2);

impl SmaragdApp {
    /// Notice files added, removed, or changed outside Smaragd — another program,
    /// a sync tool (Dropbox/Syncthing), a manual `git pull` outside the app's own
    /// Versions menu — and pick them up automatically: rescans the binder tree
    /// (see `Project::rescan`) so added/removed files show up, and reloads the
    /// open document's buffer if its on-disk content moved. A no-op with no
    /// project open. Gated to `EXTERNAL_SCAN_INTERVAL` (see its doc comment) and
    /// schedules the next repaint for when that interval is next due, so
    /// detection keeps happening even while the app is otherwise idle — mirrors
    /// `tick_pomodoro`'s own `request_repaint_after`.
    pub(super) fn check_external_changes(&mut self, ctx: &egui::Context) {
        if self.project.is_none() {
            return;
        }
        ctx.request_repaint_after(EXTERNAL_SCAN_INTERVAL);
        let now = Instant::now();
        if self
            .external_scan_at
            .is_some_and(|at| now.duration_since(at) < EXTERNAL_SCAN_INTERVAL)
        {
            return;
        }
        self.external_scan_at = Some(now);
        self.scan_for_external_changes();
    }

    /// The unconditional (no timer gate) body of `check_external_changes` —
    /// split out so tests can trigger a scan on demand instead of sleeping past
    /// `EXTERNAL_SCAN_INTERVAL`.
    fn scan_for_external_changes(&mut self) {
        if let Some(project) = &mut self.project {
            project.rescan();
            self.document_status_cache.clear();
        }
        self.check_open_document_for_external_change();
    }

    /// If the open document's on-disk content moved since it was last read or
    /// written here: reload it immediately when there's nothing unsaved to
    /// lose, or — if `editor.dirty` — leave the buffer alone and record the
    /// conflict in `external_conflict` for `external_conflict_prompt` to ask
    /// the user about instead of silently clobbering either version. Leaves an
    /// already-recorded conflict alone rather than re-detecting it every scan.
    fn check_open_document_for_external_change(&mut self) {
        if !self.editor.changed_on_disk() {
            return;
        }
        if self.editor.dirty {
            if self.external_conflict.is_none() {
                self.external_conflict = self.editor.open_path.clone();
            }
            return;
        }
        let Some(path) = self.editor.open_path.clone() else {
            return;
        };
        match self.editor.reload_from_disk() {
            Ok(()) => {
                self.document_status_cache.invalidate(&path);
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Document");
                self.set_status_message(format!("Reloaded \"{name}\" — changed on disk"));
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't reload {}: {err}", path.display()));
            }
        }
    }

    /// Resolve `external_conflict` per the user's choice in
    /// `external_conflict_prompt`: `discard_local` reloads the on-disk version
    /// (losing the unsaved edits), otherwise the local buffer is kept and the
    /// on-disk change is just acknowledged so the same write isn't flagged again.
    pub(super) fn resolve_external_conflict(&mut self, discard_local: bool) {
        self.external_conflict = None;
        if discard_local {
            match self.editor.reload_from_disk() {
                Ok(()) => self.set_status_message("Reloaded from disk"),
                Err(err) => self.push_error_toast(format!("Couldn't reload: {err}")),
            }
        } else {
            self.editor.acknowledge_disk_change();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_document_is_silently_reloaded_when_changed_externally() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene").unwrap();
        fs::write(&doc, "original").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open(&doc).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&doc, "changed elsewhere").unwrap();
        app.scan_for_external_changes();

        assert_eq!(app.editor.buffer, "changed elsewhere");
        assert!(!app.editor.dirty);
        assert!(app.external_conflict.is_none());
    }

    #[test]
    fn a_dirty_document_is_not_clobbered_and_raises_a_conflict_instead() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene").unwrap();
        fs::write(&doc, "original").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open(&doc).unwrap();
        app.editor.buffer = "my unsaved edit".to_string();
        app.editor.mark_dirty();

        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&doc, "changed elsewhere").unwrap();
        app.scan_for_external_changes();

        assert_eq!(app.editor.buffer, "my unsaved edit");
        assert!(app.editor.dirty);
        assert_eq!(app.external_conflict.as_deref(), Some(doc.as_path()));
    }

    #[test]
    fn resolving_a_conflict_by_keeping_mine_preserves_the_buffer_and_clears_the_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene").unwrap();
        fs::write(&doc, "original").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open(&doc).unwrap();
        app.editor.buffer = "my unsaved edit".to_string();
        app.editor.mark_dirty();
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&doc, "changed elsewhere").unwrap();
        app.scan_for_external_changes();
        assert!(app.external_conflict.is_some());

        app.resolve_external_conflict(false);

        assert!(app.external_conflict.is_none());
        assert_eq!(app.editor.buffer, "my unsaved edit");
        assert!(app.editor.dirty);
        // The write that triggered the conflict is now acknowledged, so an
        // unrelated later scan shouldn't immediately re-raise it.
        app.scan_for_external_changes();
        assert!(app.external_conflict.is_none());
    }

    #[test]
    fn resolving_a_conflict_by_reloading_discards_the_local_edit() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene").unwrap();
        fs::write(&doc, "original").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open(&doc).unwrap();
        app.editor.buffer = "my unsaved edit".to_string();
        app.editor.mark_dirty();
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&doc, "changed elsewhere").unwrap();
        app.scan_for_external_changes();

        app.resolve_external_conflict(true);

        assert!(app.external_conflict.is_none());
        assert_eq!(app.editor.buffer, "changed elsewhere");
        assert!(!app.editor.dirty);
    }

    #[test]
    fn scan_rescans_the_project_tree_so_an_externally_added_file_shows_up() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);

        fs::write(dir.path().join("New From Outside.md"), "hello").unwrap();
        app.scan_for_external_changes();

        let found = app
            .project
            .as_ref()
            .unwrap()
            .tree
            .document_paths()
            .iter()
            .any(|path| path.file_name().and_then(|n| n.to_str()) == Some("New From Outside.md"));
        assert!(found, "rescan should have picked up the new file");
    }

    #[test]
    fn check_external_changes_is_a_no_op_with_no_project_open() {
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();

        app.check_external_changes(&ctx);

        assert!(app.external_scan_at.is_none());
    }
}
