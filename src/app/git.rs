use super::*;

/// Which of the two network-bound git actions a `pending_git` background thread is
/// running — needed to know how to react (e.g. rescan on pull) once it finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GitOperation {
    Push,
    Pull,
}

impl GitOperation {
    fn label(self) -> &'static str {
        match self {
            GitOperation::Push => "Push",
            GitOperation::Pull => "Pull",
        }
    }
}

impl SmaragdApp {
    /// If git support is enabled for `project` but its `.git` directory is missing —
    /// deleted outside the app, or `project.json` synced somewhere that never had one
    /// — recreate it. A no-op both when git isn't enabled and when the repo already
    /// exists, so it's safe to call on every project open (not just once at enable
    /// time) — the same "checked and healed independently of when it was set up"
    /// philosophy `Project::ensure_role_folder` uses for the Research/Trash folders.
    pub(super) fn ensure_git_repo(project: &Project) -> Result<(), crate::git::GitError> {
        if project.meta.git_enabled
            && crate::git::is_available()
            && !crate::git::is_repo(&project.root)
        {
            crate::git::init(&project.root)?;
        }
        Ok(())
    }

    /// The one-time "enable git support?" dialog (modeled after the Obsidian Git
    /// plugin), shown at most once per project — see `ProjectMeta::git_prompted`.
    /// A no-op if `git` isn't on `PATH`, or the project's already been asked.
    pub(super) fn maybe_offer_git_support(&mut self) {
        let Some(project) = &self.project else {
            return;
        };
        if project.meta.git_prompted || project.meta.git_enabled || !crate::git::is_available() {
            return;
        }

        let already_repo = crate::git::is_repo(&project.root);
        let description = if already_repo {
            "This project is already a git repository. Enable Smaragd's git integration (commit/push/pull from the Versions menu)?"
        } else {
            "Git was detected on your system. Initialize a git repository for this project and enable version control from the Versions menu?"
        };
        let enable = rfd::MessageDialog::new()
            .set_title("Enable Git Support")
            .set_description(description)
            .set_level(rfd::MessageLevel::Info)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();

        let Some(project) = &mut self.project else {
            return;
        };
        if enable == rfd::MessageDialogResult::Yes {
            if let Err(err) = Self::init_repo_if_needed(&project.root) {
                self.push_error_toast(format!("Couldn't initialize git: {err}"));
                return;
            }
            if let Err(err) = project.enable_git_support() {
                self.push_error_toast(format!("Couldn't save settings: {err}"));
            }
        } else if let Err(err) = project.decline_git_support() {
            self.push_error_toast(format!("Couldn't save settings: {err}"));
        }
    }

    /// `git init` `root` unless it's already a repository. Shared by
    /// `maybe_offer_git_support` and `enable_git_support_manually`, which both need
    /// this exact "become a repo if not already one" step as part of turning git
    /// support on.
    fn init_repo_if_needed(root: &Path) -> Result<(), crate::git::GitError> {
        if !crate::git::is_repo(root) {
            crate::git::init(root)?;
        }
        Ok(())
    }

    /// "Enable Git Support" from the Versions menu or `:git enable` — unlike
    /// `maybe_offer_git_support`, always runs when asked, regardless of whether the
    /// project's already been prompted (this is how a user who declined the one-time
    /// dialog turns it on later).
    pub(super) fn enable_git_support_manually(&mut self) {
        let Some(project) = &self.project else {
            self.push_error_toast("No project open");
            return;
        };
        if !crate::git::is_available() {
            self.push_error_toast("git was not found on this system");
            return;
        }
        if let Err(err) = Self::init_repo_if_needed(&project.root) {
            self.push_error_toast(format!("Couldn't initialize git: {err}"));
            return;
        }

        let Some(project) = &mut self.project else {
            return;
        };
        match project.enable_git_support() {
            Ok(()) => self.set_status_message("Git support enabled"),
            Err(err) => self.push_error_toast(format!("Couldn't save settings: {err}")),
        }
    }

    /// Open the commit-message prompt (the existing name-prompt modal, reused),
    /// pre-filled with a default message. Shared by the Versions menu, the
    /// `GitCommit` shortcut, and `:git commit`/`:git backup` with no inline message.
    pub(super) fn prompt_git_commit(&mut self, push_after: bool) {
        let Some(project) = &self.project else {
            self.push_error_toast("No project open");
            return;
        };
        if !project.meta.git_enabled {
            self.push_error_toast("Git support isn't enabled for this project");
            return;
        }
        self.prompt = Some(PendingPrompt {
            action: PromptAction::GitCommit { push_after },
            state: NamePromptState::new(
                "Commit",
                if push_after {
                    "Commit and Push"
                } else {
                    "Commit"
                },
                "Smaragd backup",
            ),
        });
    }

    pub(super) fn run_git_commit(&mut self, ctx: &egui::Context, message: &str, push_after: bool) {
        let Some(project) = &self.project else {
            self.push_error_toast("No project open");
            return;
        };
        if !project.meta.git_enabled {
            self.push_error_toast("Git support isn't enabled for this project");
            return;
        }
        if let Err(err) = Self::ensure_git_repo(project) {
            self.push_error_toast(format!("Couldn't initialize git: {err}"));
            return;
        }
        match crate::git::commit_all(&project.root, message) {
            Ok(()) => {
                self.set_status_message("Committed");
                if push_after {
                    self.run_git_push(ctx);
                }
            }
            Err(crate::git::GitError::NothingToCommit) => {
                self.set_status_message("Nothing to commit");
            }
            Err(err) => self.push_error_toast(format!("Commit failed: {err}")),
        }
    }

    pub(super) fn run_git_push(&mut self, ctx: &egui::Context) {
        let Some(project) = &self.project else {
            self.push_error_toast("No project open");
            return;
        };
        if !project.meta.git_enabled {
            self.push_error_toast("Git support isn't enabled for this project");
            return;
        }
        self.spawn_git_operation(ctx, GitOperation::Push, project.root.clone());
    }

    /// Pulls, then (once `poll_git_operation` picks up the result) rescans the binder
    /// tree so any files the pull added/removed show up. Deliberately doesn't reload
    /// the currently open document even if its on-disk content changed — that could
    /// silently clobber unsaved local edits; the user can reopen it themselves if they
    /// want the pulled version.
    pub(super) fn run_git_pull(&mut self, ctx: &egui::Context) {
        let Some(project) = &self.project else {
            self.push_error_toast("No project open");
            return;
        };
        if !project.meta.git_enabled {
            self.push_error_toast("Git support isn't enabled for this project");
            return;
        }
        self.spawn_git_operation(ctx, GitOperation::Pull, project.root.clone());
    }

    /// Kick off `operation` against `root` on a background thread — `git push`/`pull`
    /// hit the network and can hang or take a long time, so neither ever runs
    /// synchronously on the UI thread. Refuses to start a second operation while one
    /// is already in flight rather than queuing or racing it. The spawned thread
    /// requests a repaint once it has a result, so `poll_git_operation` (called every
    /// frame) picks it up promptly instead of waiting for unrelated UI activity.
    fn spawn_git_operation(&mut self, ctx: &egui::Context, operation: GitOperation, root: PathBuf) {
        if self.pending_git.is_some() {
            self.push_error_toast("A git operation is already in progress");
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let repaint_ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = match operation {
                GitOperation::Push => crate::git::push(&root),
                GitOperation::Pull => crate::git::pull(&root),
            };
            let _ = sender.send(result);
            repaint_ctx.request_repaint();
        });
        self.set_status_message(format!("{}ing…", operation.label()));
        self.pending_git = Some((operation, receiver));
    }

    /// Check whether the in-flight `pending_git` operation (if any) has finished, and
    /// apply its result — a status message, plus a binder rescan on a successful pull.
    /// Called every frame; a no-op whenever nothing is pending or the background
    /// thread hasn't sent its result yet.
    pub(super) fn poll_git_operation(&mut self, ctx: &egui::Context) {
        let Some((_, receiver)) = &self.pending_git else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                let (operation, _) = self.pending_git.take().expect("checked above");
                self.push_error_toast(format!(
                    "{} failed: background thread panicked",
                    operation.label()
                ));
                return;
            }
        };
        let (operation, _) = self.pending_git.take().expect("checked above");
        match result {
            Ok(()) => {
                if operation == GitOperation::Pull
                    && let Some(project) = &mut self.project
                {
                    project.rescan();
                    self.spawn_word_count_recompute(ctx);
                }
                self.set_status_message(format!("{}ed", operation.label()));
            }
            Err(err) => {
                self.push_error_toast(format!("{} failed: {err}", operation.label()));
            }
        }
    }
}
