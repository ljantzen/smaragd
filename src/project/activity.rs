use super::*;

impl Project {
    /// Begin tracking a new app session against this project — closing
    /// whatever session might already be in progress is the caller's job
    /// (see `SmaragdApp::set_project`, which closes the outgoing project's
    /// session before opening the new one). Unconditionally overwrites any
    /// dangling `current_session_started` left behind by a previous run that
    /// never closed cleanly (a crash, a force-quit) — that stale session is
    /// simply discarded rather than logged, since there's no reliable way to
    /// know when it actually ended.
    pub fn start_session(&mut self) -> io::Result<()> {
        self.meta.current_session_started = Some(crate::dashboard::now_timestamp());
        self.meta.current_session_baseline_words = None;
        self.save_metadata()
    }

    /// Capture `current_total` as the in-progress session's starting word
    /// count, the first time this runs after the session began. Called
    /// alongside `maybe_roll_over_session` from the same word-count-
    /// recompute call sites (`app::refresh`) — the earliest point after
    /// `start_session` a real total becomes available, since opening a
    /// project never synchronously scans every document (see `word_count`'s
    /// own doc comment on why that's backgrounded). A no-op once already
    /// captured, or if no session is in progress.
    pub fn maybe_capture_session_baseline(&mut self, current_total: usize) -> io::Result<()> {
        if self.meta.current_session_started.is_none()
            || self.meta.current_session_baseline_words.is_some()
        {
            return Ok(());
        }
        self.meta.current_session_baseline_words = Some(current_total as u32);
        self.save_metadata()
    }

    /// Close the in-progress session, if any, recording it in `session_log`
    /// and pruning old entries — called both when this project stops being
    /// the active one (switching to a different project) and when the app
    /// shuts down with it still open (see `app::mod`'s `close_requested`
    /// handling, alongside `persist_session`/`persist_dock_layout`).
    /// `current_total` is the last known word count
    /// (`SmaragdApp::word_count.cache`); if no recompute ever captured a
    /// baseline for this session (an especially short one), `current_total`
    /// is used as its own baseline too, logging zero words rather than a
    /// spurious full-project count. A no-op if no session is in progress.
    pub fn close_session(&mut self, current_total: usize) -> io::Result<()> {
        let Some(started) = self.meta.current_session_started.take() else {
            return Ok(());
        };
        let baseline = self
            .meta
            .current_session_baseline_words
            .take()
            .unwrap_or(current_total as u32);
        let words_written = current_total.saturating_sub(baseline as usize) as u32;
        self.meta.session_log.push(crate::dashboard::SessionRecord {
            started,
            ended: crate::dashboard::now_timestamp(),
            words_written,
        });
        crate::dashboard::prune_session_log(
            &mut self.meta.session_log,
            chrono::Local::now().date_naive(),
        );
        self.save_metadata()
    }

    /// Record `path` as created right now — called from `write_new_document`
    /// (`create.rs`), the single choke point every document-creation path
    /// (`create_document`/`create_document_from_template`/
    /// `create_document_with_content`) goes through.
    pub(super) fn record_document_created(&mut self, path: &Path) {
        let key = relative_key(&self.root, path);
        self.meta
            .document_created
            .insert(key, crate::dashboard::now_timestamp());
    }

    /// Rewrite `document_created`'s key for a renamed/moved document or
    /// folder. Unlike `rewrite_relative_key_prefix` (folder-only maps —
    /// `node_order`/`folder_roles`/`trashed_origins`/`folder_meta`),
    /// `document_created` is keyed by document paths too, so this is called
    /// unconditionally from `rename`/`move_node_with`, the same way
    /// `rewrite_bookmark_paths`/`rewrite_note_paths` are.
    pub(super) fn rewrite_document_created_prefix(&mut self, old_prefix: &str, new_prefix: &str) {
        rewrite_prefix_in(&mut self.meta.document_created, old_prefix, new_prefix);
    }

    /// Every tracked document's on-disk modified time — the Dashboard's
    /// "Documents Modified" data source. Deliberately not persisted: unlike
    /// `document_created` (which has no other source of truth), the
    /// filesystem already tracks this, and reading it live also credits a
    /// document touched outside smaragd entirely (an external editor, a
    /// `git pull`), not just edits made through the app. Same raw-`std::fs`-
    /// bypassing-`ProjectStore` precedent `editor::read_mtime` already sets
    /// for mtime specifically. Recomputed fresh from disk every call — can
    /// be slow for a large project, so callers should run it off the UI
    /// thread, same convention as `word_count`.
    pub fn document_modified_times(&self) -> HashMap<PathBuf, std::time::SystemTime> {
        let mut out = HashMap::new();
        self.collect_modified_times(&self.tree.root, &mut out);
        out
    }

    fn collect_modified_times(
        &self,
        node: &BinderNode,
        out: &mut HashMap<PathBuf, std::time::SystemTime>,
    ) {
        match &node.kind {
            BinderNodeKind::Document => {
                if let Ok(metadata) = std::fs::metadata(&node.path)
                    && let Ok(modified) = metadata.modified()
                {
                    out.insert(node.path.clone(), modified);
                }
            }
            BinderNodeKind::Folder { children } => {
                if matches!(
                    self.folder_role(&node.path),
                    Some(FolderRole::Trash) | Some(FolderRole::Templates)
                ) {
                    return;
                }
                for child in children {
                    self.collect_modified_times(child, out);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_session_sets_started_and_clears_any_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project.start_session().unwrap();

        assert!(project.meta.current_session_started.is_some());
        assert_eq!(project.meta.current_session_baseline_words, None);
    }

    #[test]
    fn maybe_capture_session_baseline_only_captures_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.start_session().unwrap();

        project.maybe_capture_session_baseline(100).unwrap();
        project.maybe_capture_session_baseline(999).unwrap();

        assert_eq!(project.meta.current_session_baseline_words, Some(100));
    }

    #[test]
    fn maybe_capture_session_baseline_is_a_noop_with_no_session_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project.maybe_capture_session_baseline(100).unwrap();

        assert_eq!(project.meta.current_session_baseline_words, None);
    }

    #[test]
    fn close_session_logs_words_written_since_the_captured_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.start_session().unwrap();
        project.maybe_capture_session_baseline(100).unwrap();

        project.close_session(450).unwrap();

        assert_eq!(project.meta.session_log.len(), 1);
        assert_eq!(project.meta.session_log[0].words_written, 350);
        assert_eq!(project.meta.current_session_started, None);
        assert_eq!(project.meta.current_session_baseline_words, None);
    }

    #[test]
    fn close_session_with_no_captured_baseline_logs_zero_words() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.start_session().unwrap();

        project.close_session(5000).unwrap();

        assert_eq!(project.meta.session_log[0].words_written, 0);
    }

    #[test]
    fn close_session_is_a_noop_with_no_session_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project.close_session(100).unwrap();

        assert!(project.meta.session_log.is_empty());
    }

    #[test]
    fn close_session_never_logs_negative_words_when_the_total_drops() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.start_session().unwrap();
        project.maybe_capture_session_baseline(500).unwrap();

        project.close_session(200).unwrap();

        assert_eq!(project.meta.session_log[0].words_written, 0);
    }

    #[test]
    fn start_session_discards_a_dangling_session_from_an_unclean_previous_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.meta.current_session_started = Some("2000-01-01T00:00:00".to_string());
        project.meta.current_session_baseline_words = Some(10);
        project.save_metadata().unwrap();

        project.start_session().unwrap();
        project.close_session(50).unwrap();

        // Only the fresh session was logged — the dangling one from
        // "2000-01-01" was silently discarded, not resurrected.
        assert_eq!(project.meta.session_log.len(), 1);
        assert_ne!(project.meta.session_log[0].started, "2000-01-01T00:00:00");
    }

    #[test]
    fn record_document_created_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        let path = project.create_document(dir.path(), "Scene 1").unwrap();

        assert!(project.meta.document_created.contains_key("Scene 1.md"));
        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert!(reloaded.meta.document_created.contains_key("Scene 1.md"));
        let _ = path;
    }

    #[test]
    fn document_created_follows_a_rename() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.create_document(dir.path(), "Old Name").unwrap();

        project
            .rename(&dir.path().join("Old Name.md"), "New Name")
            .unwrap();

        assert!(!project.meta.document_created.contains_key("Old Name.md"));
        assert!(project.meta.document_created.contains_key("New Name.md"));
    }

    #[test]
    fn document_created_follows_a_move_into_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();
        project.create_document(dir.path(), "Scene 1").unwrap();

        project
            .move_item(&dir.path().join("Scene 1.md"), &folder)
            .unwrap();

        assert!(!project.meta.document_created.contains_key("Scene 1.md"));
        assert!(
            project
                .meta
                .document_created
                .contains_key("Chapter 1/Scene 1.md")
        );
    }

    #[test]
    fn document_created_is_removed_on_permanent_delete() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Doomed").unwrap();

        project.delete(&path).unwrap();

        assert!(!project.meta.document_created.contains_key("Doomed.md"));
    }

    #[test]
    fn document_modified_times_reads_every_tracked_documents_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Scene").unwrap();

        let times = project.document_modified_times();

        assert!(times.contains_key(&path));
    }

    #[test]
    fn document_modified_times_excludes_trash_and_templates() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let trashed = project.create_document(&trash, "Old").unwrap();

        let times = project.document_modified_times();

        assert!(!times.contains_key(&trashed));
    }
}
