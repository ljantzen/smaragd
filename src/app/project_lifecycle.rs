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
        self.clear_status_message();
        self.settings.last_project_path = Some(path.to_path_buf());
        self.settings.record_recent_project(path);
        self.persist_settings();
        self.maybe_offer_git_support();
        if let Some(project) = &self.project
            && let Err(err) = Self::ensure_git_repo(project)
        {
            self.push_error_toast(format!("Couldn't initialize git: {err}"));
        }
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
    /// been chosen).
    pub(super) fn start_new_project(&mut self) {
        self.new_project_template_prompt.request_open();
    }

    /// Continue "New Project" once `template_id` has been chosen: pick a parent
    /// folder via the native folder picker, then prompt for the new project's name
    /// via the existing name-prompt modal.
    pub(super) fn start_new_project_with_template(&mut self, template_id: String) {
        let Some(location) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
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
        match Project::initialize(&root) {
            Ok(mut project) => {
                // An id that no longer resolves (e.g. a custom template deleted
                // between picker and confirm) is treated as "no scaffolding" rather
                // than a hard error — the same fallback Blank itself produces.
                let template_error =
                    crate::project_template::find(&self.project_templates, template_id)
                        .and_then(|template| template.apply(&mut project).err());
                // `set_project` unconditionally clears `status_message`, so a
                // template-apply error must be recorded after it runs, not before.
                self.set_project(ctx, project, &root);
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

    /// Open `path` as a genuine switch to a different document — ends any
    /// active collaboration session first (see `CollabSession`'s module
    /// doc). Every call site *except* `rename_node`'s post-rename reopen
    /// goes through here: a rename keeps the same logical document open
    /// (see `open_document_internal`), so it must not tear down a session
    /// scoped to it.
    pub(super) fn open_document(&mut self, path: &Path) {
        if self.collab.is_some() {
            self.end_collab_session("Collaboration session ended: switched documents");
        }
        self.open_document_internal(path);
    }

    /// The actual open — no collaboration-session teardown. Only
    /// `open_document` and `rename_node` (reopening the same document under
    /// its new name) should call this directly.
    pub(super) fn open_document_internal(&mut self, path: &Path) {
        match self.editor.open(path) {
            Ok(()) => self.selected_path = Some(path.to_path_buf()),
            Err(err) => {
                self.push_error_toast(format!("Couldn't open {}: {err}", path.display()));
            }
        }
    }

    /// Close the currently open document (silently autosaving first if dirty — same
    /// convention as `open_document`/`rename_node`, no discard/cancel prompt).
    pub(super) fn close_document(&mut self, ctx: &egui::Context) {
        if self.collab.is_some() {
            self.end_collab_session("Collaboration session ended: document closed");
        }
        if let Err(err) = self.editor.close() {
            self.push_error_toast(format!("Couldn't save before closing: {err}"));
            return;
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
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't move {}: {err}", path.display()));
            }
        }
    }

    /// Same rebase-selected/open-path logic as `move_item`, for a document or
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
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't move {}: {err}", path.display()));
            }
        }
    }
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
}
