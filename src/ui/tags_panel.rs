use crate::project::TagGroup;

/// Outcomes of user interaction with the Tags panel, handled by the caller
/// (`app.rs`) rather than mutated here — keeps this module a pure rendering
/// layer, matching `BacklinksEvent`. Clicking a tag heading or editing the
/// search box doesn't need an event: both just mutate `search_text` directly,
/// the same way `metadata_panel::show` mutates its draft in place.
pub enum TagsEvent {
    OpenDocument(std::path::PathBuf),
    /// The manual "Refresh" button was clicked — recompute now regardless of
    /// whether the open document has actually changed since the last scan.
    Refresh,
    /// The "Rename…" button next to a tag heading was clicked — the caller
    /// (`app.rs`) opens a name-prompt modal pre-filled with this tag, then
    /// applies it project-wide via `Project::rename_tag`.
    RenameTag(String),
}

/// Renders the Tags dock: by default, the currently-open document's own tags
/// (`tags`, frontmatter `tags:` merged with inline `#tag` mentions — see
/// `Project::related_by_tag`), each paired with the other documents in the
/// project that share it; typing into the search box (or clicking one of
/// those tag headings, which fills it in) switches to a flat, vault-wide list
/// of every document carrying the typed tag (`search_results` — see
/// `Project::documents_with_tag`). `open_path` is only used to distinguish
/// "no document open" from "document open, zero tags" — `tags` itself is
/// already scoped to whatever document is open by the caller (see `app.rs`'s
/// `recompute_tags`); this module never calls into `Project` itself.
pub fn show(
    ui: &mut egui::Ui,
    open_path: Option<&std::path::Path>,
    tags: &[TagGroup],
    search_text: &mut String,
    search_results: &[(std::path::PathBuf, String)],
) -> Option<TagsEvent> {
    let mut event = None;

    ui.horizontal(|ui| {
        ui.heading("Tags");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Refresh").clicked() {
                event = Some(TagsEvent::Refresh);
            }
        });
    });
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Search:");
        ui.text_edit_singleline(search_text);
        if ui.small_button("Clear").clicked() {
            search_text.clear();
        }
    });
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        if !search_text.trim().is_empty() {
            ui.label(format!("Documents tagged #{}", search_text.trim()));
            ui.add_space(4.0);
            if search_results.is_empty() {
                ui.label("No documents have this tag.");
            } else {
                for (path, title) in search_results {
                    if ui.link(title).clicked() {
                        event = Some(TagsEvent::OpenDocument(path.clone()));
                    }
                }
            }
            return;
        }

        if open_path.is_none() {
            ui.label("Open a document to see its tags.");
            return;
        }
        if tags.is_empty() {
            ui.label("This document has no tags yet.");
            return;
        }

        for group in tags {
            ui.horizontal(|ui| {
                if ui.link(format!("#{}", group.tag)).clicked() {
                    *search_text = group.tag.clone();
                }
                if ui.small_button("Rename…").clicked() {
                    event = Some(TagsEvent::RenameTag(group.tag.clone()));
                }
            });
            if group.documents.is_empty() {
                ui.label(egui::RichText::new("No other document shares this tag yet.").weak());
            } else {
                for (path, title) in &group.documents {
                    if ui.link(title).clicked() {
                        event = Some(TagsEvent::OpenDocument(path.clone()));
                    }
                }
            }
            ui.add_space(6.0);
        }
    });

    event
}
