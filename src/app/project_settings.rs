use super::*;

use ui::project_settings_panel::ProjectSettingsEvent;

impl SmaragdApp {
    /// Applies a control changed in the Project Settings dialog
    /// (`ui::project_settings_panel`) to the open project — each event maps to
    /// one `Project::set_*` call, which persists to `project.json`
    /// immediately, same as every other per-project setting. A no-op with no
    /// project open (the dialog can only be reached with one, but a project
    /// could in principle close on the very frame an event was emitted).
    pub(super) fn handle_project_settings_event(&mut self, event: ProjectSettingsEvent) {
        let Some(project) = self.project.as_mut() else {
            return;
        };
        let result = match event {
            ProjectSettingsEvent::SetGitAutoCommitEnabled(on) => {
                project.set_git_auto_commit_enabled(on)
            }
            ProjectSettingsEvent::SetGitAutoCommitIntervalMinutes(minutes) => {
                project.set_git_auto_commit_interval_minutes(minutes)
            }
            ProjectSettingsEvent::SetGitAutoCommitPushEnabled(on) => {
                project.set_git_auto_commit_push_enabled(on)
            }
            ProjectSettingsEvent::SetSyncFiles(on) => project.set_sync_files(on),
            ProjectSettingsEvent::SetBinderColorMode(mode) => project.set_binder_color_mode(mode),
        };
        match result {
            Ok(()) => {
                // Sync reads `ProjectMeta::sync_files` fresh from the project every
                // pass, so this only needs to wake the runner up sooner — same
                // effect the removed `SyncPanelEvent::SetSyncFiles` handler had.
                if matches!(event, ProjectSettingsEvent::SetSyncFiles(_)) {
                    self.sync_now();
                }
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't save the project settings: {err}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_project() -> (tempfile::TempDir, SmaragdApp) {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        (dir, app)
    }

    #[test]
    fn sets_the_auto_commit_interval_and_persists_it() {
        let (dir, mut app) = app_with_project();

        app.handle_project_settings_event(ProjectSettingsEvent::SetGitAutoCommitIntervalMinutes(
            45,
        ));

        assert_eq!(
            app.project
                .as_ref()
                .unwrap()
                .meta
                .git_auto_commit_interval_minutes,
            45
        );
        assert_eq!(
            Project::load_from_folder(dir.path())
                .unwrap()
                .meta
                .git_auto_commit_interval_minutes,
            45
        );
    }

    #[test]
    fn sets_the_binder_color_mode() {
        let (_dir, mut app) = app_with_project();

        app.handle_project_settings_event(ProjectSettingsEvent::SetBinderColorMode(
            BinderColorMode::Pov,
        ));

        assert_eq!(
            app.project.as_ref().unwrap().meta.binder_color_mode,
            BinderColorMode::Pov
        );
    }

    #[test]
    fn setting_sync_files_persists_it_and_wakes_sync() {
        let (dir, mut app) = app_with_project();

        app.handle_project_settings_event(ProjectSettingsEvent::SetSyncFiles(true));

        assert!(app.project.as_ref().unwrap().meta.sync_files);
        assert!(
            Project::load_from_folder(dir.path())
                .unwrap()
                .meta
                .sync_files
        );
    }

    #[test]
    fn is_a_no_op_with_no_project_open() {
        let mut app = SmaragdApp::test_fixture();

        app.handle_project_settings_event(ProjectSettingsEvent::SetGitAutoCommitEnabled(true));

        assert!(app.project.is_none());
    }
}
