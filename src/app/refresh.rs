use super::*;
use std::collections::HashMap;

/// Which form the Metadata dock currently shows — the open document's own
/// frontmatter (the default), the project-wide fields (`BinderEvent::SelectProject`,
/// the binder's root row), or a specific folder's own metadata
/// (`BinderEvent::SelectFolder`, any other folder row). Replaces what used to
/// be a bare `project_selected: bool`, now that a folder is a third possible
/// target alongside "the project" and "no folder at all."
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) enum MetadataTarget {
    #[default]
    Document,
    Project,
    Folder(PathBuf),
}

/// Live editing buffers for the open document's frontmatter, and the bookkeeping
/// needed to keep them in sync with whichever document is open — grouped out of
/// `SmaragdApp` (2026-07-31 code-quality review) since all three always change
/// together, driven by `refresh_metadata_if_needed`/`apply_metadata_edits_if_changed`.
#[derive(Default)]
pub(super) struct MetadataState {
    /// There's no "closed" state to represent here, since the Metadata dock
    /// tab's own presence in `dock_state` is what tracks visibility.
    pub(super) draft: MetadataDraft,
    /// Which document `draft`/`last_applied` were last computed for, so a later
    /// frame can tell whether `editor.open_path` has since changed.
    pub(super) computed_for: Option<PathBuf>,
    /// The `DocumentMeta` last written into `editor.buffer` — compared against
    /// `draft.to_meta()` each frame to notice a live edit without re-writing the
    /// buffer when nothing changed.
    pub(super) last_applied: DocumentMeta,
    /// Which form the Metadata dock shows right now — see `MetadataTarget`.
    pub(super) target: MetadataTarget,
    /// `MetadataDraft` for whichever folder `target` currently names — the
    /// same buffer-diff mechanism as `draft`/`last_applied` above, but kept
    /// as its own pair rather than reused: a folder's and the open
    /// document's in-progress edits can coexist (switching the Metadata
    /// dock's target doesn't "commit" whichever form you're leaving).
    pub(super) folder_draft: MetadataDraft,
    /// The `DocumentMeta` last written to `Project::folder_meta` for
    /// `target`'s folder — compared against `folder_draft.to_meta()` each
    /// frame, mirroring `last_applied`.
    pub(super) folder_last_applied: DocumentMeta,
    /// Which folder path `folder_draft`/`folder_last_applied` were last
    /// computed for, mirroring `computed_for`. `None` until a folder's ever
    /// been selected.
    pub(super) folder_computed_for: Option<PathBuf>,
}

/// Lazily-populated, disk-backed cache of each *closed* document's frontmatter
/// `status`, keyed by absolute path — avoids a per-frame disk read for every
/// visible binder row (the binder re-renders every frame; see
/// `WordCountState`'s doc comment above for why this codebase avoids that
/// class of I/O elsewhere too). The currently *open* document's status is
/// read live from `metadata.draft.status` instead (already in memory, and may
/// include unsaved edits) — this cache is never consulted for it, only for
/// every other document a binder row might show. `RefCell` because it's
/// populated from inside `binder_panel::show`, which only gets `&Project` and
/// plain closures, not `&mut SmaragdApp`.
#[derive(Default)]
pub(super) struct DocumentStatusCache {
    cache: std::cell::RefCell<HashMap<PathBuf, Option<String>>>,
}

impl DocumentStatusCache {
    /// `path`'s cached status, reading it from disk (`Project::document_meta`)
    /// only the first time `path` is looked up since the last invalidation.
    pub(super) fn status(&self, project: &Project, path: &Path) -> Option<String> {
        if let Some(cached) = self.cache.borrow().get(path) {
            return cached.clone();
        }
        let status = project.document_meta(path).ok().and_then(|m| m.status);
        self.cache
            .borrow_mut()
            .insert(path.to_path_buf(), status.clone());
        status
    }

    /// Drop `path`'s cached entry so the next `status()` call re-reads it
    /// from disk — call whenever `path` might have changed since it was last
    /// cached (e.g. it was just autosaved on close).
    pub(super) fn invalidate(&mut self, path: &Path) {
        self.cache.get_mut().remove(path);
    }

    /// Drop every cached entry — call whenever the project tree may have
    /// shifted (a new project opened, or any rename/move/delete/restore).
    /// Simpler and safer than surgically rewriting keys the way
    /// `Project::rewrite_relative_key_prefix` does for *persisted*
    /// `ProjectMeta` maps: this cache is a purely in-memory, ephemeral perf
    /// optimization, so a full clear costs at most one disk re-read per
    /// subsequently-visible row, not a project-wide walk.
    pub(super) fn clear(&mut self) {
        self.cache.get_mut().clear();
    }
}

/// Every `[[wikilink]]` elsewhere in the project pointing at the open document,
/// and which document it was computed for — grouped out of `SmaragdApp`
/// (2026-07-31 code-quality review) since both always change together, driven
/// by `refresh_backlinks_if_needed`.
#[derive(Default)]
pub(super) struct BacklinksState {
    pub(super) entries: Vec<BacklinkEntry>,
    /// Which document `entries` was last computed for.
    pub(super) computed_for: Option<PathBuf>,
}

/// The open document's tags, the project-wide tag search box, and the
/// bookkeeping needed to keep both in sync — grouped out of `SmaragdApp`
/// (2026-07-31 code-quality review) since all five always change together,
/// driven by `refresh_tags_if_needed`/`refresh_tag_search_if_needed`.
#[derive(Default)]
pub(super) struct TagsState {
    /// The open document's tags (frontmatter `tags:` merged with inline `#tag`
    /// mentions), each paired with the other project documents sharing it.
    pub(super) entries: Vec<crate::project::TagGroup>,
    /// Which document `entries` was last computed for.
    pub(super) computed_for: Option<PathBuf>,
    /// Live text of the Tags dock's search box — typing here (or clicking one
    /// of `entries`' tag headings, which fills it in) requests a vault-wide
    /// lookup of every document carrying that tag.
    pub(super) search_text: String,
    /// Vault-wide search results for `search_text` (see
    /// `Project::documents_with_tag`), recomputed only when it changes.
    pub(super) search_results: Vec<(PathBuf, String)>,
    /// Which `search_text` value `search_results` was last computed for.
    pub(super) search_computed_for: String,
}

/// The open project's word count, its background-recompute machinery, and the
/// characters-typed activity counter — grouped out of `SmaragdApp` (2026-07-31
/// code-quality review) since all six always change together, driven by
/// `spawn_word_count_recompute`/`track_char_activity`.
#[derive(Default)]
pub(super) struct WordCountState {
    /// The open project's word count under `ProjectMeta::word_count_scope`,
    /// shown in the Word Count dock tab and the status bar. Recomputed only on
    /// a handful of triggers (see `spawn_word_count_recompute`'s callers) via a
    /// background thread, never every frame — unlike `metadata_panel`'s live
    /// per-document count from the open buffer, which is unrelated to this.
    pub(super) cache: usize,
    /// A word-count recompute currently running on a background thread, if any —
    /// walking every tracked document's content from disk could be slow for a
    /// large project, so (like `pending_git`'s push/pull) this never runs
    /// synchronously on the UI thread. `None` once `poll_word_count` has picked
    /// up its result.
    pub(super) pending: Option<std::sync::mpsc::Receiver<usize>>,
    /// `editor.dirty` as of the previous frame — compared against its current
    /// value in `refresh_word_count_if_needed` to edge-detect "a save just
    /// happened" (dirty went `true` -> `false`), the moment any of the three
    /// existing save paths (explicit save, focus-loss autosave, closing the
    /// document) commits bytes to disk, without needing a call at each site.
    pub(super) last_dirty: bool,
    /// Characters typed *and* deleted this session, in tracked documents only
    /// (see `Project::is_path_tracked`) — an activity counter, not a net delta:
    /// typing 100 characters then deleting them all reads 200, not 0. Purely
    /// informational (no target), kept only in memory — not persisted to
    /// `project.json` — and reset when a project is opened (see `set_project`)
    /// or the panel's "Reset Session" is clicked (see `handle_word_count_event`).
    /// Updated by `track_char_activity`, called after the dock renders each frame.
    pub(super) char_activity: u64,
    /// The open document's buffer length (in `chars()`), as of the last frame
    /// `track_char_activity` ran — compared against the current frame's length
    /// to find how many characters just changed. `None` right after opening or
    /// switching documents, so the jump between two different documents' lengths
    /// is never miscounted as characters typed.
    pub(super) char_activity_last_len: Option<usize>,
    /// Which document `char_activity_last_len` was captured for — lets
    /// `track_char_activity` notice a document switch (vs. an edit to the same
    /// document) and reset the baseline instead of diffing across two unrelated
    /// buffers.
    pub(super) char_activity_tracked_path: Option<PathBuf>,
}

impl SmaragdApp {
    /// Refresh `metadata.draft` from the open document's current frontmatter
    /// (parsed from the live buffer, not necessarily what's on disk yet, so it
    /// reflects any unsaved edits to the block itself) whenever the open document
    /// has changed since the last computation — a no-op most frames. Called before
    /// the dock renders each frame, alongside `refresh_backlinks_if_needed`.
    pub(super) fn refresh_metadata_if_needed(&mut self) {
        if self.editor.open_path == self.metadata.computed_for {
            return;
        }
        let meta = match &self.editor.open_path {
            Some(_) => crate::frontmatter::parse(&self.editor.buffer),
            None => DocumentMeta::default(),
        };
        self.metadata.draft = MetadataDraft::from_meta(&meta);
        self.metadata.last_applied = meta;
        self.metadata.computed_for = self.editor.open_path.clone();
        // Only set — never clear — the status message here: most document switches
        // have nothing wrong with their frontmatter, and blanking whatever the
        // status bar was already showing (e.g. a just-completed git operation) on
        // every single switch would be far noisier than useful.
        if self.editor.open_path.is_some()
            && let Some(err) = crate::frontmatter::validate(&self.editor.buffer)
        {
            self.push_error_toast(err.to_string());
        }
    }

    /// Notice a live edit to `metadata.draft` (typed into the Metadata dock tab this
    /// frame, if it was visible) and, if anything actually changed, rewrite the
    /// editor buffer's frontmatter block in place (preserving any keys the form
    /// doesn't expose — see `frontmatter::write_back`) and mark it dirty, same as
    /// any other in-buffer edit; the existing save path (explicit Save, autosave on
    /// focus loss, etc.) takes it from there. Called after the dock renders each
    /// frame. A safe no-op when no document is open or nothing changed — the draft
    /// can only be mutated by the user typing into a visible Metadata tab.
    pub(super) fn apply_metadata_edits_if_changed(&mut self) {
        if self.editor.open_path.is_none() {
            return;
        }
        let current = self.metadata.draft.to_meta();
        if current == self.metadata.last_applied {
            return;
        }
        self.editor.buffer = crate::frontmatter::write_back(&self.editor.buffer, &current);
        self.editor.mark_dirty();
        self.metadata.last_applied = current;
    }

    /// `refresh_metadata_if_needed`'s equivalent for `MetadataTarget::Folder`:
    /// refresh `metadata.folder_draft` from `Project::folder_meta` whenever
    /// `target` names a folder `folder_computed_for` doesn't match yet.
    /// Unlike a document's frontmatter, this needs no disk read or YAML
    /// validation — `folder_meta` is already plain in-memory `ProjectMeta`.
    pub(super) fn refresh_folder_metadata_if_needed(&mut self) {
        let MetadataTarget::Folder(path) = self.metadata.target.clone() else {
            return;
        };
        if self.metadata.folder_computed_for.as_deref() == Some(path.as_path()) {
            return;
        }
        let meta = self
            .project
            .as_ref()
            .map(|project| project.folder_meta(&path))
            .unwrap_or_default();
        self.metadata.folder_draft = MetadataDraft::from_meta(&meta);
        self.metadata.folder_last_applied = meta;
        self.metadata.folder_computed_for = Some(path);
    }

    /// `apply_metadata_edits_if_changed`'s equivalent for
    /// `MetadataTarget::Folder`: persist a live edit to `metadata.folder_draft`
    /// via `Project::set_folder_meta` instead of rewriting `editor.buffer`.
    pub(super) fn apply_folder_metadata_edits_if_changed(&mut self) {
        let MetadataTarget::Folder(path) = self.metadata.target.clone() else {
            return;
        };
        let current = self.metadata.folder_draft.to_meta();
        if current == self.metadata.folder_last_applied {
            return;
        }
        if let Some(project) = &mut self.project
            && let Err(err) = project.set_folder_meta(&path, current.clone())
        {
            self.push_error_toast(format!("Couldn't save folder metadata: {err}"));
        }
        self.metadata.folder_last_applied = current;
    }

    /// Refresh `backlinks` from the project whenever the open document has changed
    /// since the last scan — a no-op most frames. Called before the dock renders
    /// each frame; recomputing regardless of whether the Backlinks tab happens to
    /// be visible right now is simplest, since the scan itself is cheap (see
    /// `Project::backlinks`).
    pub(super) fn refresh_backlinks_if_needed(&mut self) {
        if self.editor.open_path == self.backlinks.computed_for {
            return;
        }
        self.recompute_backlinks();
    }

    pub(super) fn recompute_backlinks(&mut self) {
        self.backlinks.entries = match (&self.project, &self.editor.open_path) {
            (Some(project), Some(path)) => project.backlinks(path),
            _ => Vec::new(),
        };
        self.backlinks.computed_for = self.editor.open_path.clone();
    }

    /// Refresh `tags` from the project whenever the open document has changed
    /// since the last scan — a no-op most frames. Called before the dock
    /// renders each frame, alongside `refresh_backlinks_if_needed`.
    pub(super) fn refresh_tags_if_needed(&mut self) {
        if self.editor.open_path == self.tags.computed_for {
            return;
        }
        self.recompute_tags();
    }

    pub(super) fn recompute_tags(&mut self) {
        self.tags.entries = match (&self.project, &self.editor.open_path) {
            (Some(project), Some(path)) => project.related_by_tag(path),
            _ => Vec::new(),
        };
        self.tags.computed_for = self.editor.open_path.clone();
    }

    /// Refresh `tags.search_results` whenever `tags.search_text` has changed
    /// since the last scan — a no-op most frames. Called *after* the dock
    /// renders each frame (unlike `refresh_tags_if_needed`), since the search
    /// box is a live text field the user may have just edited that same
    /// frame — same reasoning as why `apply_metadata_edits_if_changed` runs
    /// after rendering rather than before.
    pub(super) fn refresh_tag_search_if_needed(&mut self) {
        if self.tags.search_text == self.tags.search_computed_for {
            return;
        }
        let query = self.tags.search_text.trim();
        self.tags.search_results = match (&self.project, query.is_empty()) {
            (Some(project), false) => project.documents_with_tag(query),
            _ => Vec::new(),
        };
        self.tags.search_computed_for = self.tags.search_text.clone();
    }

    /// Recompute `word_count.cache` whenever a save just completed — edge-detects
    /// `editor.dirty` going `true` -> `false` this frame, the moment any of the
    /// three existing save paths (explicit `Ctrl+S`, focus-loss autosave inside
    /// `editor_panel::show`, or `close_document`) commits bytes to disk, without
    /// needing a call at each of those sites. Unlike `refresh_backlinks_if_needed`/
    /// `refresh_tags_if_needed` (keyed on which document is open), word count
    /// doesn't depend on the open document at all, so a dirty-edge check is the
    /// right trigger here instead. A no-op most frames.
    pub(super) fn refresh_word_count_if_needed(&mut self, ctx: &egui::Context) {
        let just_saved = self.word_count.last_dirty && !self.editor.dirty;
        self.word_count.last_dirty = self.editor.dirty;
        if just_saved {
            self.spawn_word_count_recompute(ctx);
        }
    }

    /// Update `word_count.char_activity` from however much the open document's
    /// buffer length changed since the last frame — both growing (typing) and
    /// shrinking (deleting) count toward the total, so typing 100 characters
    /// then deleting them all adds 200, not 0 (see that field's doc
    /// comment). Cheap (no disk I/O, no full-project walk — see
    /// `Project::is_path_tracked`), so unlike `word_count.cache` this runs every
    /// frame. Called after the dock renders each frame, alongside
    /// `apply_metadata_edits_if_changed`.
    pub(super) fn track_char_activity(&mut self) {
        let Some(open_path) = self.editor.open_path.clone() else {
            self.word_count.char_activity_last_len = None;
            self.word_count.char_activity_tracked_path = None;
            return;
        };
        if self.word_count.char_activity_tracked_path.as_deref() != Some(open_path.as_path()) {
            // Just opened or switched to this document — the previous frame's
            // length (if any) belonged to a different buffer, so there's
            // nothing meaningful to diff against yet.
            self.word_count.char_activity_tracked_path = Some(open_path.clone());
            self.word_count.char_activity_last_len = Some(self.editor.buffer.chars().count());
            return;
        }
        let current_len = self.editor.buffer.chars().count();
        if let Some(previous_len) = self.word_count.char_activity_last_len {
            let is_tracked = self.project.as_ref().is_some_and(|project| {
                project.is_path_tracked(&open_path, project.meta.word_count_scope)
            });
            if is_tracked {
                self.word_count.char_activity += current_len.abs_diff(previous_len) as u64;
            }
        }
        self.word_count.char_activity_last_len = Some(current_len);
    }

    /// Kick off a word-count recompute on a background thread for the current
    /// project (a no-op with none open) — walking every tracked document's
    /// content from disk could be slow for a large project, so (like `git push`/
    /// `pull`, see `spawn_git_operation`) this never runs synchronously on the UI
    /// thread. Refuses to start a second computation while one's already in
    /// flight rather than queuing or racing it. The spawned thread requests a
    /// repaint once it has a result, so `poll_word_count` (called every frame)
    /// picks it up promptly.
    pub(super) fn spawn_word_count_recompute(&mut self, ctx: &egui::Context) {
        let Some(project) = &self.project else {
            self.word_count.cache = 0;
            return;
        };
        if self.word_count.pending.is_some() {
            return;
        }
        // Clone just the data the walk needs rather than requiring `Project`
        // itself to be `Send` — `root`/`tree`/`meta` are all plain, cloneable
        // data (see their derives in `project/mod.rs`).
        let root = project.root.clone();
        let tree = project.tree.clone();
        let meta = project.meta.clone();
        let scope = meta.word_count_scope;
        let (sender, receiver) = std::sync::mpsc::channel();
        let repaint_ctx = ctx.clone();
        std::thread::spawn(move || {
            let snapshot = crate::project::Project { root, tree, meta };
            let total = snapshot.word_count(scope);
            let _ = sender.send(total);
            repaint_ctx.request_repaint();
        });
        self.word_count.pending = Some(receiver);
    }

    /// Check whether an in-flight `word_count.pending` recompute has finished,
    /// and if so apply it to `word_count.cache` and roll the Session Target's
    /// baseline forward if the calendar day has changed (see
    /// `Project::maybe_roll_over_session`). Called every frame, mirroring
    /// `poll_git_operation`; a no-op whenever nothing is pending or the
    /// background thread hasn't sent its result yet.
    pub(super) fn poll_word_count(&mut self) {
        let Some(receiver) = &self.word_count.pending else {
            return;
        };
        let total = match receiver.try_recv() {
            Ok(total) => total,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.word_count.pending = None;
                return;
            }
        };
        self.word_count.pending = None;
        self.word_count.cache = total;
        if let Some(project) = &mut self.project {
            let _ = project.maybe_roll_over_session(total);
        }
    }
}

#[cfg(test)]
mod char_activity_tests {
    use super::*;

    #[test]
    fn no_project_and_no_open_document_never_panics_or_accumulates() {
        let mut app = SmaragdApp::test_fixture();

        app.track_char_activity();
        app.track_char_activity();

        assert_eq!(app.word_count.char_activity, 0);
    }

    #[test]
    fn typing_then_deleting_counts_both_directions_not_the_net_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Scene").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open_path = Some(path);

        // First frame with this document open: only establishes the baseline,
        // the initial (empty) length isn't itself counted as "typed."
        app.track_char_activity();
        assert_eq!(app.word_count.char_activity, 0);

        // "Type" 100 characters.
        app.editor.buffer = "a".repeat(100);
        app.track_char_activity();
        assert_eq!(app.word_count.char_activity, 100);

        // "Delete" them all back to empty — the example from the bug report:
        // 100 typed + 100 deleted reads 200, not a net 0.
        app.editor.buffer.clear();
        app.track_char_activity();
        assert_eq!(app.word_count.char_activity, 200);
    }

    #[test]
    fn switching_documents_does_not_count_the_length_jump_between_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let first = project.create_document(dir.path(), "First").unwrap();
        let second = project.create_document(dir.path(), "Second").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);

        app.editor.open_path = Some(first);
        app.track_char_activity(); // establishes the baseline (empty) length
        app.editor.buffer = "a".repeat(50);
        app.track_char_activity();
        assert_eq!(app.word_count.char_activity, 50);

        // Switch to a different, much longer document.
        app.editor.open_path = Some(second);
        app.editor.buffer = "b".repeat(500);
        app.track_char_activity();

        // The 450-character jump between the two unrelated buffers must not
        // be counted as characters typed.
        assert_eq!(app.word_count.char_activity, 50);
    }

    #[test]
    fn editing_a_document_outside_the_tracked_scope_does_not_count() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let manuscript = project.create_folder(dir.path(), "Manuscript").unwrap();
        project
            .set_folder_role(&manuscript, Some(crate::project::FolderRole::Manuscript))
            .unwrap();
        // ManuscriptOnly is the default scope, and this document lives
        // outside the one Manuscript-role folder.
        let outtake = project.create_document(dir.path(), "Outtake").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open_path = Some(outtake);

        app.track_char_activity();
        app.editor.buffer = "a".repeat(100);
        app.track_char_activity();

        assert_eq!(app.word_count.char_activity, 0);
    }
}

#[cfg(test)]
mod word_count_refresh_tests {
    use super::*;

    #[test]
    fn a_save_edge_triggers_a_recompute() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let ctx = egui::Context::default();

        app.editor.dirty = true;
        app.refresh_word_count_if_needed(&ctx);
        assert!(
            app.word_count.pending.is_none(),
            "becoming dirty is not a save"
        );

        app.editor.dirty = false;
        app.refresh_word_count_if_needed(&ctx);
        assert!(
            app.word_count.pending.is_some(),
            "dirty->clean transition is a save and should trigger a recompute"
        );
    }

    #[test]
    fn staying_clean_never_triggers_a_recompute() {
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();

        app.refresh_word_count_if_needed(&ctx);
        app.refresh_word_count_if_needed(&ctx);

        assert!(app.word_count.pending.is_none());
    }
}

#[cfg(test)]
mod document_status_cache_tests {
    use super::*;

    #[test]
    fn status_reads_from_disk_only_once_per_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Scene").unwrap();
        std::fs::write(&path, "---\nstatus: draft\n---\nBody.").unwrap();
        let cache = DocumentStatusCache::default();

        assert_eq!(cache.status(&project, &path), Some("draft".to_string()));

        // Change the file on disk without invalidating — a second lookup
        // should still return the *cached* value, proving it didn't re-read.
        std::fs::write(&path, "---\nstatus: final\n---\nBody.").unwrap();
        assert_eq!(
            cache.status(&project, &path),
            Some("draft".to_string()),
            "a second lookup without invalidation should return the cached value"
        );
    }

    #[test]
    fn invalidate_forces_a_fresh_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Scene").unwrap();
        std::fs::write(&path, "---\nstatus: draft\n---\nBody.").unwrap();
        let mut cache = DocumentStatusCache::default();
        cache.status(&project, &path);

        std::fs::write(&path, "---\nstatus: final\n---\nBody.").unwrap();
        cache.invalidate(&path);

        assert_eq!(cache.status(&project, &path), Some("final".to_string()));
    }

    #[test]
    fn clear_forces_a_fresh_read_for_every_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Scene").unwrap();
        std::fs::write(&path, "---\nstatus: draft\n---\nBody.").unwrap();
        let mut cache = DocumentStatusCache::default();
        cache.status(&project, &path);

        std::fs::write(&path, "---\nstatus: final\n---\nBody.").unwrap();
        cache.clear();

        assert_eq!(cache.status(&project, &path), Some("final".to_string()));
    }
}

#[cfg(test)]
mod folder_metadata_refresh_tests {
    use super::*;

    #[test]
    fn refresh_loads_the_selected_folders_meta() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        project
            .set_folder_meta(
                &chapter,
                DocumentMeta {
                    status: Some("draft".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.metadata.target = MetadataTarget::Folder(chapter);

        app.refresh_folder_metadata_if_needed();

        assert_eq!(app.metadata.folder_draft.status, "draft");
    }

    #[test]
    fn refresh_is_a_no_op_when_the_target_is_not_a_folder() {
        let mut app = SmaragdApp::test_fixture();
        app.metadata.folder_draft.status = "untouched".to_string();

        app.refresh_folder_metadata_if_needed();

        assert_eq!(app.metadata.folder_draft.status, "untouched");
    }

    #[test]
    fn apply_persists_a_live_edit_to_the_targeted_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.metadata.target = MetadataTarget::Folder(chapter.clone());
        app.metadata.folder_draft.status = "draft".to_string();

        app.apply_folder_metadata_edits_if_changed();

        assert_eq!(
            app.project.as_ref().unwrap().folder_meta(&chapter).status,
            Some("draft".to_string())
        );
    }

    #[test]
    fn apply_is_a_no_op_when_nothing_changed() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.metadata.target = MetadataTarget::Folder(chapter);
        // `folder_draft`/`folder_last_applied` both default to an empty
        // `MetadataDraft`/`DocumentMeta` — nothing to persist.

        app.apply_folder_metadata_edits_if_changed();

        assert!(app.project.as_ref().unwrap().meta.folder_meta.is_empty());
    }
}
