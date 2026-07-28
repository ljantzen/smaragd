//! The template-choice step of "New Project" (File > New Project): shown before
//! the native folder picker, listing every loaded `ProjectTemplate` (Blank
//! first/default) so the user picks a starting scaffold before naming/locating
//! the new project folder.

use egui::{Id, Key};

use crate::project_template::ProjectTemplate;

/// UI state, owned by `app.rs` for the app's lifetime.
#[derive(Default)]
pub struct NewProjectTemplatePromptState {
    pub open: bool,
    selected: usize,
}

impl NewProjectTemplatePromptState {
    pub fn request_open(&mut self) {
        self.open = true;
        self.selected = 0; // Blank, first in the list
    }
}

/// Renders the modal. Returns `Some(template_id)` the frame "Choose" is clicked;
/// `None` while still open or after Cancel/Escape (both close it internally, same
/// convention as `open_document_prompt::show`).
pub fn show(
    ctx: &egui::Context,
    state: &mut NewProjectTemplatePromptState,
    templates: &[ProjectTemplate],
) -> Option<String> {
    if !state.open || templates.is_empty() {
        return None;
    }

    state.selected = state.selected.min(templates.len() - 1);

    let mut outcome = None;
    let mut close = false;
    egui::Modal::new(Id::new("new_project_template_prompt_modal")).show(ctx, |ui| {
        ui.set_min_width(380.0);
        ui.heading("New Project");
        ui.label("Choose a starting template:");
        ui.add_space(8.0);

        for (index, template) in templates.iter().enumerate() {
            ui.radio_value(&mut state.selected, index, &template.label);
            ui.weak(&template.description);
            ui.add_space(4.0);
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Choose").clicked() {
                outcome = templates.get(state.selected).map(|t| t.id.clone());
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });

        if ui.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
    });

    if outcome.is_some() || close {
        state.open = false;
    }
    outcome
}
