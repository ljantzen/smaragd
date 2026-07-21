use crate::editor::EditorState;

/// Renders the document editor. Returns `Some(error message)` if an autosave
/// triggered by focus loss failed, for the caller to surface as a status message.
pub fn show(ui: &mut egui::Ui, editor: &mut EditorState) -> Option<String> {
    let Some(path) = editor.open_path.clone() else {
        ui.label("Select a file from the binder to start editing.");
        return None;
    };

    ui.horizontal(|ui| {
        ui.label(path.display().to_string());
        if editor.dirty {
            ui.label("*");
        }
    });
    ui.separator();

    let response = ui.add_sized(
        ui.available_size(),
        egui::TextEdit::multiline(&mut editor.buffer)
            .desired_width(f32::INFINITY)
            .code_editor(),
    );

    if response.changed() {
        editor.mark_dirty();
    }

    if response.lost_focus()
        && let Err(err) = editor.save()
    {
        return Some(format!("Save failed: {err}"));
    }

    None
}
