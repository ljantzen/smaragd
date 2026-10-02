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

/// How many recent commits `refresh_git_log` fetches — the Version Activity
/// panel shows a short history, not the whole log.
const GIT_LOG_LIMIT: usize = 20;

/// Bounds `SmaragdApp::git_activity_log`'s length — the most recent actions
/// only; `git_log_cache` (real git history) has no such limit's worth of
/// concern since git itself is the source of truth there.
const GIT_ACTIVITY_LOG_LIMIT: usize = 50;

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl SmaragdApp {
    /// Append `message` to `git_activity_log`, newest first, trimmed to
    /// `GIT_ACTIVITY_LOG_LIMIT` — called alongside every `set_status_message`/
    /// `push_error_toast` a git action already triggers, so the Version
    /// Activity panel's history always agrees with what the status bar/toasts
    /// showed at the time.
    pub(super) fn record_git_activity(
        &mut self,
        message: impl Into<String>,
        outcome: crate::git::GitActivityOutcome,
    ) {
        self.record_git_activity_with_files(message, outcome, Vec::new());
    }

    /// [`Self::record_git_activity`], additionally attaching the files a
    /// push/pull touched — see `GitActivityEntry::files`.
    pub(super) fn record_git_activity_with_files(
        &mut self,
        message: impl Into<String>,
        outcome: crate::git::GitActivityOutcome,
        files: Vec<String>,
    ) {
        self.git_activity_log
            .push_front(crate::git::GitActivityEntry {
                at_unix: unix_now(),
                message: message.into(),
                outcome,
                files,
            });
        self.git_activity_log.truncate(GIT_ACTIVITY_LOG_LIMIT);
    }

    /// Refresh `git_log_cache` from `git log`, or clear it if git integration
    /// is off (globally or for this project) or no project is open — same
    /// shape and call sites as `refresh_git_dirty_paths`.
    pub(super) fn refresh_git_log(&mut self) {
        let log = self.project.as_ref().and_then(|project| {
            (self.settings.git_integration_enabled() && project.meta.git_enabled)
                .then(|| crate::git::log(&project.root, GIT_LOG_LIMIT))
        });
        self.git_log_cache = log.unwrap_or_default();
    }
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

    /// The one-time "enable git support?", shown at most once per project, see `ProjectMeta::git_prompted`.
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
            Ok(()) => {
                self.set_status_message("Git support enabled");
                self.refresh_git_dirty_paths();
                self.refresh_git_log();
                self.record_git_activity(
                    "Git support enabled",
                    crate::git::GitActivityOutcome::Success,
                );
            }
            Err(err) => self.push_error_toast(format!("Couldn't save settings: {err}")),
        }
    }

    /// Refresh `git_dirty_paths` from `git status`, or clear it if git
    /// integration is off (globally or for this project) or no project is
    /// open — see that field's doc comment for why every git-touching call
    /// site funnels through here rather than only ever setting it on
    /// success. Failures are swallowed (not reported as a toast): this is a
    /// best-effort visual enhancement, not a user-triggered action with its
    /// own outcome to report — an actual git problem still surfaces the next
    /// time the user tries to commit/push/pull.
    pub(super) fn refresh_git_dirty_paths(&mut self) {
        let dirty = self.project.as_ref().and_then(|project| {
            (self.settings.git_integration_enabled() && project.meta.git_enabled)
                .then(|| crate::git::status(&project.root).ok())
                .flatten()
        });
        self.git_dirty_paths = dirty.unwrap_or_default();
    }

    /// Renders the configured commit-message template (`Settings::
    /// git_commit_message_template`) against `project`'s current git state —
    /// shared by `prompt_git_commit` (the manual Commit prompt's pre-fill) and
    /// `maybe_run_auto_commit`, so a manual commit and an automatic one are
    /// never worded differently for the same template.
    pub(super) fn render_commit_message_for(&self, project: &Project) -> String {
        let template = self.settings.resolve_git_commit_message_template();
        let date = crate::templates::format_date(&self.settings.template_date_format);
        let time = crate::git::format_commit_time();
        let num_files = self.git_dirty_paths.len();
        let diff = crate::git::diff_stat(&project.root).ok();
        let files = crate::git::changed_files(&project.root).unwrap_or_default();
        crate::git::render_commit_message(
            &template,
            &crate::git::CommitContext {
                date: &date,
                time: &time,
                num_files,
                diff: diff.as_ref(),
                files: &files,
                root: &project.root,
            },
        )
    }

    /// Open the commit-message prompt (`ui::git_commit_prompt`), pre-filled
    /// via `render_commit_message_for`. Shared by the Versions menu, the
    /// `GitCommit`/`GitCommitAndPush` shortcuts, and `:git commit`/`:git
    /// backup` with no inline message.
    pub(super) fn prompt_git_commit(&mut self, push_after: bool) {
        let Some(project) = &self.project else {
            self.push_error_toast("No project open");
            return;
        };
        if !project.meta.git_enabled {
            self.push_error_toast("Git support isn't enabled for this project");
            return;
        }
        let message = self.render_commit_message_for(project);
        self.git_commit_prompt = Some(ui::git_commit_prompt::GitCommitPromptState::new(
            message, push_after,
        ));
    }

    /// Resolve the Commit prompt's outcome: commit with the (edited) message
    /// on confirm, or do nothing on cancel.
    pub(super) fn handle_git_commit_prompt_outcome(
        &mut self,
        ctx: &egui::Context,
        outcome: ui::git_commit_prompt::GitCommitPromptOutcome,
    ) {
        let Some(prompt) = self.git_commit_prompt.take() else {
            return;
        };
        if let ui::git_commit_prompt::GitCommitPromptOutcome::Confirmed(message) = outcome {
            self.run_git_commit(ctx, &message, prompt.push_after);
        }
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
                self.refresh_git_dirty_paths();
                self.refresh_git_log();
                self.record_git_activity("Committed", crate::git::GitActivityOutcome::Success);
                if push_after {
                    self.run_git_push(ctx);
                }
            }
            Err(crate::git::GitError::NothingToCommit) => {
                self.set_status_message("Nothing to commit");
                self.record_git_activity(
                    "Nothing to commit",
                    crate::git::GitActivityOutcome::Neutral,
                );
            }
            Err(err) => {
                let message = format!("Commit failed: {err}");
                self.push_error_toast(message.clone());
                self.record_git_activity(message, crate::git::GitActivityOutcome::Error);
            }
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
    /// tree so any files the pull added/removed show up. Doesn't itself reload the
    /// currently open document even if its on-disk content changed — that's handled
    /// uniformly (for a pull or any other external write) by
    /// `external_watch::check_external_changes`'s own periodic check, which reloads
    /// it automatically if there's nothing unsaved to lose, or prompts instead of
    /// silently clobbering either version if there is.
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
                let message = format!("{} failed: background thread panicked", operation.label());
                self.push_error_toast(message.clone());
                self.record_git_activity(message, crate::git::GitActivityOutcome::Error);
                return;
            }
        };
        let (operation, _) = self.pending_git.take().expect("checked above");
        match result {
            Ok(files) => {
                if operation == GitOperation::Pull
                    && let Some(project) = &mut self.project
                {
                    project.rescan();
                    self.spawn_word_count_recompute(ctx);
                }
                self.refresh_git_dirty_paths();
                self.refresh_git_log();
                let message = format!("{}ed", operation.label());
                self.set_status_message(message.clone());
                let file_lines = self
                    .project
                    .as_ref()
                    .map(|project| crate::git::file_list_lines(&project.root, &files))
                    .unwrap_or_default();
                self.record_git_activity_with_files(
                    message,
                    crate::git::GitActivityOutcome::Success,
                    file_lines,
                );
            }
            Err(err) => {
                let message = format!("{} failed: {err}", operation.label());
                self.push_error_toast(message.clone());
                self.record_git_activity(message, crate::git::GitActivityOutcome::Error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo_with_identity(root: &Path) {
        crate::git::init(root).unwrap();
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

    #[test]
    fn refresh_git_dirty_paths_picks_up_an_untracked_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        init_repo_with_identity(&project.root);
        project.enable_git_support().unwrap();
        // Commit the initial `.smaragd/project.json` first so only `new.md`
        // below shows up as dirty, isolating what this test actually checks.
        crate::git::commit_all(&project.root, "initial commit").unwrap();
        std::fs::write(dir.path().join("new.md"), "brand new").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);

        app.refresh_git_dirty_paths();

        assert_eq!(app.git_dirty_paths, [dir.path().join("new.md")].into());
    }

    #[test]
    fn refresh_git_dirty_paths_is_empty_when_global_git_integration_is_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        init_repo_with_identity(&project.root);
        project.enable_git_support().unwrap();
        std::fs::write(dir.path().join("new.md"), "brand new").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.settings.git_integration_disabled = true;

        app.refresh_git_dirty_paths();

        assert!(app.git_dirty_paths.is_empty());
    }

    #[test]
    fn refresh_git_dirty_paths_is_empty_when_this_project_has_not_enabled_git() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        init_repo_with_identity(&project.root);
        // Deliberately not calling `enable_git_support` — a repo can exist on
        // disk without smaragd's own per-project flag being on.
        std::fs::write(dir.path().join("new.md"), "brand new").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);

        app.refresh_git_dirty_paths();

        assert!(app.git_dirty_paths.is_empty());
    }

    #[test]
    fn refresh_git_dirty_paths_clears_a_stale_set_with_no_project_open() {
        let mut app = SmaragdApp::test_fixture();
        app.git_dirty_paths = [PathBuf::from("/stale/path.md")].into();

        app.refresh_git_dirty_paths();

        assert!(app.git_dirty_paths.is_empty());
    }

    #[test]
    fn refresh_git_log_picks_up_commits() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        init_repo_with_identity(&project.root);
        project.enable_git_support().unwrap();
        crate::git::commit_all(&project.root, "initial commit").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);

        app.refresh_git_log();

        assert_eq!(app.git_log_cache.len(), 1);
        assert_eq!(app.git_log_cache[0].subject, "initial commit");
    }

    #[test]
    fn refresh_git_log_is_empty_when_global_git_integration_is_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        init_repo_with_identity(&project.root);
        project.enable_git_support().unwrap();
        crate::git::commit_all(&project.root, "initial commit").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.settings.git_integration_disabled = true;

        app.refresh_git_log();

        assert!(app.git_log_cache.is_empty());
    }

    #[test]
    fn record_git_activity_prepends_and_trims_to_the_limit() {
        let mut app = SmaragdApp::test_fixture();

        for n in 0..(GIT_ACTIVITY_LOG_LIMIT + 5) {
            app.record_git_activity(
                format!("entry {n}"),
                crate::git::GitActivityOutcome::Success,
            );
        }

        assert_eq!(app.git_activity_log.len(), GIT_ACTIVITY_LOG_LIMIT);
        assert_eq!(
            app.git_activity_log.front().unwrap().message,
            format!("entry {}", GIT_ACTIVITY_LOG_LIMIT + 4)
        );
    }

    #[test]
    fn prompt_git_commit_pre_fills_the_configured_template() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        init_repo_with_identity(&project.root);
        project.enable_git_support().unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.settings.git_commit_message_template = Some("Backup at {{time}}".to_string());

        app.prompt_git_commit(false);

        let message = &app.git_commit_prompt.as_ref().unwrap().message;
        assert!(
            message.starts_with("Backup at ") && !message.contains("{{"),
            "expected the template rendered, got {message:?}"
        );
    }

    #[test]
    fn prompt_git_commit_lists_dirty_files_via_the_default_template() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        init_repo_with_identity(&project.root);
        project.enable_git_support().unwrap();
        crate::git::commit_all(&project.root, "initial commit").unwrap();
        std::fs::write(project.root.join("new.md"), "hi").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);

        app.prompt_git_commit(false);

        let message = &app.git_commit_prompt.as_ref().unwrap().message;
        assert!(
            message.contains("A new.md"),
            "expected the default template's file list, got {message:?}"
        );
    }

    #[test]
    fn a_successful_commit_is_recorded_as_activity() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        init_repo_with_identity(&project.root);
        project.enable_git_support().unwrap();
        std::fs::write(dir.path().join("new.md"), "hi").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let ctx = egui::Context::default();

        app.run_git_commit(&ctx, "a commit", false);

        assert_eq!(app.git_activity_log.len(), 1);
        assert_eq!(app.git_activity_log[0].message, "Committed");
        assert_eq!(
            app.git_activity_log[0].outcome,
            crate::git::GitActivityOutcome::Success
        );
        assert_eq!(app.git_log_cache.len(), 1);
        assert_eq!(app.git_log_cache[0].subject, "a commit");
    }

    #[test]
    fn a_no_op_commit_is_recorded_as_neutral_activity() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        init_repo_with_identity(&project.root);
        project.enable_git_support().unwrap();
        crate::git::commit_all(&project.root, "initial commit").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let ctx = egui::Context::default();

        app.run_git_commit(&ctx, "nothing changed", false);

        assert_eq!(app.git_activity_log.len(), 1);
        assert_eq!(app.git_activity_log[0].message, "Nothing to commit");
        assert_eq!(
            app.git_activity_log[0].outcome,
            crate::git::GitActivityOutcome::Neutral
        );
    }
}
