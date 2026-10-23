use super::*;

impl SmaragdApp {
    /// Open today's daily note, creating it (optionally from the configured
    /// template) if it doesn't exist yet — see `Project::open_or_create_daily_note`.
    /// Same guard/error-reporting shape as `ShortcutAction::OpenProjectSettings`.
    /// A no-op (with a toast) while `daily_notes_enabled` is off — the feature is
    /// off by default, so this is the common case for a project that's never
    /// turned it on, not an edge case.
    pub(super) fn open_daily_note(&mut self) {
        if self.project.is_none() {
            self.push_error_toast("No project open");
            return;
        }
        if !self
            .project
            .as_ref()
            .is_some_and(|project| project.daily_notes_enabled())
        {
            self.push_error_toast(
                "Daily Notes is off — enable it in Project Settings > Daily Notes",
            );
            return;
        }
        let Some(project) = &mut self.project else {
            return;
        };
        match project.open_or_create_daily_note(
            &self.settings.template_date_format,
            &self.settings.template_time_format,
        ) {
            Ok((path, cursor)) => {
                self.open_document(&path);
                if let Some(byte) = cursor {
                    let len = self.editor.buffer().len();
                    self.editor.set_pending_cursor(byte.min(len));
                }
            }
            Err(err) => self.push_error_toast(format!("Couldn't open today's note: {err}")),
        }
    }
}
