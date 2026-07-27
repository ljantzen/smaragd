use crate::project::BacklinkEntry;

/// Outcomes of user interaction with the backlinks panel, handled by the caller
/// (`app.rs`) rather than mutated here — keeps this module a pure rendering layer,
/// matching `BinderEvent`/`CorkboardEvent`.
pub enum BacklinksEvent {
    OpenDocument(std::path::PathBuf),
    /// The manual "Refresh" button was clicked — recompute now regardless of
    /// whether the open document has actually changed since the last scan.
    Refresh,
}

/// Renders the list of documents linking to the currently-open one. `open_path` is
/// only used to distinguish "no document open" from "document open, zero
/// backlinks" — `backlinks` itself is already scoped to whatever document is open by
/// the caller (see `app.rs`'s `recompute_backlinks`); this module never calls into
/// `Project` itself.
pub fn show(
    ui: &mut egui::Ui,
    open_path: Option<&std::path::Path>,
    backlinks: &[BacklinkEntry],
) -> Option<BacklinksEvent> {
    let mut event = None;

    ui.horizontal(|ui| {
        ui.heading("Backlinks");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Refresh").clicked() {
                event = Some(BacklinksEvent::Refresh);
            }
        });
    });
    ui.separator();

    if open_path.is_none() {
        ui.label("Open a document to see what links to it.");
        return event;
    }
    if backlinks.is_empty() {
        ui.label("No other document links here yet.");
        return event;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        // Group consecutive same-source entries under one clickable title, one
        // snippet row per occurrence beneath it (Obsidian-style "N links" grouping)
        // without needing a grouped data structure — `Project::backlinks` already
        // emits entries in document-tree order, so a shared source's occurrences
        // are already adjacent.
        let mut index = 0;
        while index < backlinks.len() {
            let source = &backlinks[index];
            let run_end = backlinks[index..]
                .iter()
                .position(|entry| entry.source_path != source.source_path)
                .map_or(backlinks.len(), |offset| index + offset);

            if ui.link(&source.source_title).clicked() {
                event = Some(BacklinksEvent::OpenDocument(source.source_path.clone()));
            }
            for entry in &backlinks[index..run_end] {
                ui.label(egui::RichText::new(&entry.snippet).weak());
            }
            ui.add_space(6.0);

            index = run_end;
        }
    });

    event
}
