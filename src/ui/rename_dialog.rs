use std::path::PathBuf;

/// State for an in-progress rename, owned by the caller (`app.rs`) for the duration of
/// the dialog. `name` starts as the node's current base name (no `.md` extension for a
/// document) and is edited in place.
pub struct RenameState {
    pub path: PathBuf,
    pub name: String,
}

pub enum RenameOutcome {
    Confirmed(String),
    Cancelled,
}

/// Renders the rename modal. Returns `Some` once the user confirms or cancels this
/// frame; while `None`, the dialog is still open and awaiting input.
pub fn show(ctx: &egui::Context, state: &mut RenameState) -> Option<RenameOutcome> {
    let mut outcome = None;
    egui::Modal::new(egui::Id::new("rename_modal")).show(ctx, |ui| {
        ui.set_min_width(240.0);
        ui.heading("Rename");
        ui.add_space(8.0);

        let response = ui.text_edit_singleline(&mut state.name);
        response.request_focus();
        let confirmed_by_enter =
            response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Rename").clicked() || confirmed_by_enter {
                outcome = Some(RenameOutcome::Confirmed(state.name.clone()));
            }
            if ui.button("Cancel").clicked() {
                outcome = Some(RenameOutcome::Cancelled);
            }
        });
    });
    outcome
}
