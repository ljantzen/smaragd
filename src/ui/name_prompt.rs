/// A modal prompting for a single name — used for "New File", "New Folder", and
/// "Rename" from the binder context menu. Owned by the caller (`app.rs`) for the
/// duration of the dialog; what happens on confirm depends on which action opened it,
/// which the caller tracks separately.
pub struct NamePromptState {
    pub title: String,
    pub confirm_label: String,
    pub name: String,
    focus_requested: bool,
}

impl NamePromptState {
    pub fn new(
        title: impl Into<String>,
        confirm_label: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            confirm_label: confirm_label.into(),
            name: name.into(),
            focus_requested: true,
        }
    }
}

pub enum NamePromptOutcome {
    Confirmed(String),
    Cancelled,
}

/// Renders the prompt modal. Returns `Some` once the user confirms (via the button or
/// pressing Enter in the text field) or cancels this frame; while `None`, the dialog
/// is still open and awaiting input.
pub fn show(ctx: &egui::Context, state: &mut NamePromptState) -> Option<NamePromptOutcome> {
    let mut outcome = None;
    egui::Modal::new(egui::Id::new("name_prompt_modal")).show(ctx, |ui| {
        ui.set_min_width(240.0);
        ui.heading(&state.title);
        ui.add_space(8.0);

        let response = ui.text_edit_singleline(&mut state.name);
        // One-shot, not a per-frame `!has_focus()` guard: `TextEdit` itself
        // surrenders its own focus the instant it processes Enter (to signal
        // `lost_focus()` below); re-requesting focus on every frame the field
        // merely *lacks* focus would immediately reclaim it again that same
        // frame, undoing the surrender before it's ever observed and making
        // Enter un-confirmable. See `command_prompt.rs`/`find_replace_panel.rs`
        // for the same one-shot pattern.
        if state.focus_requested {
            response.request_focus();
            state.focus_requested = false;
        }
        let confirmed_by_enter =
            response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(&state.confirm_label).clicked() || confirmed_by_enter {
                outcome = Some(NamePromptOutcome::Confirmed(state.name.clone()));
            }
            if ui.button("Cancel").clicked() {
                outcome = Some(NamePromptOutcome::Cancelled);
            }
        });

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            outcome = Some(NamePromptOutcome::Cancelled);
        }
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Harness {
        ctx: egui::Context,
    }

    impl Harness {
        fn frame(
            &self,
            state: &mut NamePromptState,
            events: Vec<egui::Event>,
        ) -> Option<NamePromptOutcome> {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut outcome = None;
            let _ = self.ctx.run_ui(input, |ui| {
                outcome = show(ui.ctx(), state);
            });
            outcome
        }

        fn idle(&self, state: &mut NamePromptState) {
            self.frame(state, vec![]);
        }

        fn press_enter(&self, state: &mut NamePromptState) -> Option<NamePromptOutcome> {
            self.frame(
                state,
                vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            )
        }
    }

    #[test]
    fn enter_confirms_once_the_text_field_has_settled_into_focus() {
        let harness = Harness::default();
        let mut state = NamePromptState::new("New File", "Create", "scene6");
        // First frame grants focus via `request_focus`; a second (idle) frame lets
        // that settle before a keypress — the same one-frame gap `TextEdit` itself
        // has, mirrored by `binder_panel.rs`'s test harness for the same reason.
        harness.idle(&mut state);
        harness.idle(&mut state);
        let outcome = harness.press_enter(&mut state);
        assert!(matches!(
            outcome,
            Some(NamePromptOutcome::Confirmed(name)) if name == "scene6"
        ));
    }
}
