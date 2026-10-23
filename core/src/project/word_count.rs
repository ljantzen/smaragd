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
            BinderNodeKind::Document => self
                .store
                .read_to_string(&node.path)
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

    /// Every folder's combined descendant word count, keyed by that folder's
    /// absolute path (the project root included) — the data source for
    /// `BinderColorMode::WordCountProgress`'s folder rows (see
    /// `ProjectMeta::binder_color_mode`). A fresh recursive walk rather than a
    /// reuse of `word_count_from`: that one is shaped around a *single*,
    /// `WordCountScope`-dependent root selection, whereas this always wants
    /// *every* folder's own subtree total unconditionally. Deliberately
    /// scoped by `WordCountScope::EverythingExceptTrash` regardless of
    /// `ProjectMeta::word_count_scope` — the same "never aggregated together"
    /// reasoning `ProjectMeta::draft_target_words`'s own doc comment gives for
    /// staying independent of a document's own `word_count_target` — via
    /// [`Project::is_path_tracked`] rather than re-deriving the Trash/
    /// Templates exclusion here. Like [`Project::word_count`], reads every
    /// tracked document from disk, so callers must run this off the UI thread
    /// — see `app::spawn_word_count_recompute`, which computes this in the
    /// same background pass as the Draft Target total.
    pub fn folder_word_counts(&self) -> HashMap<PathBuf, usize> {
        let mut totals = HashMap::new();
        self.folder_word_counts_from(&self.tree.root, &mut totals);
        totals
    }

    /// Recursive worker behind [`Project::folder_word_counts`]: returns this
    /// node's own word count (a document's, or a folder's summed-from-
    /// children total) and, for a folder, additionally records that total
    /// into `totals` as a side effect — so one bottom-up walk populates every
    /// folder's entry in a single pass.
    fn folder_word_counts_from(
        &self,
        node: &BinderNode,
        totals: &mut HashMap<PathBuf, usize>,
    ) -> usize {
        match &node.kind {
            BinderNodeKind::Document => {
                if !self.is_path_tracked(&node.path, WordCountScope::EverythingExceptTrash) {
                    return 0;
                }
                self.store
                    .read_to_string(&node.path)
                    .map(|contents| crate::frontmatter::count_words(&contents))
                    .unwrap_or(0)
            }
            BinderNodeKind::Folder { children } => {
                let total = children
                    .iter()
                    .map(|child| self.folder_word_counts_from(child, totals))
                    .sum();
                totals.insert(node.path.clone(), total);
                total
            }
        }
    }

    /// Whether the document at `path` would count toward [`Project::word_count`]
    /// under `scope` — a cheap, no-disk-I/O predicate (just path/role
    /// comparisons) used to gate the live "characters typed today" counters
    /// (`SmaragdApp::track_char_activity`, which checks this once per scope)
    /// to keystrokes made in a tracked document, without re-walking the whole
    /// project on every edit.
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

    /// Set Today's Target — see [`ProjectMeta::today_target_words`].
    pub fn set_today_target_words(&mut self, target: Option<u32>) -> io::Result<()> {
        self.meta.today_target_words = target;
        self.save_metadata()
    }

    /// Switch which documents count toward the Draft Target's live total — see
    /// [`WordCountScope`]. Deliberately touches nothing else: each scope keeps
    /// its own Today baseline (see
    /// [`ProjectMeta::today_baseline_words_everything_except_trash`]), so
    /// switching back and forth is purely a display toggle and never resets
    /// or loses the day's progress.
    pub fn set_word_count_scope(&mut self, scope: WordCountScope) -> io::Result<()> {
        self.meta.word_count_scope = scope;
        self.save_metadata()
    }

    /// Today's baseline for `scope` — see
    /// [`ProjectMeta::today_baseline_words_everything_except_trash`].
    pub fn today_baseline_words(&self, scope: WordCountScope) -> u32 {
        match scope {
            WordCountScope::ManuscriptOnly => self.meta.today_baseline_words_manuscript_only,
            WordCountScope::EverythingExceptTrash => {
                self.meta.today_baseline_words_everything_except_trash
            }
        }
    }

    fn set_today_baseline_words(&mut self, scope: WordCountScope, total: u32) {
        match scope {
            WordCountScope::ManuscriptOnly => {
                self.meta.today_baseline_words_manuscript_only = total
            }
            WordCountScope::EverythingExceptTrash => {
                self.meta.today_baseline_words_everything_except_trash = total
            }
        }
    }

    /// Re-baselines *both* scopes' Today totals together, from a total
    /// computed under each — `active_scope`/`active_total` are the scope and
    /// total the caller already has in hand (typically from a just-completed
    /// background recompute), so only the other, inactive scope needs a
    /// fresh (synchronous) [`Project::word_count`] call here. Shared by
    /// [`Project::maybe_roll_over_today`] and [`Project::reset_today`] —
    /// rebaselining only the active scope would leave the other one's
    /// baseline stale, so switching to it later would show a wrong, usually
    /// much too large, Today count.
    fn rebaseline_both_scopes(&mut self, active_scope: WordCountScope, active_total: usize) {
        for scope in [
            WordCountScope::ManuscriptOnly,
            WordCountScope::EverythingExceptTrash,
        ] {
            let total = if scope == active_scope {
                active_total
            } else {
                self.word_count(scope)
            };
            self.set_today_baseline_words(scope, total as u32);
        }
    }

    /// Automatic daily rollover for Today's Target: re-baselines to
    /// `current_total` only if `today_baseline_date` isn't already today. A
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
    /// day-boundary-precision limitation this inherits. `current_total` is
    /// assumed to be under `self.meta.word_count_scope` (the scope the
    /// caller's background recompute just ran under).
    pub fn maybe_roll_over_today(&mut self, current_total: usize) -> io::Result<()> {
        let now = chrono::Local::now();
        let today = now.format("%Y-%m-%d").to_string();
        if self.meta.today_baseline_date.as_deref() == Some(today.as_str()) {
            return Ok(());
        }
        if let Some(prev_date) = self.meta.today_baseline_date.clone() {
            let prev_baseline = self.today_baseline_words(self.meta.word_count_scope);
            let words_written = current_total.saturating_sub(prev_baseline as usize) as u32;
            self.meta.daily_word_counts.insert(prev_date, words_written);
        }
        crate::streak::prune_daily_history(&mut self.meta.daily_word_counts, now.date_naive());
        self.rebaseline_both_scopes(self.meta.word_count_scope, current_total);
        self.meta.today_baseline_date = Some(today);
        self.save_metadata()
    }

    /// The Word Count panel's explicit "Reset Today's Count" button —
    /// re-baselines Today's Target to `current_total` right now, unconditionally, unlike
    /// [`Project::maybe_roll_over_today`]'s same-day no-op. `current_total` is
    /// assumed to be under `self.meta.word_count_scope`, same convention as
    /// [`Project::maybe_roll_over_today`]; rebaselines the other scope too
    /// (see [`Project::rebaseline_both_scopes`]), so the reset isn't
    /// immediately undone by switching scope right after clicking it.
    pub fn reset_today(&mut self, current_total: usize) -> io::Result<()> {
        self.rebaseline_both_scopes(self.meta.word_count_scope, current_total);
        self.meta.today_baseline_date = Some(chrono::Local::now().format("%Y-%m-%d").to_string());
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
    fn set_word_count_scope_never_touches_either_scopes_today_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let manuscript = project.create_folder(dir.path(), "Manuscript").unwrap();
        project
            .set_folder_role(&manuscript, Some(FolderRole::Manuscript))
            .unwrap();
        let scene = project.create_document(&manuscript, "Scene").unwrap();
        fs::write(&scene, "one two three").unwrap();
        let outtake = project.create_document(dir.path(), "Outtake").unwrap();
        fs::write(&outtake, "four five").unwrap();

        // Rolling over today's session baselines both scopes: 3 words under
        // ManuscriptOnly, 5 under EverythingExceptTrash.
        project.maybe_roll_over_today(3).unwrap();
        assert_eq!(
            project.today_baseline_words(WordCountScope::ManuscriptOnly),
            3
        );
        assert_eq!(
            project.today_baseline_words(WordCountScope::EverythingExceptTrash),
            5
        );

        // Switching scope back and forth is purely a display toggle — it
        // must not reset or touch either baseline, so no typing at all still
        // reads "Today: 0 words" under either scope.
        project
            .set_word_count_scope(WordCountScope::EverythingExceptTrash)
            .unwrap();
        project
            .set_word_count_scope(WordCountScope::ManuscriptOnly)
            .unwrap();

        assert_eq!(
            project.today_baseline_words(WordCountScope::ManuscriptOnly),
            3
        );
        assert_eq!(
            project.today_baseline_words(WordCountScope::EverythingExceptTrash),
            5
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
    fn maybe_roll_over_today_is_a_noop_within_the_same_day() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.maybe_roll_over_today(100).unwrap();
        let date_after_first_call = project.meta.today_baseline_date.clone();

        project.maybe_roll_over_today(250).unwrap();

        assert_eq!(
            project.today_baseline_words(WordCountScope::ManuscriptOnly),
            100
        );
        assert_eq!(project.meta.today_baseline_date, date_after_first_call);
        assert!(project.meta.daily_word_counts.is_empty());
    }

    #[test]
    fn maybe_roll_over_today_rebaselines_on_a_new_day() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.meta.today_baseline_words_manuscript_only = 100;
        project.meta.today_baseline_date = Some("2000-01-01".to_string());
        project.save_metadata().unwrap();

        project.maybe_roll_over_today(300).unwrap();

        assert_eq!(
            project.today_baseline_words(WordCountScope::ManuscriptOnly),
            300
        );
        assert_ne!(
            project.meta.today_baseline_date,
            Some("2000-01-01".to_string())
        );
    }

    #[test]
    fn maybe_roll_over_today_logs_nothing_on_the_very_first_call() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        assert!(project.meta.today_baseline_date.is_none());

        project.maybe_roll_over_today(100).unwrap();

        assert!(project.meta.daily_word_counts.is_empty());
    }

    #[test]
    fn maybe_roll_over_today_logs_the_previous_days_delta_under_its_own_date() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let yesterday = (chrono::Local::now().date_naive() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        project.meta.today_baseline_words_manuscript_only = 100;
        project.meta.today_baseline_date = Some(yesterday.clone());
        project.save_metadata().unwrap();

        project.maybe_roll_over_today(350).unwrap();

        assert_eq!(project.meta.daily_word_counts.get(&yesterday), Some(&250));
    }

    #[test]
    fn maybe_roll_over_today_prunes_daily_history_older_than_the_retention_window() {
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
        project.meta.today_baseline_date = Some("2000-01-01".to_string());
        project.save_metadata().unwrap();

        project.maybe_roll_over_today(100).unwrap();

        assert!(!project.meta.daily_word_counts.contains_key(&stale_date));
    }

    #[test]
    fn today_count_increases_when_the_days_first_rollover_happened_under_a_different_scope() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let manuscript = project.create_folder(dir.path(), "Manuscript").unwrap();
        project
            .set_folder_role(&manuscript, Some(FolderRole::Manuscript))
            .unwrap();
        let scene = project.create_document(&manuscript, "Scene").unwrap();
        fs::write(&scene, "one two three").unwrap();
        let outtake = project.create_document(dir.path(), "Outtake").unwrap();
        fs::write(&outtake, "four five").unwrap();

        // App starts: ManuscriptOnly is still the persisted/default scope,
        // so the day's first rollover happens under it — EverythingExceptTrash's
        // baseline is set via the *inactive*-scope branch of
        // `rebaseline_both_scopes`, not the active-scope one.
        let opening_total = project.word_count(WordCountScope::ManuscriptOnly);
        assert_eq!(opening_total, 3);
        project.maybe_roll_over_today(opening_total).unwrap();
        assert_eq!(
            project.today_baseline_words(WordCountScope::EverythingExceptTrash),
            5,
            "EverythingExceptTrash's baseline must be captured at app-open too, \
             even though ManuscriptOnly is the scope driving this rollover"
        );

        // Now the user switches scope (no baseline change) and types+saves
        // outside the manuscript.
        project
            .set_word_count_scope(WordCountScope::EverythingExceptTrash)
            .unwrap();
        fs::write(&outtake, "four five six seven eight nine ten").unwrap();
        let new_total = project.word_count(WordCountScope::EverythingExceptTrash);
        assert_eq!(new_total, 10);
        project.maybe_roll_over_today(new_total).unwrap();

        let today_words = new_total.saturating_sub(
            project.today_baseline_words(WordCountScope::EverythingExceptTrash) as usize,
        );
        assert_eq!(
            today_words, 5,
            "Today count must reflect the 5 new words typed"
        );
    }

    #[test]
    fn today_count_increases_after_typing_while_everything_except_trash_is_active() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let manuscript = project.create_folder(dir.path(), "Manuscript").unwrap();
        project
            .set_folder_role(&manuscript, Some(FolderRole::Manuscript))
            .unwrap();
        let scene = project.create_document(&manuscript, "Scene").unwrap();
        fs::write(&scene, "one two three").unwrap();
        let outtake = project.create_document(dir.path(), "Outtake").unwrap();
        fs::write(&outtake, "four five").unwrap();

        project
            .set_word_count_scope(WordCountScope::EverythingExceptTrash)
            .unwrap();
        let initial_total = project.word_count(WordCountScope::EverythingExceptTrash);
        assert_eq!(initial_total, 5);
        project.maybe_roll_over_today(initial_total).unwrap();
        assert_eq!(
            project.today_baseline_words(WordCountScope::EverythingExceptTrash),
            5
        );

        // "Type" more words into the outtake document and simulate the
        // save-triggered recompute that follows.
        fs::write(&outtake, "four five six seven eight nine ten").unwrap();
        let new_total = project.word_count(WordCountScope::EverythingExceptTrash);
        assert_eq!(new_total, 10);
        project.maybe_roll_over_today(new_total).unwrap();

        let today_words = new_total.saturating_sub(
            project.today_baseline_words(WordCountScope::EverythingExceptTrash) as usize,
        );
        assert_eq!(
            today_words, 5,
            "Today count must reflect the 5 new words typed"
        );
    }

    #[test]
    fn today_count_increases_across_a_real_day_boundary_while_everything_except_trash_was_already_active()
     {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let outtake = project.create_document(dir.path(), "Outtake").unwrap();
        fs::write(&outtake, "one two three").unwrap();

        // Simulate yesterday: EverythingExceptTrash was already the
        // persisted scope, with yesterday's baseline on both fields.
        project.meta.word_count_scope = WordCountScope::EverythingExceptTrash;
        project.meta.today_baseline_words_manuscript_only = 3;
        project.meta.today_baseline_words_everything_except_trash = 3;
        let yesterday = (chrono::Local::now().date_naive() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        project.meta.today_baseline_date = Some(yesterday);
        project.save_metadata().unwrap();

        // The app opens today; the first recompute runs under the
        // already-persisted EverythingExceptTrash scope.
        let todays_total = project.word_count(WordCountScope::EverythingExceptTrash);
        assert_eq!(todays_total, 3);
        project.maybe_roll_over_today(todays_total).unwrap();
        let today_words = todays_total.saturating_sub(
            project.today_baseline_words(WordCountScope::EverythingExceptTrash) as usize,
        );
        assert_eq!(today_words, 0, "nothing written yet today");

        // Now type and save 4 more words.
        fs::write(&outtake, "one two three four five six seven").unwrap();
        let new_total = project.word_count(WordCountScope::EverythingExceptTrash);
        project.maybe_roll_over_today(new_total).unwrap();
        let today_words = new_total.saturating_sub(
            project.today_baseline_words(WordCountScope::EverythingExceptTrash) as usize,
        );
        assert_eq!(today_words, 4);
    }

    #[test]
    fn reset_today_rebaselines_unconditionally() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.maybe_roll_over_today(100).unwrap();

        project.reset_today(400).unwrap();

        assert_eq!(
            project.today_baseline_words(WordCountScope::ManuscriptOnly),
            400
        );
    }

    #[test]
    fn reset_today_also_rebaselines_the_inactive_scope() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let manuscript = project.create_folder(dir.path(), "Manuscript").unwrap();
        project
            .set_folder_role(&manuscript, Some(FolderRole::Manuscript))
            .unwrap();
        let scene = project.create_document(&manuscript, "Scene").unwrap();
        fs::write(&scene, "one two three").unwrap();
        let outtake = project.create_document(dir.path(), "Outtake").unwrap();
        fs::write(&outtake, "four five").unwrap();

        // Active scope is ManuscriptOnly (the default); reset_today's
        // current_total (3) is under that scope.
        project.reset_today(3).unwrap();

        assert_eq!(
            project.today_baseline_words(WordCountScope::ManuscriptOnly),
            3
        );
        // EverythingExceptTrash must also be freshly rebaselined (to 5), not
        // left stale — otherwise switching to it right after "Reset Today's
        // Count" would still show the old, pre-reset Today count.
        assert_eq!(
            project.today_baseline_words(WordCountScope::EverythingExceptTrash),
            5
        );
    }

    #[test]
    fn folder_word_counts_sums_direct_and_nested_descendant_documents() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let scene = project.create_folder(&chapter, "Scene 1").unwrap();
        let doc_a = project.create_document(&chapter, "A").unwrap();
        fs::write(&doc_a, "one two three").unwrap();
        let doc_b = project.create_document(&scene, "B").unwrap();
        fs::write(&doc_b, "four five").unwrap();

        let totals = project.folder_word_counts();

        assert_eq!(totals.get(&scene), Some(&2));
        assert_eq!(totals.get(&chapter), Some(&5));
    }

    #[test]
    fn folder_word_counts_excludes_trash_and_templates_subtrees() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let trashed_doc = project.create_document(&trash, "Old Scene").unwrap();
        fs::write(&trashed_doc, "one two three four five").unwrap();

        let totals = project.folder_word_counts();

        assert_eq!(totals.get(&trash), Some(&0));
    }

    #[test]
    fn folder_word_counts_includes_the_project_root_itself() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene").unwrap();
        fs::write(&doc, "one two three").unwrap();

        let totals = project.folder_word_counts();

        assert_eq!(totals.get(&project.root.clone()), Some(&3));
    }

    #[test]
    fn folder_word_counts_gives_a_leaf_folder_with_no_documents_a_total_of_zero() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let empty = project.create_folder(dir.path(), "Empty").unwrap();

        let totals = project.folder_word_counts();

        assert_eq!(totals.get(&empty), Some(&0));
    }

    #[test]
    fn folder_word_counts_is_unaffected_by_manuscript_only_scope() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let manuscript = project.create_folder(dir.path(), "Manuscript").unwrap();
        project
            .set_folder_role(&manuscript, Some(FolderRole::Manuscript))
            .unwrap();
        project
            .set_word_count_scope(WordCountScope::ManuscriptOnly)
            .unwrap();
        // Outside the Manuscript-role folder — `word_count(ManuscriptOnly)`
        // would exclude this, but `folder_word_counts` always sums every
        // tracked document regardless of `word_count_scope`.
        let doc = project.create_document(dir.path(), "Outtake").unwrap();
        fs::write(&doc, "one two three").unwrap();

        let totals = project.folder_word_counts();

        assert_eq!(totals.get(&project.root.clone()), Some(&3));
    }
}
