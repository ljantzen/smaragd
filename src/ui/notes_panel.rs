use crate::project::Project;

/// Outcomes of user interaction with the Notes panel, handled by the caller
/// (`app.rs`) rather than mutated here — same pure-rendering-layer convention
/// `BookmarksEvent`/`TagsEvent`/`BacklinksEvent` already use.
pub enum NotesEvent {
    Open {
        path: std::path::PathBuf,
        line: usize,
        column: usize,
    },
    Delete(uuid::Uuid),
}

/// Renders every note in the whole project — not just the currently open
/// document, see `Project::resolved_notes` — as one flat,
/// document-then-line-then-column-ordered list. Recomputed fresh every frame
/// rather than cached, same reasoning as `bookmarks_panel::show`.
pub fn show(ui: &mut egui::Ui, project: &Project) -> Option<NotesEvent> {
    let mut event = None;
    ui.heading("Notes");
    ui.separator();

    let notes = project.resolved_notes();
    if notes.is_empty() {
        ui.label(
            "No notes yet — add one from the editor's line-number gutter, or its \
             keyboard shortcut.",
        );
        return event;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for note in &notes {
            ui.horizontal(|ui| {
                // The note's location is the "goto" affordance, same
                // hyperlink-styled convention `bookmarks_panel` uses. A
                // dangling note (its document no longer resolves) has
                // nowhere to go, so it stays a plain weak label instead of a
                // link — "Delete" still works.
                match &note.document_stem {
                    Some(stem) => {
                        if ui
                            .link(format!("{stem} : {}:{}", note.line, note.column))
                            .clicked()
                        {
                            event = Some(NotesEvent::Open {
                                path: note.path.clone(),
                                line: note.line,
                                column: note.column,
                            });
                        }
                    }
                    None => {
                        ui.label(
                            egui::RichText::new(format!(
                                "(not found) : {}:{}",
                                note.line, note.column
                            ))
                            .weak(),
                        );
                    }
                }
                ui.label(crate::ui::story_grid_panel::truncate(&note.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Delete").clicked() {
                        event = Some(NotesEvent::Delete(note.id));
                    }
                });
            });
        }
    });

    event
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    fn run(project: &Project) -> Option<NotesEvent> {
        let ctx = egui::Context::default();
        let mut event = None;
        crate::egui_test_support::run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
            event = show(ui, project);
        });
        event
    }

    #[test]
    fn an_empty_project_renders_without_panicking_and_raises_no_event() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        assert!(run(&project).is_none());
    }

    #[test]
    fn a_project_with_notes_renders_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project
            .upsert_note(&doc, 3, 5, "check pacing here".to_string())
            .unwrap();

        assert!(run(&project).is_none());
    }

    #[test]
    fn a_dangling_note_still_renders_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        // A note whose document doesn't exist in the binder tree at all —
        // `resolved_notes` leaves `document_stem` as `None` for it, the
        // "(not found)" row `notes_panel::show` must render without a link.
        project.meta.notes.push(crate::project::Note {
            id: uuid::Uuid::new_v4(),
            path: "does-not-exist.md".to_string(),
            line: 1,
            column: 0,
            text: "orphaned".to_string(),
        });

        assert!(run(&project).is_none());
    }
}
