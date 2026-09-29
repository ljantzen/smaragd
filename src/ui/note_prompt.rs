/// A modal for writing/editing a Note at a specific `(path, line, column)` —
/// opened by `ShortcutAction::AddNoteAtCursor` or a gutter click on the
/// Editor's note column. Owned by the caller (`app.rs`) for the duration of
/// the dialog. Unlike `NamePromptState`, this holds enough of its own target
/// (`path`/`line`/`column`) that the caller doesn't need to track it
/// separately alongside the modal.
pub struct NotePromptState {
    pub path: std::path::PathBuf,
    pub line: usize,
    pub column: usize,
    pub text: String,
    /// Whether a note already existed at this exact position when the prompt
    /// was opened — controls whether "Delete" is shown at all (deleting a
    /// note that was never saved makes no sense) and whether cancelling
    /// should be a true no-op vs. simply not creating anything.
    existed: bool,
    focus_requested: bool,
}

impl NotePromptState {
    pub fn new(path: std::path::PathBuf, line: usize, column: usize, text: String) -> Self {
        Self {
            path,
            line,
            column,
            existed: !text.is_empty(),
            text,
            focus_requested: true,
        }
    }
}

pub enum NotePromptOutcome {
    Saved(String),
    Deleted,
    Cancelled,
}

/// Renders the prompt modal. Returns `Some` once the user confirms (Ctrl+Enter or the
/// "Save" button), deletes, or cancels (the "Cancel" button or Escape) this frame;
/// while `None`, the dialog is still open and awaiting input.
pub fn show(ctx: &egui::Context, state: &mut NotePromptState) -> Option<NotePromptOutcome> {
    let mut outcome = None;
    egui::Modal::new(egui::Id::new("note_prompt_modal")).show(ctx, |ui| {
        ui.set_min_width(320.0);
        ui.heading("Note");
        ui.add_space(8.0);

        // Multiline, unlike `name_prompt`'s single line — a note is more than
        // one word — so Enter must insert a newline rather than confirm; the
        // confirm gesture is Ctrl+Enter instead (checked below), matching the
        // same one-shot `focus_requested` pattern `name_prompt`/
        // `command_prompt`/`find_replace_panel` already use for a text field
        // that surrenders focus once, not on every frame it merely lacks it.
        let response = ui.add(
            egui::TextEdit::multiline(&mut state.text)
                .desired_rows(4)
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
            if ui.button("Save").clicked() || confirmed_by_shortcut {
                outcome = Some(NotePromptOutcome::Saved(state.text.clone()));
            }
            if state.existed && ui.button("Delete").clicked() {
                outcome = Some(NotePromptOutcome::Deleted);
            }
            if ui.button("Cancel").clicked() {
                outcome = Some(NotePromptOutcome::Cancelled);
            }
        });

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            outcome = Some(NotePromptOutcome::Cancelled);
        }
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_with_empty_text_does_not_mark_the_note_as_already_existing() {
        let state = NotePromptState::new(std::path::PathBuf::from("scene.md"), 3, 0, String::new());
        assert!(!state.existed);
    }

    #[test]
    fn new_with_text_marks_the_note_as_already_existing() {
        let state = NotePromptState::new(
            std::path::PathBuf::from("scene.md"),
            3,
            0,
            "already here".to_string(),
        );
        assert!(state.existed);
    }

    #[test]
    fn escape_cancels() {
        let ctx = egui::Context::default();
        let mut state =
            NotePromptState::new(std::path::PathBuf::from("scene.md"), 1, 0, String::new());
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
        assert!(matches!(outcome, Some(NotePromptOutcome::Cancelled)));
    }
}
