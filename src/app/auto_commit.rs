use super::*;
use std::time::Duration;
use std::time::Instant;

impl SmaragdApp {
    /// Periodically commit (and, if configured, push) the open project's
    /// changes on its own — see `ProjectMeta::git_auto_commit_enabled`. A
    /// no-op with no project open, when git integration is off (globally or
    /// for this project), when auto-commit itself is off, or while a push/pull
    /// is already running (`pending_git`). Gated to the project's own
    /// `resolve_git_auto_commit_interval_minutes` and schedules the next
    /// repaint for when that's next due, mirroring
    /// `check_external_changes`/`tick_pomodoro`'s own `request_repaint_after`
    /// so it keeps firing even while the app is otherwise idle.
    pub(super) fn maybe_run_auto_commit(&mut self, ctx: &egui::Context) {
        let Some(project) = &self.project else {
            self.auto_commit_last_run = None;
            return;
        };
        if !self.settings.git_integration_enabled()
            || !project.meta.git_enabled
            || !project.meta.git_auto_commit_enabled
            || self.pending_git.is_some()
        {
            return;
        }
        let interval = Duration::from_secs(
            project.meta.resolve_git_auto_commit_interval_minutes() as u64 * 60,
        );
        ctx.request_repaint_after(interval);
        let now = Instant::now();
        if self
            .auto_commit_last_run
            .is_some_and(|last| now.duration_since(last) < interval)
        {
            return;
        }
        self.auto_commit_last_run = Some(now);
        let push_after = project.meta.git_auto_commit_push_enabled;
        let message = self.render_commit_message_for(project);
        self.run_git_commit(ctx, &message, push_after);
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

    fn fixture_with_auto_commit_ready(interval_minutes: u32) -> (tempfile::TempDir, SmaragdApp) {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        init_repo_with_identity(&project.root);
        project.enable_git_support().unwrap();
        project.set_git_auto_commit_enabled(true).unwrap();
        project
            .set_git_auto_commit_interval_minutes(interval_minutes)
            .unwrap();
        crate::git::commit_all(&project.root, "initial commit").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        (dir, app)
    }

    #[test]
    fn is_a_no_op_with_no_project_open() {
        let mut app = SmaragdApp::test_fixture();
        app.auto_commit_last_run = Some(Instant::now());
        let ctx = egui::Context::default();

        app.maybe_run_auto_commit(&ctx);

        assert!(app.auto_commit_last_run.is_none());
    }

    #[test]
    fn is_a_no_op_when_the_project_has_not_turned_auto_commit_on() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        init_repo_with_identity(&project.root);
        project.enable_git_support().unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let ctx = egui::Context::default();

        app.maybe_run_auto_commit(&ctx);

        assert!(app.auto_commit_last_run.is_none());
    }

    #[test]
    fn is_a_no_op_when_global_git_integration_is_disabled() {
        let (_dir, mut app) = fixture_with_auto_commit_ready(0);
        app.settings.git_integration_disabled = true;
        let ctx = egui::Context::default();

        app.maybe_run_auto_commit(&ctx);

        assert!(app.auto_commit_last_run.is_none());
    }

    #[test]
    fn is_a_no_op_while_a_push_or_pull_is_already_in_flight() {
        let (_dir, mut app) = fixture_with_auto_commit_ready(0);
        let (_sender, receiver) = std::sync::mpsc::channel();
        app.pending_git = Some((GitOperation::Push, receiver));
        let ctx = egui::Context::default();

        app.maybe_run_auto_commit(&ctx);

        assert!(app.auto_commit_last_run.is_none());
    }

    #[test]
    fn does_not_fire_again_before_the_interval_elapses() {
        let (_dir, mut app) = fixture_with_auto_commit_ready(60);
        let ctx = egui::Context::default();
        app.maybe_run_auto_commit(&ctx);
        let first_run = app.auto_commit_last_run;
        assert!(first_run.is_some());
        std::fs::write(app.project.as_ref().unwrap().root.join("new.md"), "hi").unwrap();

        app.maybe_run_auto_commit(&ctx);

        assert_eq!(app.auto_commit_last_run, first_run);
    }

    #[test]
    fn commits_dirty_changes_once_the_interval_has_elapsed() {
        // A 0-minute interval resolves to the default (15 min, see
        // `resolve_git_auto_commit_interval_minutes`) unless configured — use a
        // tiny positive value instead, then force `auto_commit_last_run` into
        // the past rather than sleeping for real.
        let (_dir, mut app) = fixture_with_auto_commit_ready(1);
        let root = app.project.as_ref().unwrap().root.clone();
        std::fs::write(root.join("new.md"), "hi").unwrap();
        app.auto_commit_last_run = Some(Instant::now() - Duration::from_secs(120));
        let ctx = egui::Context::default();

        app.maybe_run_auto_commit(&ctx);

        assert!(crate::git::status(&root).unwrap().is_empty());
    }
}
