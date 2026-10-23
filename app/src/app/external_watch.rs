use super::*;
use std::time::Duration;

use std::time::Instant;

/// How often `check_external_changes` actually does any work — gates both the
/// binder rescan (a directory walk; see `scan::scan_project`) and the open
/// document's mtime check, rather than paying either cost every single frame
/// (egui repaints far more often than that while e.g. the caret is blinking).
/// Short enough that an externally added/changed file shows up promptly;
/// long enough that it's a non-event for CPU/battery.
const EXTERNAL_SCAN_INTERVAL: Duration = Duration::from_secs(2);

/// How long the open document must sit untouched (no typing or deleting)
/// before `check_editor_idle` autosaves it — long enough that a pause
/// mid-sentence doesn't trigger a save, short enough that the Word Count
/// panel's numbers catch up without the user noticing a lag.
pub(super) const EDITOR_IDLE_SAVE_DELAY: Duration = Duration::from_secs(2);

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
        // Catches the Binder's git-dirty marker going stale after a plain
        // edit: the focus-loss autosave in `editor_panel.rs` and the
        // save-before-switching-documents path inside `EditorState::open`
        // both save straight to disk without telling `SmaragdApp` a save
        // happened (see `save_editor`'s doc comment — only the explicit
        // Ctrl+S/`:w` path refreshes `git_dirty_paths` directly), so without
        // this, committing/pushing once would correctly clear a document's
        // dirty marker, but it would then never reappear until something else
        // happened to call `refresh_git_dirty_paths` (a manual save, a git
        // action, or reopening the project). Piggybacking on this same
        // `EXTERNAL_SCAN_INTERVAL` poll picks it up within a couple of
        // seconds regardless of which save path ran — a no-op in CPU terms
        // when git isn't enabled, since `refresh_git_dirty_paths` itself
        // checks that first.
        self.refresh_git_dirty_paths();
    }

    /// Flushes the open document to disk once typing has paused for
    /// `EDITOR_IDLE_SAVE_DELAY`, purely so the Word Count panel's
    /// "Current"/"Today" totals — which only ever update on a save, see
    /// `refresh_word_count_if_needed` — catch up on their own shortly after
    /// the user stops typing, without needing an explicit save or a
    /// "Refresh" click.
    ///
    /// Deliberately **not** a real save: uses
    /// `EditorState::flush_for_word_count`, which writes the buffer and
    /// refreshes `disk_mtime` like a save but leaves `dirty` as it was — see
    /// that method's doc comment for why. In short, `dirty` is what keeps a
    /// document in the sync engine's protected/"held" set
    /// (`SmaragdApp::sync_update_held`); clearing it here would release that
    /// protection on every multi-second pause instead of only at a real save
    /// boundary (explicit Save, focus loss, closing the tab), opening a
    /// window where a remote collaborator's concurrent edit could land on
    /// disk and then be silently overwritten the next time this document is
    /// actually saved. So this calls `spawn_word_count_recompute` and
    /// `Project::invalidate_tag_cache` directly instead of leaning on
    /// `refresh_word_count_if_needed`'s dirty-edge detector, which a flush
    /// that never clears `dirty` would never trip.
    ///
    /// `editor_idle_since` is set by `track_char_activity` on every actual
    /// edit (not just the one that first makes the buffer dirty), so this
    /// fires `EDITOR_IDLE_SAVE_DELAY` after the *last* keystroke, not the
    /// first — a mid-sentence pause doesn't trigger it; and is cleared again
    /// after a successful flush (independently of `dirty`, which stays
    /// true), so the next edit's `track_char_activity` call restarts the
    /// clock for the *next* pause rather than this flushing every frame
    /// while the user is away. Schedules a repaint for exactly when the
    /// delay will next be due, same as `check_external_changes`; falls back
    /// to a short retry when the editor is dirty but `track_char_activity`
    /// hasn't run yet this span (just became dirty this same frame), so the
    /// idle clock is never left waiting on some *other* timer's repaint to
    /// notice it should start.
    pub(super) fn check_editor_idle(&mut self, ctx: &egui::Context) {
        if !self.editor.dirty() {
            return;
        }
        let Some(idle_since) = self.editor_idle_since else {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        };
        let now = Instant::now();
        let elapsed = now.duration_since(idle_since);
        if elapsed < EDITOR_IDLE_SAVE_DELAY {
            ctx.request_repaint_after(EDITOR_IDLE_SAVE_DELAY - elapsed);
            return;
        }
        let store = self.editor_store();
        match self.editor.flush_for_word_count(store.as_ref()) {
            Ok(()) => {
                if let Some(project) = &self.project {
                    project.invalidate_tag_cache();
                }
                self.spawn_word_count_recompute(ctx);
            }
            Err(err) => {
                let path = self
                    .editor
                    .open_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.push_error_toast(format!("Couldn't refresh word count for {path}: {err}"));
            }
        }
        self.editor_idle_since = None;
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
        if self.editor.dirty() {
            if self.external_conflict.is_none() {
                self.external_conflict = self.editor.open_path().map(Path::to_path_buf);
            }
            return;
        }
        let Some(path) = self.editor.open_path().map(Path::to_path_buf) else {
            return;
        };
        let store = self.editor_store();
        match self.editor.reload_from_disk_with_store(store.as_ref()) {
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
            let store = self.editor_store();
            match self.editor.reload_from_disk_with_store(store.as_ref()) {
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

        assert_eq!(app.editor.buffer(), "changed elsewhere");
        assert!(!app.editor.dirty());
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
        *app.editor.buffer_mut() = "my unsaved edit".to_string();
        app.editor.mark_dirty();

        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&doc, "changed elsewhere").unwrap();
        app.scan_for_external_changes();

        assert_eq!(app.editor.buffer(), "my unsaved edit");
        assert!(app.editor.dirty());
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
        *app.editor.buffer_mut() = "my unsaved edit".to_string();
        app.editor.mark_dirty();
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&doc, "changed elsewhere").unwrap();
        app.scan_for_external_changes();
        assert!(app.external_conflict.is_some());

        app.resolve_external_conflict(false);

        assert!(app.external_conflict.is_none());
        assert_eq!(app.editor.buffer(), "my unsaved edit");
        assert!(app.editor.dirty());
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
        *app.editor.buffer_mut() = "my unsaved edit".to_string();
        app.editor.mark_dirty();
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&doc, "changed elsewhere").unwrap();
        app.scan_for_external_changes();

        app.resolve_external_conflict(true);

        assert!(app.external_conflict.is_none());
        assert_eq!(app.editor.buffer(), "changed elsewhere");
        assert!(!app.editor.dirty());
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

    fn init_repo_with_identity(root: &Path) {
        smaragd_core::git::init(root).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "--local", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "--local", "user.name", "Smaragd Tests"])
            .output()
            .unwrap();
    }

    /// Regression test: after a commit cleared the Binder's git-dirty marker,
    /// a plain edit saved via the focus-loss autosave (not the explicit
    /// Ctrl+S/`:w` path `save_editor` itself refreshes after) used to leave
    /// the marker stuck clean until something unrelated happened to call
    /// `refresh_git_dirty_paths` again. `scan_for_external_changes` now
    /// refreshes it on every poll, so a save via *any* path is picked up
    /// within one `EXTERNAL_SCAN_INTERVAL`.
    #[test]
    fn scan_picks_up_a_file_that_went_dirty_without_an_explicit_save_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        init_repo_with_identity(&project.root);
        project.enable_git_support().unwrap();
        let doc = project.create_document(dir.path(), "Scene").unwrap();
        fs::write(&doc, "original").unwrap();
        smaragd_core::git::commit_all(&project.root, "initial commit").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.scan_for_external_changes();
        assert!(
            app.git_dirty_paths.is_empty(),
            "freshly committed, nothing should be dirty yet"
        );

        // Simulate the focus-loss autosave: a write straight to disk, with no
        // call anywhere to `refresh_git_dirty_paths` — exactly what
        // `EditorState::save_with_store` does on its own.
        fs::write(&doc, "edited, but never explicitly saved via Ctrl+S").unwrap();

        app.scan_for_external_changes();

        assert!(app.git_dirty_paths.contains(&doc));
    }

    #[test]
    fn check_editor_idle_is_a_no_op_on_a_clean_document() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open(&doc).unwrap();
        let ctx = egui::Context::default();

        app.check_editor_idle(&ctx);

        assert!(!app.editor.dirty());
    }

    #[test]
    fn check_editor_idle_does_not_save_before_the_delay_has_elapsed() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open(&doc).unwrap();
        *app.editor.buffer_mut() = "not idle long enough yet".to_string();
        app.editor.mark_dirty();
        app.editor_idle_since = Some(Instant::now());
        let ctx = egui::Context::default();

        app.check_editor_idle(&ctx);

        assert!(app.editor.dirty(), "the idle delay hasn't elapsed yet");
        assert_eq!(fs::read_to_string(&doc).unwrap(), "");
    }

    #[test]
    fn check_editor_idle_flushes_to_disk_once_the_delay_has_elapsed() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open(&doc).unwrap();
        *app.editor.buffer_mut() = "written while idle".to_string();
        app.editor.mark_dirty();
        // Simulate the idle delay having already elapsed, rather than
        // actually sleeping `EDITOR_IDLE_SAVE_DELAY` in the test.
        app.editor_idle_since = Some(Instant::now() - EDITOR_IDLE_SAVE_DELAY);
        let ctx = egui::Context::default();

        app.check_editor_idle(&ctx);

        assert_eq!(fs::read_to_string(&doc).unwrap(), "written while idle");
        // Deliberately still dirty: this is a word-count-only flush, not a
        // real save, so the document must stay in the sync engine's
        // protected/"held" set until an actual save boundary — see
        // `EditorState::flush_for_word_count`'s doc comment.
        assert!(
            app.editor.dirty(),
            "an idle flush must not release sync's hold on this document"
        );
        assert!(
            app.editor_idle_since.is_none(),
            "the clock resets so the next edit starts a fresh countdown"
        );
        assert!(
            app.word_count.pending.is_some(),
            "the flush must trigger a word-count recompute directly, since \
             dirty never clears to drive the usual save-edge detector"
        );
    }

    #[test]
    fn check_editor_idle_waits_for_track_char_activity_to_start_the_clock() {
        // Dirty, but no edit length-change has been observed yet this
        // document's open span (`editor_idle_since` is still `None`) — must
        // not save immediately just because the buffer happens to be dirty.
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open(&doc).unwrap();
        *app.editor.buffer_mut() = "dirty but no idle clock yet".to_string();
        app.editor.mark_dirty();
        assert!(app.editor_idle_since.is_none());
        let ctx = egui::Context::default();

        app.check_editor_idle(&ctx);

        assert!(app.editor.dirty());
    }

    #[test]
    fn track_char_activity_restarts_the_idle_clock_on_every_edit() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open(&doc).unwrap();
        app.track_char_activity(); // establishes the baseline (empty) length

        *app.editor.buffer_mut() = "a".repeat(10);
        app.track_char_activity();
        let first_tick = app.editor_idle_since.expect("an edit must start the clock");

        std::thread::sleep(std::time::Duration::from_millis(10));
        *app.editor.buffer_mut() = "a".repeat(20);
        app.track_char_activity();
        let second_tick = app.editor_idle_since.expect("still edited");

        assert!(
            second_tick > first_tick,
            "a later edit must push the idle clock forward, not leave it at the first edit"
        );
    }
}
