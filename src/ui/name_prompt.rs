/// A modal prompting for a single name — used for "New File", "New Folder", and
/// "Rename" from the binder context menu. Owned by the caller (`app.rs`) for the
/// duration of the dialog; what happens on confirm depends on which action opened it,
/// which the caller tracks separately.
pub struct NamePromptState {
    pub title: String,
    pub confirm_label: String,
    pub name: String,
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
        response.request_focus();
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
    });
    outcome
}
