use super::*;

impl SmaragdApp {
    pub(super) fn handle_word_count_event(
        &mut self,
        ctx: &egui::Context,
        event: ui::word_count_panel::WordCountEvent,
    ) {
        use ui::word_count_panel::WordCountEvent;
        match event {
            WordCountEvent::SetDraftTarget(target) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.set_draft_target_words(target)
                {
                    self.push_error_toast(format!("Couldn't save draft target: {err}"));
                }
            }
            WordCountEvent::SetSessionTarget(target) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.set_session_target_words(target)
                {
                    self.push_error_toast(format!("Couldn't save session target: {err}"));
                }
            }
            WordCountEvent::SetScope(scope) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.set_word_count_scope(scope)
                {
                    self.push_error_toast(format!("Couldn't save tracking scope: {err}"));
                }
                self.spawn_word_count_recompute(ctx);
            }
            WordCountEvent::Refresh => self.spawn_word_count_recompute(ctx),
            WordCountEvent::ResetSession => {
                let total = self.word_count.cache;
                if let Some(project) = &mut self.project
                    && let Err(err) = project.reset_session(total)
                {
                    self.push_error_toast(format!("Couldn't reset session: {err}"));
                }
                self.word_count.char_activity = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::WordCountScope;
    use crate::ui::word_count_panel::WordCountEvent;

    #[test]
    fn word_count_event_set_draft_target_persists_on_the_open_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let ctx = egui::Context::default();

        app.handle_word_count_event(&ctx, WordCountEvent::SetDraftTarget(Some(50_000)));

        assert_eq!(
            app.project.as_ref().unwrap().meta.draft_target_words,
            Some(50_000)
        );
    }

    #[test]
    fn word_count_event_set_scope_persists_and_triggers_a_recompute() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let ctx = egui::Context::default();

        app.handle_word_count_event(
            &ctx,
            WordCountEvent::SetScope(WordCountScope::EverythingExceptTrash),
        );

        assert_eq!(
            app.project.as_ref().unwrap().meta.word_count_scope,
            WordCountScope::EverythingExceptTrash
        );
        assert!(app.word_count.pending.is_some());
    }

    #[test]
    fn word_count_event_reset_session_zeroes_char_activity() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.word_count.char_activity = 42;
        let ctx = egui::Context::default();

        app.handle_word_count_event(&ctx, WordCountEvent::ResetSession);

        assert_eq!(app.word_count.char_activity, 0);
    }
}
