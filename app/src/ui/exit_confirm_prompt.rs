/// Shown when the app is about to close while there's unsaved work — the open
/// document has edits (`EditorState::dirty`), or a story card editor modal is
/// open with a draft (`SmaragdApp::card_draft`) — lets the user save, discard,
/// or cancel the close instead of silently losing or silently keeping it.
#[derive(Default)]
pub struct ExitConfirmState {
    pub open: bool,
}

pub enum ExitConfirmOutcome {
    Save,
    Discard,
    Cancel,
}

/// Renders the confirmation modal. Returns `Some` the frame the user picks an
/// option (or presses Escape, treated as Cancel); `None` while still open and
/// awaiting input. Caller is responsible for clearing `state.open` once an
/// outcome comes back.
pub fn show(ctx: &egui::Context, _state: &mut ExitConfirmState) -> Option<ExitConfirmOutcome> {
    let mut outcome = None;
    egui::Modal::new(egui::Id::new("exit_confirm_modal")).show(ctx, |ui| {
        ui.set_min_width(280.0);
        ui.heading("Unsaved Changes");
        ui.add_space(8.0);
        ui.label("You have unsaved changes. Save before exiting?");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                outcome = Some(ExitConfirmOutcome::Save);
            }
            if ui.button("Discard").clicked() {
                outcome = Some(ExitConfirmOutcome::Discard);
            }
            if ui.button("Cancel").clicked() {
                outcome = Some(ExitConfirmOutcome::Cancel);
            }
        });

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            outcome = Some(ExitConfirmOutcome::Cancel);
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
        fn frame(&self, state: &mut ExitConfirmState) -> Option<ExitConfirmOutcome> {
            let mut outcome = None;
            crate::egui_test_support::run_ui_and_discard(
                &self.ctx,
                egui::RawInput::default(),
                |ui| {
                    outcome = show(ui.ctx(), state);
                },
            );
            outcome
        }
    }

    #[test]
    fn no_outcome_until_a_button_is_clicked() {
        let harness = Harness::default();
        let mut state = ExitConfirmState { open: true };
        assert!(harness.frame(&mut state).is_none());
    }
}
