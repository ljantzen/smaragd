use uuid::Uuid;

use crate::project::{Project, StoryCard};

/// Outcomes of user interaction with the corkboard, handled by the caller (`app.rs`)
/// rather than mutated here — keeps this module a pure rendering layer over
/// `&Project`, matching `BinderEvent`'s pattern in `binder_panel.rs`.
pub enum CorkboardEvent {
    /// Open the editor for a brand new, not-yet-saved card.
    CreateCard,
    EditCard(Uuid),
    DeleteCard(Uuid),
    MoveCard { id: Uuid, new_index: usize },
    OpenLinkedDocument(std::path::PathBuf),
}

/// Renders the corkboard: a wrapping grid of story-card summaries. Card count is
/// unbounded and the panel width varies, so this uses `horizontal_wrapped` (as
/// `markdown_preview.rs` does for inline spans) rather than a fixed-column
/// `egui::Grid`.
pub fn show(ui: &mut egui::Ui, project: &Project) -> Option<CorkboardEvent> {
    let mut event = None;

    ui.horizontal(|ui| {
        if ui.button("+ New Card").clicked() {
            event = Some(CorkboardEvent::CreateCard);
        }
    });
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let count = project.meta.story_cards.len();
            for (index, card) in project.meta.story_cards.iter().enumerate() {
                if let Some(card_event) = show_card(ui, project, card, index, count) {
                    event = Some(card_event);
                }
            }
        });
    });

    event
}

fn show_card(
    ui: &mut egui::Ui,
    project: &Project,
    card: &StoryCard,
    index: usize,
    count: usize,
) -> Option<CorkboardEvent> {
    let mut event = None;

    egui::Frame::group(ui.style())
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(220.0);

            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("\u{2193}").clicked() && index + 1 < count {
                        event = Some(CorkboardEvent::MoveCard {
                            id: card.id,
                            new_index: index + 1,
                        });
                    }
                    if ui.small_button("\u{2191}").clicked() && index > 0 {
                        event = Some(CorkboardEvent::MoveCard {
                            id: card.id,
                            new_index: index - 1,
                        });
                    }
                    let title = if card.scene_number.is_empty() {
                        "Untitled scene".to_string()
                    } else {
                        format!("Scene {}", card.scene_number)
                    };
                    ui.strong(title);
                });
            });

            if !card.alpha_point.is_empty() {
                ui.label(egui::RichText::new(&card.alpha_point).italics().weak());
            }
            ui.add_space(4.0);
            if !card.cause.is_empty() {
                ui.label(format!("Cause: {}", truncate(&card.cause, 90)));
            }
            if !card.effect.is_empty() {
                ui.label(format!("Effect: {}", truncate(&card.effect, 90)));
            }

            if let Some(stem) = &card.linked_document_stem {
                let resolved = project.tree.find_document_by_stem(stem);
                ui.add_space(4.0);
                let label = match resolved {
                    Some(_) => format!("\u{1F517} {stem}"),
                    None => format!("\u{26A0} {stem} (not found)"),
                };
                let response = ui.small_button(label);
                if response.clicked()
                    && let Some(node) = resolved
                {
                    event = Some(CorkboardEvent::OpenLinkedDocument(node.path.clone()));
                }
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Edit").clicked() {
                    event = Some(CorkboardEvent::EditCard(card.id));
                }
                if ui.button("Delete").clicked() {
                    event = Some(CorkboardEvent::DeleteCard(card.id));
                }
            });
        });

    event
}

fn truncate(text: &str, max_chars: usize) -> String {
    let first_line = text.lines().next().unwrap_or("");
    if first_line.chars().count() <= max_chars {
        first_line.to_string()
    } else {
        let mut truncated: String = first_line.chars().take(max_chars).collect();
        truncated.push('\u{2026}');
        truncated
    }
}

/// Editing state for the card-editor modal — a form matching Lisa Cron's
/// four-quadrant schema field-for-field, not a raw YAML/frontmatter editor.
/// `subplot_tags_text`/`linked_document_text` are plain-text editing buffers for the
/// underlying `Vec<String>`/`Option<String>` fields, folded back in on save.
pub struct CardDraft {
    pub story_card: StoryCard,
    pub subplot_tags_text: String,
    pub linked_document_text: String,
    /// Whether this draft is a brand new card that hasn't been saved yet — controls
    /// whether the editor offers a "Delete" button.
    pub is_new: bool,
}

impl Default for CardDraft {
    fn default() -> Self {
        Self::new()
    }
}

impl CardDraft {
    pub fn new() -> Self {
        Self {
            story_card: StoryCard::new(),
            subplot_tags_text: String::new(),
            linked_document_text: String::new(),
            is_new: true,
        }
    }

    pub fn from_card(card: &StoryCard) -> Self {
        Self {
            story_card: card.clone(),
            subplot_tags_text: card.subplot_tags.join(", "),
            linked_document_text: card.linked_document_stem.clone().unwrap_or_default(),
            is_new: false,
        }
    }

    /// Fold the editing buffers back into `story_card`, ready to persist.
    pub fn finalize(mut self) -> StoryCard {
        self.story_card.subplot_tags = self
            .subplot_tags_text
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let linked = self.linked_document_text.trim();
        self.story_card.linked_document_stem = if linked.is_empty() {
            None
        } else {
            Some(linked.to_string())
        };
        self.story_card
    }
}

pub enum CardEditorOutcome {
    Save,
    Delete(Uuid),
    Cancel,
}

/// Renders the card-editor modal. Returns `Some` once the user confirms, deletes, or
/// cancels this frame.
pub fn show_card_editor(ctx: &egui::Context, draft: &mut CardDraft) -> Option<CardEditorOutcome> {
    let mut outcome = None;

    egui::Modal::new(egui::Id::new("story_card_editor_modal")).show(ctx, |ui| {
        ui.set_min_width(420.0);
        ui.heading(if draft.is_new {
            "New Story Card"
        } else {
            "Edit Story Card"
        });
        ui.add_space(8.0);

        egui::Grid::new("story_card_editor_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Scene #:");
                ui.text_edit_singleline(&mut draft.story_card.scene_number);
                ui.end_row();

                ui.label("Alpha Point:");
                ui.text_edit_singleline(&mut draft.story_card.alpha_point);
                ui.end_row();

                ui.label("Subplots:");
                ui.text_edit_singleline(&mut draft.subplot_tags_text);
                ui.end_row();

                ui.label("Linked document:");
                ui.text_edit_singleline(&mut draft.linked_document_text);
                ui.end_row();
            });

        ui.separator();
        ui.label("Cause (what happens, and why it matters to the protagonist's goal):");
        ui.text_edit_multiline(&mut draft.story_card.cause);
        ui.add_space(6.0);
        ui.label("Effect (external and internal consequence):");
        ui.text_edit_multiline(&mut draft.story_card.effect);
        ui.add_space(6.0);
        ui.label("Realization:");
        ui.text_edit_multiline(&mut draft.story_card.realization);
        ui.add_space(6.0);
        ui.label("And so? (what they do next):");
        ui.text_edit_multiline(&mut draft.story_card.and_so);

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                outcome = Some(CardEditorOutcome::Save);
            }
            if !draft.is_new && ui.button("Delete").clicked() {
                outcome = Some(CardEditorOutcome::Delete(draft.story_card.id));
            }
            if ui.button("Cancel").clicked() {
                outcome = Some(CardEditorOutcome::Cancel);
            }
        });
    });

    outcome
}
