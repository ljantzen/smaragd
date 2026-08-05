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

    /// Like `request_open`, but starts the radio selection on `preferred_id`
    /// instead of Blank — used to steer a genuine first launch (see
    /// `Settings::is_first_launch`) toward a richer starting template than an
    /// empty project. Falls back to Blank if `preferred_id` isn't found (e.g. a
    /// build where it's been renamed or removed).
    pub fn request_open_preferring(&mut self, preferred_id: &str, templates: &[ProjectTemplate]) {
        self.open = true;
        self.selected = templates
            .iter()
            .position(|template| template.id == preferred_id)
            .unwrap_or(0);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn templates() -> Vec<ProjectTemplate> {
        crate::project_template::built_in_templates()
    }

    #[test]
    fn request_open_preferring_selects_the_matching_template() {
        let templates = templates();
        let worldbuilding_index = templates
            .iter()
            .position(|t| t.id == "worldbuilding")
            .expect("worldbuilding is a built-in template");
        let mut state = NewProjectTemplatePromptState::default();

        state.request_open_preferring("worldbuilding", &templates);

        assert!(state.open);
        assert_eq!(state.selected, worldbuilding_index);
    }

    #[test]
    fn request_open_preferring_falls_back_to_blank_for_an_unknown_id() {
        let templates = templates();
        let mut state = NewProjectTemplatePromptState::default();

        state.request_open_preferring("does-not-exist", &templates);

        assert_eq!(state.selected, 0);
    }

    #[test]
    fn request_open_always_resets_to_blank() {
        let templates = templates();
        let mut state = NewProjectTemplatePromptState::default();
        state.request_open_preferring("worldbuilding", &templates);

        state.request_open();

        assert_eq!(state.selected, 0);
    }
}
