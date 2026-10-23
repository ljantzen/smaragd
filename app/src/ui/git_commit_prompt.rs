/// A modal for reviewing/editing a commit message before it's actually
/// committed — opened pre-filled with the rendered `Settings::
/// git_commit_message_template` (see `SmaragdApp::prompt_git_commit`).
/// Unlike `NamePromptState`, this is its own dedicated state rather than a
/// case of the generic name-prompt: a commit message is multi-line (the
/// template's body commonly lists changed files via `{{fileList}}`), so Enter
/// must insert a newline rather than confirm — same reasoning, and the same
/// Ctrl+Enter confirm gesture, as `NotePromptState`.
pub struct GitCommitPromptState {
    pub message: String,
    /// Whether confirming should push afterward — "Commit" vs. "Commit and
    /// Push", both the modal's heading/button label and, once confirmed, an
    /// argument to `SmaragdApp::run_git_commit`.
    pub push_after: bool,
    focus_requested: bool,
}

impl GitCommitPromptState {
    pub fn new(message: String, push_after: bool) -> Self {
        Self {
            message,
            push_after,
            focus_requested: true,
        }
    }
}

pub enum GitCommitPromptOutcome {
    Confirmed(String),
    Cancelled,
}

/// Renders the prompt modal. Returns `Some` once the user confirms
/// (Ctrl+Enter or the Commit/Commit and Push button) or cancels (the Cancel
/// button or Escape) this frame; while `None`, the dialog is still open and
/// awaiting input.
pub fn show(
    ctx: &egui::Context,
    state: &mut GitCommitPromptState,
) -> Option<GitCommitPromptOutcome> {
    let mut outcome = None;
    let label = if state.push_after {
        "Commit and Push"
    } else {
        "Commit"
    };
    egui::Modal::new(egui::Id::new("git_commit_prompt_modal")).show(ctx, |ui| {
        ui.set_min_width(360.0);
        ui.heading(label);
        ui.add_space(8.0);

        let response = ui.add(
            egui::TextEdit::multiline(&mut state.message)
                .desired_rows(8)
                .desired_width(f32::INFINITY),
        );
        if state.focus_requested {
            response.request_focus();
            state.focus_requested = false;
        }
        let confirmed_by_shortcut = response.has_focus()
            && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(label).clicked() || confirmed_by_shortcut {
                outcome = Some(GitCommitPromptOutcome::Confirmed(state.message.clone()));
            }
            if ui.button("Cancel").clicked() {
                outcome = Some(GitCommitPromptOutcome::Cancelled);
            }
        });

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            outcome = Some(GitCommitPromptOutcome::Cancelled);
        }
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_cancels() {
        let ctx = egui::Context::default();
        let mut state = GitCommitPromptState::new("Smaragd backup".to_string(), false);
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut outcome = None;
        crate::egui_test_support::run_ui_and_discard(&ctx, input, |ui| {
            outcome = show(ui.ctx(), &mut state);
        });
        assert!(matches!(outcome, Some(GitCommitPromptOutcome::Cancelled)));
    }

    #[test]
    fn no_outcome_until_a_button_is_clicked() {
        let ctx = egui::Context::default();
        let mut state = GitCommitPromptState::new("Smaragd backup".to_string(), false);
        let mut outcome = None;
        crate::egui_test_support::run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
            outcome = show(ui.ctx(), &mut state);
        });
        assert!(outcome.is_none());
    }
}
