use super::*;

/// Which folder(s) [`Project::word_count`] sums toward the Word Count panel's
/// Draft Target — see that panel's `ui::word_count_panel`. Both variants exclude
/// Trash and Templates content identically (see [`Project::word_count_from`]);
/// they differ only in which root(s) get walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WordCountScope {
    /// Sum only documents under a `FolderRole::Manuscript` folder. Falls back to
    /// the whole project (still excluding Trash/Templates) if no folder currently
    /// holds that role — the same fallback "Export Manuscript…" uses (see
    /// `export::gather`).
    #[default]
    ManuscriptOnly,
    /// Sum every document in the project except Trash/Templates content.
    EverythingExceptTrash,
}

impl Project {
    /// The total word count `scope` currently includes — recomputed fresh from
    /// disk every call, same convention as [`Project::backlinks`]; an unreadable
    /// document is skipped, not fatal. Can be slow for a large project (reads
    /// every tracked document's content), so callers should run it off the UI
    /// thread — see `app.rs`'s `spawn_word_count_recompute`, which mirrors how
    /// `git push`/`pull` are backgrounded.
    pub fn word_count(&self, scope: WordCountScope) -> usize {
        match scope {
            WordCountScope::ManuscriptOnly => {
                let roots = self.folder_role_paths(FolderRole::Manuscript);
                if roots.is_empty() {
                    self.word_count_from(&self.tree.root)
                } else {
                    roots
                        .iter()
                        .filter_map(|path| self.tree.find_by_path(path))
                        .map(|node| self.word_count_from(node))
                        .sum()
                }
            }
            WordCountScope::EverythingExceptTrash => self.word_count_from(&self.tree.root),
        }
    }

    /// Recursive walk behind [`Project::word_count`] — mirrors
    /// `export::gather_into`'s Trash-or-Templates skip exactly, but sums
    /// `frontmatter::count_words` instead of parsing markdown IR.
    fn word_count_from(&self, node: &BinderNode) -> usize {
        match &node.kind {
            BinderNodeKind::Document => fs::read_to_string(&node.path)
                .map(|contents| crate::frontmatter::count_words(&contents))
                .unwrap_or(0),
            BinderNodeKind::Folder { children } => {
                if matches!(
                    self.folder_role(&node.path),
                    Some(FolderRole::Trash) | Some(FolderRole::Templates)
                ) {
                    return 0;
                }
                children
                    .iter()
                    .map(|child| self.word_count_from(child))
                    .sum()
            }
        }
    }

    /// Whether the document at `path` would count toward [`Project::word_count`]
    /// under `scope` — a cheap, no-disk-I/O predicate (just path/role
    /// comparisons) used to gate the live "characters typed this session"
    /// counter (`SmaragdApp::track_char_activity`) to keystrokes made in a
    /// tracked document, without re-walking the whole project on every edit.
    pub fn is_path_tracked(&self, path: &Path, scope: WordCountScope) -> bool {
        let excluded_by_role = path
            .ancestors()
            .skip(1)
            .take_while(|ancestor| ancestor.starts_with(&self.root))
            .any(|ancestor| {
                matches!(
                    self.folder_role(ancestor),
                    Some(FolderRole::Trash) | Some(FolderRole::Templates)
                )
            });
        if excluded_by_role {
            return false;
        }
        match scope {
            WordCountScope::EverythingExceptTrash => true,
            WordCountScope::ManuscriptOnly => {
                let manuscript_paths = self.folder_role_paths(FolderRole::Manuscript);
                manuscript_paths.is_empty()
                    || manuscript_paths.iter().any(|root| path.starts_with(root))
            }
        }
    }

    /// Set the Draft Target (`None` clears it) — see
    /// [`ProjectMeta::draft_target_words`].
    pub fn set_draft_target_words(&mut self, target: Option<u32>) -> io::Result<()> {
        self.meta.draft_target_words = target;
        self.save_metadata()
    }

    /// Set the Session Target — see [`ProjectMeta::session_target_words`].
    pub fn set_session_target_words(&mut self, target: Option<u32>) -> io::Result<()> {
        self.meta.session_target_words = target;
        self.save_metadata()
    }

    /// Switch which documents count toward the Draft Target's live total — see
    /// [`WordCountScope`].
    pub fn set_word_count_scope(&mut self, scope: WordCountScope) -> io::Result<()> {
        self.meta.word_count_scope = scope;
        self.save_metadata()
    }

    /// Automatic daily rollover for the Session Target: re-baselines to
    /// `current_total` only if `session_baseline_date` isn't already today. A
    /// no-op otherwise, so it's safe to call unconditionally on every word-count
    /// recompute rather than needing its own separate trigger.
    ///
    /// Also the sole place `ProjectMeta::daily_word_counts` (the Writing
    /// Streak feature's history) gets written: whenever a genuine day change
    /// is detected, the previous day's word delta is logged under its date
    /// before rebaselining, and the history is pruned to
    /// `streak::DAILY_HISTORY_RETENTION_DAYS`. Deliberately reuses this
    /// existing rollover hook rather than a separate polling mechanism — see
    /// `ProjectMeta::daily_word_counts`'s doc comment for the accepted
    /// day-boundary-precision limitation this inherits.
    pub fn maybe_roll_over_session(&mut self, current_total: usize) -> io::Result<()> {
        let now = chrono::Local::now();
        let today = now.format("%Y-%m-%d").to_string();
        if self.meta.session_baseline_date.as_deref() == Some(today.as_str()) {
            return Ok(());
        }
        if let Some(prev_date) = self.meta.session_baseline_date.clone() {
            let words_written =
                current_total.saturating_sub(self.meta.session_baseline_words as usize) as u32;
            self.meta.daily_word_counts.insert(prev_date, words_written);
        }
        crate::streak::prune_daily_history(&mut self.meta.daily_word_counts, now.date_naive());
        self.meta.session_baseline_words = current_total as u32;
        self.meta.session_baseline_date = Some(today);
        self.save_metadata()
    }

    /// The Word Count panel's explicit "Reset Session" button — re-baselines the
    /// Session Target to `current_total` right now, unconditionally, unlike
    /// [`Project::maybe_roll_over_session`]'s same-day no-op.
    pub fn reset_session(&mut self, current_total: usize) -> io::Result<()> {
        self.meta.session_baseline_words = current_total as u32;
        self.meta.session_baseline_date = Some(chrono::Local::now().format("%Y-%m-%d").to_string());
        self.save_metadata()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_draft_target_words_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        assert_eq!(project.meta.draft_target_words, None);

        project.set_draft_target_words(Some(50_000)).unwrap();
        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(reloaded.meta.draft_target_words, Some(50_000));

        project.set_draft_target_words(None).unwrap();
        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(reloaded.meta.draft_target_words, None);
    }

    #[test]
    fn set_word_count_scope_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        assert_eq!(
            project.meta.word_count_scope,
            WordCountScope::ManuscriptOnly
        );

        project
            .set_word_count_scope(WordCountScope::EverythingExceptTrash)
            .unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(
            reloaded.meta.word_count_scope,
            WordCountScope::EverythingExceptTrash
        );
    }

    #[test]
    fn word_count_excludes_trash_and_templates_in_both_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        let templates = project.create_folder(dir.path(), "Templates").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        project
            .set_folder_role(&templates, Some(FolderRole::Templates))
            .unwrap();
        let trashed_doc = project.create_document(&trash, "Old Scene").unwrap();
        fs::write(&trashed_doc, "one two three four five").unwrap();
        let template_doc = project
            .create_document(&templates, "Scene Template")
            .unwrap();
        fs::write(&template_doc, "one two three").unwrap();
        let kept_doc = project.create_document(dir.path(), "Scene").unwrap();
        fs::write(&kept_doc, "one two").unwrap();

        assert_eq!(project.word_count(WordCountScope::ManuscriptOnly), 2);
        assert_eq!(project.word_count(WordCountScope::EverythingExceptTrash), 2);
    }

    #[test]
    fn word_count_manuscript_only_falls_back_to_whole_project_minus_trash_and_templates_when_unassigned()
     {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let research = project.create_folder(dir.path(), "Research").unwrap();
        project
            .set_folder_role(&research, Some(FolderRole::Research))
            .unwrap();
        let research_doc = project.create_document(&research, "Notes").unwrap();
        fs::write(&research_doc, "one two three").unwrap();
        let manuscript_doc = project.create_document(dir.path(), "Scene").unwrap();
        fs::write(&manuscript_doc, "one two").unwrap();

        // No folder holds `Manuscript` yet, so it falls back to the whole
        // project — Research still counts, unlike Trash/Templates.
        assert_eq!(project.word_count(WordCountScope::ManuscriptOnly), 5);
    }

    #[test]
    fn word_count_manuscript_only_sums_multiple_manuscript_folders() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let book_one = project.create_folder(dir.path(), "Book One").unwrap();
        let book_two = project.create_folder(dir.path(), "Book Two").unwrap();
        project
            .set_folder_role(&book_one, Some(FolderRole::Manuscript))
            .unwrap();
        project
            .set_folder_role(&book_two, Some(FolderRole::Manuscript))
            .unwrap();
        let doc_one = project.create_document(&book_one, "Scene 1").unwrap();
        fs::write(&doc_one, "one two three").unwrap();
        let doc_two = project.create_document(&book_two, "Scene 1").unwrap();
        fs::write(&doc_two, "one two").unwrap();
        // Not under either Manuscript folder — must not be counted.
        let outside_doc = project.create_document(dir.path(), "Outtake").unwrap();
        fs::write(&outside_doc, "one two three four").unwrap();

        assert_eq!(project.word_count(WordCountScope::ManuscriptOnly), 5);
    }

    #[test]
    fn word_count_everything_except_trash_includes_research_excludes_templates() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let research = project.create_folder(dir.path(), "Research").unwrap();
        let templates = project.create_folder(dir.path(), "Templates").unwrap();
        project
            .set_folder_role(&research, Some(FolderRole::Research))
            .unwrap();
        project
            .set_folder_role(&templates, Some(FolderRole::Templates))
            .unwrap();
        let research_doc = project.create_document(&research, "Notes").unwrap();
        fs::write(&research_doc, "one two three").unwrap();
        let template_doc = project.create_document(&templates, "Blank").unwrap();
        fs::write(&template_doc, "one two three four five").unwrap();

        assert_eq!(project.word_count(WordCountScope::EverythingExceptTrash), 3);
    }

    #[test]
    fn is_path_tracked_excludes_trash_and_templates_in_both_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        let templates = project.create_folder(dir.path(), "Templates").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        project
            .set_folder_role(&templates, Some(FolderRole::Templates))
            .unwrap();
        let trashed_doc = project.create_document(&trash, "Old Scene").unwrap();
        let template_doc = project.create_document(&templates, "Template").unwrap();
        let plain_doc = project.create_document(dir.path(), "Scene").unwrap();

        for scope in [
            WordCountScope::ManuscriptOnly,
            WordCountScope::EverythingExceptTrash,
        ] {
            assert!(!project.is_path_tracked(&trashed_doc, scope));
            assert!(!project.is_path_tracked(&template_doc, scope));
            assert!(project.is_path_tracked(&plain_doc, scope));
        }
    }

    #[test]
    fn is_path_tracked_manuscript_only_requires_being_under_a_manuscript_folder_once_one_exists() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let manuscript = project.create_folder(dir.path(), "Book").unwrap();
        project
            .set_folder_role(&manuscript, Some(FolderRole::Manuscript))
            .unwrap();
        let inside = project.create_document(&manuscript, "Scene").unwrap();
        let outside = project.create_document(dir.path(), "Outtake").unwrap();

        assert!(project.is_path_tracked(&inside, WordCountScope::ManuscriptOnly));
        assert!(!project.is_path_tracked(&outside, WordCountScope::ManuscriptOnly));
        // Everything-except-Trash isn't restricted to the Manuscript folder.
        assert!(project.is_path_tracked(&outside, WordCountScope::EverythingExceptTrash));
    }

    #[test]
    fn is_path_tracked_manuscript_only_falls_back_to_whole_project_when_unassigned() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene").unwrap();

        assert!(project.is_path_tracked(&doc, WordCountScope::ManuscriptOnly));
    }

    #[test]
    fn maybe_roll_over_session_is_a_noop_within_the_same_day() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.maybe_roll_over_session(100).unwrap();
        let date_after_first_call = project.meta.session_baseline_date.clone();

        project.maybe_roll_over_session(250).unwrap();

        assert_eq!(project.meta.session_baseline_words, 100);
        assert_eq!(project.meta.session_baseline_date, date_after_first_call);
        assert!(project.meta.daily_word_counts.is_empty());
    }

    #[test]
    fn maybe_roll_over_session_rebaselines_on_a_new_day() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.meta.session_baseline_words = 100;
        project.meta.session_baseline_date = Some("2000-01-01".to_string());
        project.save_metadata().unwrap();

        project.maybe_roll_over_session(300).unwrap();

        assert_eq!(project.meta.session_baseline_words, 300);
        assert_ne!(
            project.meta.session_baseline_date,
            Some("2000-01-01".to_string())
        );
    }

    #[test]
    fn maybe_roll_over_session_logs_nothing_on_the_very_first_call() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        assert!(project.meta.session_baseline_date.is_none());

        project.maybe_roll_over_session(100).unwrap();

        assert!(project.meta.daily_word_counts.is_empty());
    }

    #[test]
    fn maybe_roll_over_session_logs_the_previous_days_delta_under_its_own_date() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let yesterday = (chrono::Local::now().date_naive() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        project.meta.session_baseline_words = 100;
        project.meta.session_baseline_date = Some(yesterday.clone());
        project.save_metadata().unwrap();

        project.maybe_roll_over_session(350).unwrap();

        assert_eq!(project.meta.daily_word_counts.get(&yesterday), Some(&250));
    }

    #[test]
    fn maybe_roll_over_session_prunes_daily_history_older_than_the_retention_window() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let stale_date = (chrono::Local::now().date_naive()
            - chrono::Duration::days(crate::streak::DAILY_HISTORY_RETENTION_DAYS + 10))
        .format("%Y-%m-%d")
        .to_string();
        project
            .meta
            .daily_word_counts
            .insert(stale_date.clone(), 999);
        project.meta.session_baseline_date = Some("2000-01-01".to_string());
        project.save_metadata().unwrap();

        project.maybe_roll_over_session(100).unwrap();

        assert!(!project.meta.daily_word_counts.contains_key(&stale_date));
    }

    #[test]
    fn reset_session_rebaselines_unconditionally() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.maybe_roll_over_session(100).unwrap();

        project.reset_session(400).unwrap();

        assert_eq!(project.meta.session_baseline_words, 400);
    }
}
