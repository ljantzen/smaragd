use super::*;

impl SmaragdApp {
    /// Open `path` as a project. Used for the automatic "reopen last project" path at
    /// startup, where a missing `.smaragd` marker must just be reported (not
    /// interactively resolved) — the user didn't just explicitly ask to open this
    /// folder, so an unprompted modal dialog on launch would be wrong.
    pub(super) fn open_project(&mut self, ctx: &egui::Context, path: &Path) {
        match Project::load_from_folder(path) {
            Ok(project) => self.set_project(ctx, project, path),
            Err(err) => {
                self.push_error_toast(format!("Couldn't open {}: {err}", path.display()));
            }
        }
    }

    fn set_project(&mut self, ctx: &egui::Context, mut project: Project, path: &Path) {
        if self.settings.create_starter_folders {
            Self::ensure_starter_folders(&mut project);
        }
        // Default to whichever inner Streak tab is more useful for this
        // specific project — `Streak` (the live badge/progress view) if
        // tracking is already on, `Configure` otherwise — but only as a
        // starting point: the user can freely switch afterward, and nothing
        // else resets this until the next project open.
        self.streak_sub_tab = if project.meta.streak_enabled {
            ui::streak_panel::StreakSubTab::Streak
        } else {
            ui::streak_panel::StreakSubTab::Configure
        };
        self.project = Some(project);
        self.editor = EditorState::default();
        self.selected_path = None;
        self.document_history = DocumentHistory::default();
        self.metadata.target = MetadataTarget::Document;
        self.metadata.folder_computed_for = None;
        self.document_status_cache.clear();
        self.clear_status_message();
        self.settings.last_project_path = Some(path.to_path_buf());
        self.settings.record_recent_project(path);
        self.persist_settings();
        // Everything below here is a real-world side effect inappropriate
        // for a test to trigger — most notably `maybe_offer_git_support`,
        // which pops up a blocking native OS dialog and can hang a test run
        // until someone dismisses it — see `is_test_fixture`'s doc comment.
        if self.is_test_fixture {
            return;
        }
        if self.settings.git_integration_enabled() {
            self.maybe_offer_git_support();
            if let Some(project) = &self.project
                && let Err(err) = Self::ensure_git_repo(project)
            {
                self.push_error_toast(format!("Couldn't initialize git: {err}"));
            }
            self.refresh_git_dirty_paths();
        }
        self.run_backup(BackupTrigger::Open);
        self.reload_plugins();
        self.word_count.last_dirty = false;
        self.word_count.char_activity = 0;
        self.word_count.char_activity_last_len = None;
        self.word_count.char_activity_tracked_path = None;
        self.spawn_word_count_recompute(ctx);
    }

    /// Open `path` as a project in response to an explicit user action (the "Open
    /// Project" menu item). If `path` has never been opened by smaragd before (no
    /// `.smaragd/project.json`), offers via a native Yes/No dialog to set it up in
    /// place, matching `delete_node`'s confirmation pattern.
    pub(super) fn open_project_or_offer_to_adopt(&mut self, ctx: &egui::Context, path: &Path) {
        match Project::load_from_folder(path) {
            Ok(project) => self.set_project(ctx, project, path),
            Err(LoadError::NotInitialized(_)) => {
                let adopt = rfd::MessageDialog::new()
                    .set_title("Set Up Project")
                    .set_description(format!(
                        "\"{}\" hasn't been opened in smaragd before. Set it up as a smaragd project here?",
                        path.display()
                    ))
                    .set_level(rfd::MessageLevel::Info)
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show();
                if adopt == rfd::MessageDialogResult::Yes {
                    match Project::initialize(path) {
                        Ok(project) => self.set_project(ctx, project, path),
                        Err(err) => {
                            self.push_error_toast(format!(
                                "Couldn't set up {}: {err}",
                                path.display()
                            ));
                        }
                    }
                }
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't open {}: {err}", path.display()));
            }
        }
    }

    /// Open the OS's native folder-picker dialog and, if the user selects a folder,
    /// open it as a project immediately (offering to adopt it if needed).
    pub(super) fn browse_for_project(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.open_project_or_offer_to_adopt(ctx, &path);
        }
    }

    /// Start the "New Project" flow: first the template-choice modal (see
    /// `start_new_project_with_template` for the rest of the flow, once a template's
    /// been chosen). On a genuine first launch, pre-selects the richer
    /// World-Building template instead of defaulting to Blank — a first-time user
    /// benefits more from seeing what a scaffolded project looks like than from
    /// today's "just an empty project" default.
    pub(super) fn start_new_project(&mut self) {
        if self.settings.is_first_launch() {
            self.new_project_template_prompt
                .request_open_preferring("worldbuilding", &self.project_templates);
        } else {
            self.new_project_template_prompt.request_open();
        }
    }

    /// Continue "New Project" once `template_id` has been chosen: pick a folder via
    /// the native folder picker. If that folder is empty, there's nothing useful a
    /// project name would add beyond what the folder is already called, so the
    /// project is created directly in it, skipping the name-prompt modal. Otherwise
    /// (a non-empty folder, meant as the *parent* for a new subfolder), prompt for
    /// the new project's name via the existing name-prompt modal.
    pub(super) fn start_new_project_with_template(
        &mut self,
        ctx: &egui::Context,
        template_id: String,
    ) {
        let Some(location) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        if is_empty_dir(&location) {
            self.initialize_and_set_project(ctx, &location, &template_id);
            return;
        }
        self.prompt = Some(PendingPrompt {
            action: PromptAction::NewProject {
                location,
                template_id,
            },
            state: NamePromptState::new("New Project", "Create", ""),
        });
    }

    pub(super) fn create_project(
        &mut self,
        ctx: &egui::Context,
        location: &Path,
        name: &str,
        template_id: &str,
    ) {
        let root = location.join(name);
        if root.exists() {
            // Unlike the adopt flow, "New Project" should only ever create a fresh
            // folder — silently folding an unrelated existing folder in as a project
            // would be surprising.
            self.push_error_toast(format!("{} already exists", root.display()));
            return;
        }
        self.initialize_and_set_project(ctx, &root, template_id);
    }

    /// Shared tail end of both "New Project" paths: initialize `root` as a fresh
    /// smaragd project, apply `template_id`'s scaffolding, and make it the open
    /// project.
    fn initialize_and_set_project(&mut self, ctx: &egui::Context, root: &Path, template_id: &str) {
        match Project::initialize(root) {
            Ok(mut project) => {
                // An id that no longer resolves (e.g. a custom template deleted
                // between picker and confirm) is treated as "no scaffolding" rather
                // than a hard error — the same fallback Blank itself produces.
                let template_error =
                    crate::project_template::find(&self.project_templates, template_id)
                        .and_then(|template| template.apply(&mut project).err());
                // `set_project` unconditionally clears `status_message`, so a
                // template-apply error must be recorded after it runs, not before.
                self.set_project(ctx, project, root);
                if let Some(err) = template_error {
                    self.push_error_toast(format!("Couldn't apply template: {err}"));
                }
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't create project: {err}"));
            }
        }
    }

    /// Open the "Save Project as Template" name-prompt modal.
    pub(super) fn prompt_save_project_as_template(&mut self) {
        self.prompt = Some(PendingPrompt {
            action: PromptAction::SaveProjectAsTemplate,
            state: NamePromptState::new("Save Project as Template", "Save", ""),
        });
    }

    pub(super) fn save_project_as_template(&mut self, name: &str) {
        let Some(project) = &self.project else {
            self.push_error_toast("No project open");
            return;
        };
        let Some(dir) = crate::project_template::global_project_templates_dir() else {
            self.push_error_toast("Couldn't determine templates directory");
            return;
        };
        match crate::project_template::save_from_project(&dir, name, project) {
            Ok(_) => {
                self.set_status_message(format!("Saved template \"{name}\""));
                self.reload_project_templates();
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't save template: {err}"));
            }
        }
    }

    /// Ensure the project has a Research and a Trash folder, checked and healed
    /// independently of each other (see `Project::ensure_role_folder`) — run every
    /// time a project is opened, not just when freshly created, so turning the
    /// "Create Research and Trash folders" setting on later, or manually deleting
    /// one of these folders outside the app, gets fixed on the next open rather than
    /// staying that way indefinitely. Best-effort: a failure here (e.g. a read-only
    /// filesystem) shouldn't block opening the project.
    fn ensure_starter_folders(project: &mut Project) {
        for (name, role) in [
            ("Research", crate::project::FolderRole::Research),
            ("Trash", crate::project::FolderRole::Trash),
        ] {
            let _ = project.ensure_role_folder(role, name);
        }
    }

    /// Close the currently open project (`File > Close Project` / `Ctrl+Shift+W`).
    /// Saves the open document and any open story-card draft first — same silent
    /// autosave convention as `open_document`/`close_document`, no Save/Discard/
    /// Cancel prompt — then ends an active collaboration session (it's scoped to
    /// whatever document was open) and resets every project-scoped bit of app
    /// state back to its "no project" default. Deliberately clears
    /// `settings.last_project_path` too: having just chosen to close it, a later
    /// "reopen last project on launch" shouldn't bring it right back. A no-op if
    /// no project is open.
    pub(super) fn close_project(&mut self, ctx: &egui::Context) {
        if self.project.is_none() {
            return;
        }
        if self.collab.is_some() {
            self.end_collab_session("Collaboration session ended: project closed");
        }
        if let Err(err) = self.editor.close() {
            self.push_error_toast(format!("Couldn't save before closing project: {err}"));
            return;
        }
        if self.card_draft.is_some() {
            self.finish_card_editor(CardEditorOutcome::Save);
        }
        // Cleared before the backup below (rather than at the end, like
        // every other bit of project-scoped state) so a "Backed up project"
        // confirmation from `run_backup` is the status message left showing,
        // not immediately wiped by a later unconditional clear.
        self.clear_status_message();
        self.run_backup(BackupTrigger::Close);
        self.git_dirty_paths.clear();
        self.export = None;
        if self.focus_mode {
            self.set_focus_mode(ctx, false);
        }
        self.project = None;
        self.selected_path = None;
        self.document_history = DocumentHistory::default();
        self.metadata = MetadataState::default();
        self.backlinks = BacklinksState::default();
        self.tags = TagsState::default();
        self.word_count = WordCountState::default();
        self.settings.last_project_path = None;
        self.persist_settings();
        self.reload_plugins();
    }

    /// Open `path` as a genuine switch to a different document — recorded as a
    /// fresh entry in `document_history` (see `load_document`). Every call
    /// site *except* `rename_node`'s post-rename reopen goes through here: a
    /// rename keeps the same logical document open (see
    /// `open_document_internal`), so it must not touch a session scoped to
    /// it.
    pub(super) fn open_document(&mut self, path: &Path) {
        if !self.confirm_leave_collab_session() {
            return;
        }
        self.open_document_internal(path);
    }

    /// The actual open, recorded in `document_history` — no collaboration-
    /// session teardown. Only `open_document` and `rename_node` (reopening
    /// the same document under its new name) should call this directly;
    /// `go_back_document`/`go_forward_document` call `load_document` instead,
    /// since a Back/Forward step must not itself count as a new visit.
    pub(super) fn open_document_internal(&mut self, path: &Path) {
        if self.load_document(path) {
            self.document_history.visit(path);
        }
    }

    /// Step to the previously visited document (File > Go Back /
    /// `ShortcutAction::GoBack`), restoring its last known cursor position.
    /// A no-op if there's nothing behind the current position, or (for an
    /// active collaboration joiner) if the user declines to end the session
    /// — see `confirm_leave_collab_session`.
    pub(super) fn go_back_document(&mut self) {
        let Some(target) = self.document_history.previous().map(Path::to_path_buf) else {
            return;
        };
        if !self.confirm_leave_collab_session() {
            return;
        }
        self.document_history.go_back();
        self.load_document(&target);
    }

    /// Step to the next visited document (File > Go Forward /
    /// `ShortcutAction::GoForward`) — see `go_back_document`.
    pub(super) fn go_forward_document(&mut self) {
        let Some(target) = self.document_history.next().map(Path::to_path_buf) else {
            return;
        };
        if !self.confirm_leave_collab_session() {
            return;
        }
        self.document_history.go_forward();
        self.load_document(&target);
    }

    /// Ask (via a native Yes/No dialog) before switching away from an active
    /// *joined* collaboration session, which has no document of its own
    /// outside the session (see `start_collab_join`) and can only survive a
    /// document switch by ending — declining leaves the shared document
    /// showing and the session alive, and this returns `false` so the caller
    /// backs out of the switch entirely. Matches `delete_node`'s confirmation
    /// pattern.
    ///
    /// Always returns `true` (no prompt) for a *host*, who keeps hosting
    /// through the switch — the next `sync_local_collab_edit` diffs the newly
    /// opened buffer against the stale baseline and ships the whole new
    /// document to the joiner as an ordinary (if large) update, so the
    /// joiner's view follows along automatically (see `CollabSession`'s
    /// module doc) — and for anyone not currently collaborating.
    fn confirm_leave_collab_session(&mut self) -> bool {
        let Some(session) = &self.collab else {
            return true;
        };
        match session.role {
            CollabRole::Host => true,
            CollabRole::Joiner => {
                let confirmed = rfd::MessageDialog::new()
                    .set_title("End Collaboration Session")
                    .set_description(
                        "Opening another document will end the collaboration session. Continue?",
                    )
                    .set_level(rfd::MessageLevel::Warning)
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show();
                if confirmed != rfd::MessageDialogResult::Yes {
                    return false;
                }
                self.end_collab_session("Collaboration session ended: switched documents");
                true
            }
        }
    }

    /// Load `path` into the editor and update `selected_path`/metadata/status-
    /// cache accordingly — the mechanics shared by every way of switching
    /// documents. Records the outgoing document's cursor position (from
    /// `EditorState::cursor_byte`, refreshed every frame by
    /// `editor_panel::show`) and queues the incoming document's last known
    /// position (`EditorState::pending_cursor`) to be restored on its next
    /// render — `0` if `path` has never been visited before.
    ///
    /// Deliberately does *not* touch `document_history`'s entries itself:
    /// callers decide whether this counts as a fresh visit
    /// (`open_document_internal`, via `document_history.visit`) or a
    /// Back/Forward step (`go_back_document`/`go_forward_document`, which
    /// only move the existing position). Returns whether the open succeeded.
    fn load_document(&mut self, path: &Path) -> bool {
        // Captured before `editor.open` runs (which may autosave a dirty
        // outgoing document first) — invalidated after, so its cached status
        // reflects whatever it was just saved as, not what it was before.
        let previous = self.editor.open_path.clone();
        if let Some(previous) = &previous {
            self.document_history
                .record_cursor(previous, self.editor.cursor_byte);
        }
        match self.editor.open(path) {
            Ok(()) => {
                self.selected_path = Some(path.to_path_buf());
                self.metadata.target = MetadataTarget::Document;
                self.editor.pending_cursor =
                    Some(self.document_history.cursor_for(path).unwrap_or(0));
                if let Some(previous) = previous {
                    self.document_status_cache.invalidate(&previous);
                }
                true
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't open {}: {err}", path.display()));
                false
            }
        }
    }

    /// Close the currently open document (silently autosaving first if dirty — same
    /// autosave convention as `open_document`/`rename_node`, no discard/cancel
    /// prompt). Records its cursor position in `document_history` first, so a
    /// later Back/Forward step or reopen picks up where the user left off.
    pub(super) fn close_document(&mut self, ctx: &egui::Context) {
        if self.collab.is_some() {
            self.end_collab_session("Collaboration session ended: document closed");
        }
        let previous = self.editor.open_path.clone();
        if let Some(previous) = &previous {
            self.document_history
                .record_cursor(previous, self.editor.cursor_byte);
        }
        if let Err(err) = self.editor.close() {
            self.push_error_toast(format!("Couldn't save before closing: {err}"));
            return;
        }
        if let Some(previous) = previous {
            self.document_status_cache.invalidate(&previous);
        }
        self.selected_path = None;
        if self.focus_mode {
            // Nothing left to show if the closed document was the one Focus Mode
            // was displaying — same reasoning as `set_focus_mode`'s own "refuses to
            // enter with no document open" guard, applied here on the way out.
            self.set_focus_mode(ctx, false);
        }
    }

    /// Move a file or folder into `new_parent` (a drag-and-drop in the binder). Keeps
    /// `selected_path`/the open editor's `open_path` following along if either was
    /// pointing at the moved item *or* something inside it (moving a folder relocates
    /// its whole subtree) — the buffer's content is untouched by a plain filesystem
    /// move, so there's nothing to save or reload, just retarget where Save will
    /// write.
    pub(super) fn move_item(&mut self, path: &Path, new_parent: &Path) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.move_item(path, new_parent) {
            Ok(new_path) => {
                let rebase = |p: &Path| -> Option<PathBuf> {
                    p.strip_prefix(path).ok().map(|rest| new_path.join(rest))
                };
                if let Some(rebased) = self.selected_path.as_deref().and_then(rebase) {
                    self.selected_path = Some(rebased);
                }
                if let Some(rebased) = self.editor.open_path.as_deref().and_then(rebase) {
                    self.editor.open_path = Some(rebased);
                }
                if let MetadataTarget::Folder(target) = &self.metadata.target
                    && let Some(rebased) = rebase(target)
                {
                    self.metadata.target = MetadataTarget::Folder(rebased);
                    self.metadata.folder_computed_for = None;
                }
                self.document_history.rebase_subtree(path, &new_path);
                self.document_status_cache.clear();
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't move {}: {err}", path.display()));
            }
        }
    }

    /// Same rebase-selected/open-path/history logic as `move_item`, for a document or
    /// folder dropped directly onto another document row (see
    /// `Project::move_item_before`).
    pub(super) fn move_item_before(&mut self, path: &Path, before: &Path) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.move_item_before(path, before) {
            Ok(new_path) => {
                let rebase = |p: &Path| -> Option<PathBuf> {
                    p.strip_prefix(path).ok().map(|rest| new_path.join(rest))
                };
                if let Some(rebased) = self.selected_path.as_deref().and_then(rebase) {
                    self.selected_path = Some(rebased);
                }
                if let Some(rebased) = self.editor.open_path.as_deref().and_then(rebase) {
                    self.editor.open_path = Some(rebased);
                }
                if let MetadataTarget::Folder(target) = &self.metadata.target
                    && let Some(rebased) = rebase(target)
                {
                    self.metadata.target = MetadataTarget::Folder(rebased);
                    self.metadata.folder_computed_for = None;
                }
                self.document_history.rebase_subtree(path, &new_path);
                self.document_status_cache.clear();
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't move {}: {err}", path.display()));
            }
        }
    }
}

/// True if `path` is a directory containing no entries at all — including dotfiles,
/// so a bare `.git` or `.smaragd` left behind still counts as "not empty" and routes
/// through the name-prompt flow rather than being silently adopted in place.
fn is_empty_dir(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_a_project_without_streak_enabled_defaults_the_sub_tab_to_configure() {
        let dir = tempfile::tempdir().unwrap();
        Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        // Start from the opposite value to prove `open_project` actually
        // resets it, rather than the assertion passing by coincidence.
        app.streak_sub_tab = ui::streak_panel::StreakSubTab::Streak;
        let ctx = egui::Context::default();

        app.open_project(&ctx, dir.path());

        assert_eq!(
            app.streak_sub_tab,
            ui::streak_panel::StreakSubTab::Configure
        );
    }

    #[test]
    fn opening_a_project_with_streak_already_enabled_defaults_the_sub_tab_to_streak() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.set_streak_enabled(true).unwrap();
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();

        app.open_project(&ctx, dir.path());

        assert_eq!(app.streak_sub_tab, ui::streak_panel::StreakSubTab::Streak);
    }

    /// Regression test for the incident this guard exists to prevent:
    /// `open_project` used to unconditionally call `maybe_offer_git_support`,
    /// which pops up a blocking native "Enable Git Support" dialog — during
    /// a `cargo test` run, on the developer's real screen, hanging the test
    /// process until someone dismissed it. `git_prompted` staying `false`
    /// here is a proxy for "the dialog path never ran": the real path always
    /// sets it (see `maybe_offer_git_support`'s `enable_git_support`/
    /// `decline_git_support` calls), so if this assertion ever starts
    /// failing, the dialog is back.
    #[test]
    fn opening_a_project_in_a_test_fixture_never_reaches_the_git_support_prompt() {
        let dir = tempfile::tempdir().unwrap();
        Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        assert!(app.is_test_fixture);
        let ctx = egui::Context::default();

        app.open_project(&ctx, dir.path());

        assert!(!app.project.as_ref().unwrap().meta.git_prompted);
    }

    #[test]
    fn is_empty_dir_is_true_only_for_a_directory_with_no_entries_at_all() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_empty_dir(dir.path()));

        // A dotfile still counts as an entry.
        fs::write(dir.path().join(".gitkeep"), "").unwrap();
        assert!(!is_empty_dir(dir.path()));
    }

    #[test]
    fn create_project_creates_a_named_subfolder_under_the_chosen_location() {
        let location = tempfile::tempdir().unwrap();
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();

        app.create_project(&ctx, location.path(), "My Novel", "blank");

        let root = location.path().join("My Novel");
        assert!(root.is_dir());
        assert_eq!(app.project.as_ref().unwrap().root, root);
    }

    #[test]
    fn create_project_refuses_to_overwrite_an_existing_folder() {
        let location = tempfile::tempdir().unwrap();
        fs::create_dir(location.path().join("My Novel")).unwrap();
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();

        app.create_project(&ctx, location.path(), "My Novel", "blank");

        assert!(app.project.is_none());
    }

    #[test]
    fn close_project_resets_project_state_and_forgets_last_project_path() {
        let dir = tempfile::tempdir().unwrap();
        Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();
        app.open_project(&ctx, dir.path());
        assert!(app.project.is_some());
        assert!(app.settings.last_project_path.is_some());

        app.close_project(&ctx);

        assert!(app.project.is_none());
        assert!(app.settings.last_project_path.is_none());
    }

    #[test]
    fn close_project_is_a_no_op_with_no_project_open() {
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();

        app.close_project(&ctx);

        assert!(app.project.is_none());
    }

    #[test]
    fn close_project_saves_a_dirty_open_document_before_closing() {
        let dir = tempfile::tempdir().unwrap();
        Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();
        app.open_project(&ctx, dir.path());
        let doc_path = dir.path().join("doc.md");
        fs::write(&doc_path, "original").unwrap();
        app.open_document_internal(&doc_path);
        app.editor.buffer = "edited".to_string();
        app.editor.mark_dirty();

        app.close_project(&ctx);

        assert_eq!(fs::read_to_string(&doc_path).unwrap(), "edited");
        assert!(app.project.is_none());
    }

    #[test]
    fn close_project_backs_up_the_project_when_backup_on_close_is_enabled() {
        let dir = tempfile::tempdir().unwrap();
        Project::initialize(dir.path()).unwrap();
        let backup_dir = tempfile::tempdir().unwrap();
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();
        app.open_project(&ctx, dir.path());
        app.settings.backup_enabled = true;
        app.settings.backup_on_close = true;
        app.settings.backup_dir = Some(backup_dir.path().to_path_buf());

        app.close_project(&ctx);

        let backups: Vec<_> = fs::read_dir(backup_dir.path()).unwrap().collect();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn opening_a_project_in_a_test_fixture_never_triggers_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        Project::initialize(dir.path()).unwrap();
        let backup_dir = tempfile::tempdir().unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.settings.backup_enabled = true;
        app.settings.backup_on_open = true;
        app.settings.backup_dir = Some(backup_dir.path().to_path_buf());
        let ctx = egui::Context::default();

        app.open_project(&ctx, dir.path());

        assert_eq!(fs::read_dir(backup_dir.path()).unwrap().count(), 0);
    }

    /// Regression test for the host side of the role-aware collaboration
    /// behavior: a host switching documents must not tear the session down
    /// (see `open_document`'s doc comment) — the joiner's view instead
    /// follows along via the ordinary per-frame diff/sync path.
    #[test]
    fn hosting_and_opening_another_document_keeps_the_session_alive() {
        let dir = tempfile::tempdir().unwrap();
        let doc_path = dir.path().join("doc.md");
        fs::write(&doc_path, "hello").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.collab = Some(CollabSession::test_fixture(CollabRole::Host));

        app.open_document(&doc_path);

        assert!(app.collab.is_some());
        assert_eq!(app.editor.open_path.as_deref(), Some(doc_path.as_path()));
        assert_eq!(app.editor.buffer, "hello");
    }

    #[test]
    fn go_back_and_go_forward_step_through_opened_documents() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let mut app = SmaragdApp::test_fixture();

        app.open_document(&a);
        app.open_document(&b);
        assert_eq!(app.editor.open_path.as_deref(), Some(b.as_path()));

        app.go_back_document();
        assert_eq!(app.editor.open_path.as_deref(), Some(a.as_path()));

        app.go_forward_document();
        assert_eq!(app.editor.open_path.as_deref(), Some(b.as_path()));
    }

    #[test]
    fn going_back_restores_the_cursor_position_last_left_in_that_document() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        fs::write(&a, "hello world").unwrap();
        fs::write(&b, "second document").unwrap();
        let mut app = SmaragdApp::test_fixture();

        app.open_document(&a);
        app.editor.cursor_byte = 6; // simulates the cursor having moved in `a.md`
        app.open_document(&b);
        assert_eq!(app.editor.pending_cursor, Some(0));

        app.go_back_document();

        assert_eq!(app.editor.open_path.as_deref(), Some(a.as_path()));
        assert_eq!(app.editor.pending_cursor, Some(6));
    }

    #[test]
    fn opening_the_same_document_again_does_not_grow_the_back_history() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        fs::write(&a, "a").unwrap();
        let mut app = SmaragdApp::test_fixture();

        app.open_document(&a);
        app.open_document(&a);

        app.go_back_document();
        assert_eq!(app.editor.open_path.as_deref(), Some(a.as_path()));
    }

    #[test]
    fn renaming_the_open_document_keeps_its_history_entry_and_cursor_following_it() {
        let dir = tempfile::tempdir().unwrap();
        Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();
        app.open_project(&ctx, dir.path());
        let project_root = app.project.as_ref().unwrap().root.clone();
        let a = project_root.join("a.md");
        let b = project_root.join("b.md");
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        app.project.as_mut().unwrap().rescan();

        app.open_document(&a);
        app.open_document(&b);
        app.editor.cursor_byte = 1;
        app.rename_node(&b, "renamed");
        let renamed = project_root.join("renamed.md");
        assert_eq!(app.editor.open_path.as_deref(), Some(renamed.as_path()));

        app.go_back_document();
        assert_eq!(app.editor.open_path.as_deref(), Some(a.as_path()));

        app.go_forward_document();
        assert_eq!(app.editor.open_path.as_deref(), Some(renamed.as_path()));
        assert_eq!(app.editor.pending_cursor, Some(1));
    }
}
