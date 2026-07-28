use crate::export::BookMeta;
use crate::export::style::TypesetStyle;

/// Which button in the export modal was pressed this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportAction {
    Docx,
    Epub,
    Pdf,
    ReloadStyles,
    Close,
}

/// Renders the Export modal: the folder being compiled (read-only), editable
/// Title/Author fields, a Style picker (built-in + loaded custom
/// `TypesetStyle`s, selection-only — same files-only-authoring convention as
/// `View > Theme`, no in-app style editor), and the action buttons. Returns
/// `Some` the frame a button is pressed; the caller (`app.rs`) decides what
/// happens next (opening a save dialog, persisting the meta/style fields,
/// dismissing the modal).
pub fn show(
    ctx: &egui::Context,
    source_label: &str,
    meta: &mut BookMeta,
    style_id: &mut String,
    styles: &[TypesetStyle],
) -> Option<ExportAction> {
    let mut action = None;
    egui::Modal::new(egui::Id::new("export_modal")).show(ctx, |ui| {
        ui.set_min_width(320.0);
        ui.heading("Export");
        ui.label(format!("Compiling: {source_label}"));
        ui.add_space(8.0);

        egui::Grid::new("export_meta_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Title");
                ui.text_edit_singleline(&mut meta.title);
                ui.end_row();

                ui.label("Author");
                ui.text_edit_singleline(&mut meta.author);
                ui.end_row();

                ui.label("Style");
                let current_label = crate::export::style::find(styles, style_id)
                    .map(|s| s.label.as_str())
                    .unwrap_or("(none)");
                egui::ComboBox::new("export_style_combo", "")
                    .selected_text(current_label)
                    .show_ui(ui, |ui| {
                        for style in styles {
                            ui.selectable_value(style_id, style.id.clone(), &style.label);
                        }
                    });
                ui.end_row();
            });
        if ui.small_button("Reload Custom Styles").clicked() {
            action = Some(ExportAction::ReloadStyles);
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Export as DOCX…").clicked() {
                action = Some(ExportAction::Docx);
            }
            if ui.button("Export as EPUB…").clicked() {
                action = Some(ExportAction::Epub);
            }
            if ui.button("Export as Print PDF…").clicked() {
                action = Some(ExportAction::Pdf);
            }
            if ui.button("Close").clicked() {
                action = Some(ExportAction::Close);
            }
        });

        // No Enter-to-confirm here, unlike other modals: there are three
        // non-equivalent export actions (DOCX/EPUB/PDF) and no single obvious
        // default among them — binding Enter to one would risk kicking off the
        // wrong export format from a stray keypress while editing Title/Author.
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            action = Some(ExportAction::Close);
        }
    });
    action
}
