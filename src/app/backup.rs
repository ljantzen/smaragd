use super::*;

/// Which point in the project lifecycle triggered `SmaragdApp::run_backup` —
/// each maps to its own `Settings` toggle, checked in `is_enabled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BackupTrigger {
    Open,
    Close,
    ManualSave,
}

impl BackupTrigger {
    fn is_enabled(self, settings: &crate::settings::Settings) -> bool {
        settings.backup_enabled
            && match self {
                BackupTrigger::Open => settings.backup_on_open,
                BackupTrigger::Close => settings.backup_on_close,
                BackupTrigger::ManualSave => settings.backup_on_manual_save,
            }
    }
}

impl SmaragdApp {
    /// Take a Scrivener-style zipped backup of the current project if
    /// `trigger` is enabled in `Settings` — a silent no-op otherwise, and a
    /// silent no-op (not an error) if no project happens to be open.
    ///
    /// Synchronous, unlike the git push/pull background-thread pattern in
    /// `git.rs`: zipping a project's own (text-only, typically small) folder
    /// is fast, and — especially for the on-close trigger — must actually
    /// finish before the app potentially exits, which a fire-and-forget
    /// background thread on its own OS-scheduled timeslice can't guarantee.
    ///
    /// A failure is reported as an error toast but never blocks whatever the
    /// caller was already doing (opening/closing a project, saving a
    /// document) — a backup is a safety net, not a precondition for those.
    pub(super) fn run_backup(&mut self, trigger: BackupTrigger) {
        if !trigger.is_enabled(&self.settings) {
            return;
        }
        let Some(project) = &self.project else {
            return;
        };
        let Some(backup_dir) = self.settings.resolve_backup_dir() else {
            self.push_error_toast(
                "Couldn't determine a backup directory — set one in Settings > History",
            );
            return;
        };
        let project_name = crate::export::sanitize_filename_component(
            &project
                .root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        match crate::backup::create_backup(&project.root, &backup_dir, &project_name) {
            Ok(_) => {
                let keep = self.settings.resolve_backup_keep_count();
                if let Err(err) =
                    crate::backup::prune_old_backups(&backup_dir, &project_name, keep as usize)
                {
                    self.push_error_toast(format!(
                        "Backup succeeded, but couldn't remove old backups: {err}"
                    ));
                } else {
                    self.set_status_message("Backed up project");
                }
            }
            Err(err) => self.push_error_toast(format!("Backup failed: {err}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backup_files(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn run_backup_is_a_no_op_when_the_trigger_is_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let backup_dir = tempfile::tempdir().unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.settings.backup_enabled = true;
        app.settings.backup_on_close = false;
        app.settings.backup_dir = Some(backup_dir.path().to_path_buf());

        app.run_backup(BackupTrigger::Close);

        assert!(backup_files(backup_dir.path()).is_empty());
    }

    #[test]
    fn run_backup_is_a_no_op_when_the_master_switch_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let backup_dir = tempfile::tempdir().unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.settings.backup_enabled = false;
        app.settings.backup_on_close = true;
        app.settings.backup_dir = Some(backup_dir.path().to_path_buf());

        app.run_backup(BackupTrigger::Close);

        assert!(backup_files(backup_dir.path()).is_empty());
    }

    #[test]
    fn run_backup_writes_a_zip_and_reports_a_status_message() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let backup_dir = tempfile::tempdir().unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.settings.backup_enabled = true;
        app.settings.backup_on_close = true;
        app.settings.backup_dir = Some(backup_dir.path().to_path_buf());

        app.run_backup(BackupTrigger::Close);

        assert_eq!(backup_files(backup_dir.path()).len(), 1);
        assert_eq!(app.status_message.as_deref(), Some("Backed up project"));
    }

    #[test]
    fn run_backup_with_no_project_open_is_a_silent_no_op() {
        let backup_dir = tempfile::tempdir().unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.settings.backup_enabled = true;
        app.settings.backup_on_close = true;
        app.settings.backup_dir = Some(backup_dir.path().to_path_buf());

        app.run_backup(BackupTrigger::Close);

        assert!(backup_files(backup_dir.path()).is_empty());
        assert!(app.toasts.is_empty());
    }
}
