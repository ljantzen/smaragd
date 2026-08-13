use crate::project::Project;

/// Outcomes of user interaction with the Bookmarks panel, handled by the
/// caller (`app.rs`) rather than mutated here — same pure-rendering-layer
/// convention `TagsEvent`/`BacklinksEvent` already use.
pub enum BookmarksEvent {
    Open {
        path: std::path::PathBuf,
        line: usize,
    },
    Delete(uuid::Uuid),
}

/// Renders every bookmark in the whole project — not just the currently
/// open document, see `Project::resolved_bookmarks` — as one flat,
/// document-then-line-ordered list. Recomputed fresh every frame rather
/// than cached (like `corkboard_panel::show` reading `project.meta`
/// directly): the list is user-curated and expected to stay small, unlike
/// Tags/Backlinks' whole-vault scans.
pub fn show(ui: &mut egui::Ui, project: &Project) -> Option<BookmarksEvent> {
    let mut event = None;
    ui.heading("Bookmarks");
    ui.separator();

    let bookmarks = project.resolved_bookmarks();
    if bookmarks.is_empty() {
        ui.label(
            "No bookmarks yet — toggle one from the editor's line-number \
             gutter, or its keyboard shortcut.",
        );
        return event;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for bookmark in &bookmarks {
            ui.horizontal(|ui| {
                // The bookmark itself is the "goto" affordance — an
                // ordinary hyperlink-styled label, same as how Tags'/
                // Backlinks' own document rows work — rather than a
                // separate "Goto" button next to plain text. A dangling
                // bookmark (its document no longer resolves) has nowhere
                // to go, so it stays a plain weak label instead of a link.
                match &bookmark.document_stem {
                    Some(stem) => {
                        if ui.link(format!("{stem} : {}", bookmark.line)).clicked() {
                            event = Some(BookmarksEvent::Open {
                                path: bookmark.path.clone(),
                                line: bookmark.line,
                            });
                        }
                    }
                    None => {
                        ui.label(
                            egui::RichText::new(format!("(not found) : {}", bookmark.line)).weak(),
                        );
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Delete").clicked() {
                        event = Some(BookmarksEvent::Delete(bookmark.id));
                    }
                });
            });
        }
    });

    event
}
