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

/// One closed document's cached frontmatter-derived fields, read together
/// from a single `fs::read_to_string` — `status` (the original reason
/// `DocumentStatusCache` exists), plus `pov` and `word_count`/
/// `word_count_target` (added for `BinderColorMode::Pov`/
/// `WordCountProgress`), and `line_count`/`char_count` (added for
/// `Settings::show_document_stats_in_binder`) — so supporting these costs no
/// additional per-row disk reads over what status-coloring already paid for.
#[derive(Clone, Default)]
struct CachedDocumentRow {
    status: Option<String>,
    pov: Option<String>,
    word_count_target: Option<u32>,
    word_count: usize,
    line_count: usize,
    char_count: usize,
}

/// Lazily-populated, disk-backed cache of each *closed* document's
/// frontmatter-derived fields (see `CachedDocumentRow`), keyed by absolute
/// path — avoids a per-frame disk read for every visible binder row (the
/// binder re-renders every frame; see `WordCountState`'s doc comment above
/// for why this codebase avoids that class of I/O elsewhere too). The
/// currently *open* document's fields are read live instead (from
/// `metadata.draft`/the editor buffer, already in memory and may include
/// unsaved edits) — this cache is never consulted for it, only for every
/// other document a binder row might show. `RefCell` because it's populated
/// from inside `binder_panel::show`, which only gets `&Project` and plain
/// closures, not `&mut SmaragdApp`.
#[derive(Default)]
pub(super) struct DocumentStatusCache {
    cache: std::cell::RefCell<HashMap<PathBuf, CachedDocumentRow>>,
}

impl DocumentStatusCache {
    /// `path`'s cached row, reading it from disk (a single
    /// `fs::read_to_string` + `frontmatter::parse` + `frontmatter::count_words`)
    /// only the first time `path` is looked up since the last invalidation.
    fn row(&self, path: &Path) -> CachedDocumentRow {
        if let Some(cached) = self.cache.borrow().get(path) {
            return cached.clone();
        }
        let contents = std::fs::read_to_string(path).unwrap_or_default();
        let meta = crate::frontmatter::parse(&contents);
        let row = CachedDocumentRow {
            status: meta.status,
            pov: meta.pov,
            word_count_target: meta.word_count_target,
            word_count: crate::frontmatter::count_words(&contents),
            line_count: crate::frontmatter::count_lines(&contents),
            char_count: crate::frontmatter::count_chars(&contents),
        };
        self.cache
            .borrow_mut()
            .insert(path.to_path_buf(), row.clone());
        row
    }

    /// `path`'s cached status — see `row`.
    pub(super) fn status(&self, path: &Path) -> Option<String> {
        self.row(path).status
    }

    /// `path`'s cached POV — see `row`.
    pub(super) fn pov(&self, path: &Path) -> Option<String> {
        self.row(path).pov
    }

    /// `(word_count, word_count_target)` — the pair
    /// `BinderColorMode::WordCountProgress` needs — see `row`.
    pub(super) fn word_count_progress(&self, path: &Path) -> (usize, Option<u32>) {
        let row = self.row(path);
        (row.word_count, row.word_count_target)
    }

    /// `(line_count, word_count, char_count)` — the trailing readout
    /// `Settings::show_document_stats_in_binder` shows on each closed
    /// document's Binder row — see `row`.
    pub(super) fn document_stats(&self, path: &Path) -> (usize, usize, usize) {
        let row = self.row(path);
        (row.line_count, row.word_count, row.char_count)
    }

    /// Drop `path`'s cached entry so the next lookup re-reads it from disk —
    /// call whenever `path` might have changed since it was last cached
    /// (e.g. it was just autosaved on close).
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

/// The two values `spawn_word_count_recompute`'s background thread produces
/// in one pass — see `WordCountState::cache`/`folder_totals`.
pub(super) struct WordCountRecomputeResult {
    pub(super) total: usize,
    pub(super) folder_totals: HashMap<PathBuf, usize>,
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
    /// Every folder's combined descendant word count, as of the last
    /// completed recompute — see `Project::folder_word_counts`. Populated by
    /// the same background thread/trigger as `cache`, for the same reason
    /// `cache` itself is never computed synchronously on the UI thread; feeds
    /// `BinderColorMode::WordCountProgress`'s folder rows.
    pub(super) folder_totals: HashMap<PathBuf, usize>,
    /// A word-count recompute currently running on a background thread, if any —
    /// walking every tracked document's content from disk could be slow for a
    /// large project, so (like `pending_git`'s push/pull) this never runs
    /// synchronously on the UI thread. `None` once `poll_word_count` has picked
    /// up its result.
    pub(super) pending: Option<std::sync::mpsc::Receiver<WordCountRecomputeResult>>,
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

    /// Recompute `word_count.cache`, and drop `Project`'s cached tag index,
    /// whenever a save just completed — edge-detects `editor.dirty` going `true`
    /// -> `false` this frame, the moment any of the three existing save paths
    /// (explicit `Ctrl+S`, focus-loss autosave inside `editor_panel::show`, or
    /// `close_document`) commits bytes to disk, without needing a call at each of
    /// those sites. Unlike `refresh_backlinks_if_needed`/`refresh_tags_if_needed`
    /// (keyed on which document is open), neither word count nor the tag cache
    /// depends on which document is open, so a dirty-edge check is the right
    /// trigger for both instead. A save is the only way a document's *content*
    /// (as opposed to its existence — see `Project::rescan`, which already
    /// invalidates the tag cache itself) can change tags without going through
    /// `Project::rename_tag` (which also invalidates it itself). A no-op most
    /// frames.
    pub(super) fn refresh_word_count_if_needed(&mut self, ctx: &egui::Context) {
        let just_saved = self.word_count.last_dirty && !self.editor.dirty;
        self.word_count.last_dirty = self.editor.dirty;
        if just_saved {
            self.spawn_word_count_recompute(ctx);
            if let Some(project) = &self.project {
                project.invalidate_tag_cache();
            }
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
            let snapshot = crate::project::Project {
                root,
                tree,
                meta,
                tag_cache: Default::default(),
            };
            let total = snapshot.word_count(scope);
            let folder_totals = snapshot.folder_word_counts();
            let _ = sender.send(WordCountRecomputeResult {
                total,
                folder_totals,
            });
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
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.word_count.pending = None;
                return;
            }
        };
        self.word_count.pending = None;
        self.word_count.cache = result.total;
        self.word_count.folder_totals = result.folder_totals;
        if let Some(project) = &mut self.project {
            let _ = project.maybe_roll_over_session(result.total);
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

    /// The same dirty->clean save edge that triggers a word-count recompute also
    /// has to drop `Project`'s memoized tag index (see
    /// `queries::TagCache`/`Project::invalidate_tag_cache`) — a save is the one way
    /// a document's content, and thus its tags, can change without going through
    /// `Project::rescan` or `Project::rename_tag`, neither of which a plain editor
    /// save touches.
    #[test]
    fn a_save_edge_also_invalidates_the_project_s_tag_cache() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();
        fs::write(&doc, "#original").unwrap();
        assert_eq!(project.all_tags(), vec!["original"], "warms the cache");

        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let ctx = egui::Context::default();

        // Simulates what actually happens on disk during a save, without going
        // through `EditorState::save` itself (this test cares about the
        // invalidation trigger, not the save mechanics already covered elsewhere).
        fs::write(&doc, "#changed").unwrap();

        app.editor.dirty = true;
        app.refresh_word_count_if_needed(&ctx);
        assert_eq!(
            app.project.as_ref().unwrap().all_tags(),
            vec!["original"],
            "becoming dirty alone should not invalidate the cache"
        );

        app.editor.dirty = false;
        app.refresh_word_count_if_needed(&ctx);
        assert_eq!(
            app.project.as_ref().unwrap().all_tags(),
            vec!["changed"],
            "the dirty->clean transition should have invalidated the cache"
        );
    }

    #[test]
    fn spawn_word_count_recompute_also_populates_folder_totals() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let root = project.root.clone();
        let doc = project.create_document(dir.path(), "Scene").unwrap();
        fs::write(&doc, "one two three").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let ctx = egui::Context::default();

        app.spawn_word_count_recompute(&ctx);
        // The background thread should finish quickly for a project this
        // small; poll with a bounded retry loop rather than blocking forever.
        for _ in 0..200 {
            app.poll_word_count();
            if app.word_count.pending.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        assert_eq!(app.word_count.cache, 3);
        assert_eq!(app.word_count.folder_totals.get(&root), Some(&3));
    }

    #[test]
    fn poll_word_count_applies_both_the_total_and_folder_totals_together() {
        let mut app = SmaragdApp::test_fixture();
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut folder_totals = HashMap::new();
        folder_totals.insert(PathBuf::from("/project/Chapter 1"), 42);
        sender
            .send(WordCountRecomputeResult {
                total: 100,
                folder_totals: folder_totals.clone(),
            })
            .unwrap();
        app.word_count.pending = Some(receiver);

        app.poll_word_count();

        assert_eq!(app.word_count.cache, 100);
        assert_eq!(app.word_count.folder_totals, folder_totals);
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

        assert_eq!(cache.status(&path), Some("draft".to_string()));

        // Change the file on disk without invalidating — a second lookup
        // should still return the *cached* value, proving it didn't re-read.
        std::fs::write(&path, "---\nstatus: final\n---\nBody.").unwrap();
        assert_eq!(
            cache.status(&path),
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
        cache.status(&path);

        std::fs::write(&path, "---\nstatus: final\n---\nBody.").unwrap();
        cache.invalidate(&path);

        assert_eq!(cache.status(&path), Some("final".to_string()));
    }

    #[test]
    fn clear_forces_a_fresh_read_for_every_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Scene").unwrap();
        std::fs::write(&path, "---\nstatus: draft\n---\nBody.").unwrap();
        let mut cache = DocumentStatusCache::default();
        cache.status(&path);

        std::fs::write(&path, "---\nstatus: final\n---\nBody.").unwrap();
        cache.clear();

        assert_eq!(cache.status(&path), Some("final".to_string()));
    }

    #[test]
    fn pov_and_word_count_are_cached_from_a_single_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Scene").unwrap();
        std::fs::write(
            &path,
            "---\npov: Alice\nword_count_target: 100\n---\none two three",
        )
        .unwrap();
        let cache = DocumentStatusCache::default();

        assert_eq!(cache.pov(&path), Some("Alice".to_string()));
        assert_eq!(cache.word_count_progress(&path), (3, Some(100)));

        // Change the file after the first lookup — both accessors should
        // still return the pre-change cached values, proving they came from
        // one shared cached read rather than two independent ones.
        std::fs::write(&path, "---\npov: Bob\n---\nsomething else entirely").unwrap();
        assert_eq!(cache.pov(&path), Some("Alice".to_string()));
        assert_eq!(cache.word_count_progress(&path), (3, Some(100)));
    }

    #[test]
    fn pov_is_none_for_a_document_with_no_pov_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Scene").unwrap();
        std::fs::write(&path, "Just a plain document.").unwrap();
        let cache = DocumentStatusCache::default();

        assert_eq!(cache.pov(&path), None);
    }

    #[test]
    fn invalidate_drops_the_cached_pov_and_word_count_too() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Scene").unwrap();
        std::fs::write(&path, "---\npov: Alice\n---\none two").unwrap();
        let mut cache = DocumentStatusCache::default();
        cache.pov(&path);

        std::fs::write(&path, "---\npov: Bob\n---\none two three").unwrap();
        cache.invalidate(&path);

        assert_eq!(cache.pov(&path), Some("Bob".to_string()));
        assert_eq!(cache.word_count_progress(&path).0, 3);
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
