use egui::{Key, Modifiers};
use uuid::Uuid;

use crate::autocomplete::filter_candidates;
use crate::project::{Project, StoryCard};

/// Cap on how many linked-document suggestions are shown at once, matching the
/// wikilink popup's own cap in `editor_panel.rs` rather than dumping the whole
/// project into a list.
const MAX_SUGGESTIONS: usize = 8;

/// Outcomes of user interaction with the corkboard, handled by the caller (`app.rs`)
/// rather than mutated here — keeps this module a pure rendering layer over
/// `&Project`, matching `BinderEvent`'s pattern in `binder_panel.rs`.
pub enum CorkboardEvent {
    /// Open the editor for a brand new, not-yet-saved card.
    CreateCard,
    EditCard(Uuid),
    DeleteCard(Uuid),
    MoveCard {
        id: Uuid,
        new_index: usize,
    },
    OpenLinkedDocument(std::path::PathBuf),
    /// The project-wide Desire field changed (see `ProjectMeta::protagonist_desire`).
    SetProtagonistDesire(String),
    /// The project-wide Misbelief field changed (see
    /// `ProjectMeta::protagonist_misbelief`).
    SetProtagonistMisbelief(String),
}

/// Renders the corkboard: a wrapping grid of story-card summaries. Card count is
/// unbounded and the panel width varies, so this uses `horizontal_wrapped` (as
/// `markdown_preview.rs` does for inline spans) rather than a fixed-column
/// `egui::Grid`.
pub fn show(ui: &mut egui::Ui, project: &Project) -> Option<CorkboardEvent> {
    let mut event = None;

    // Lisa Cron's "Third Rail": the protagonist's Desire and Misbelief, project-wide
    // rather than per-card — the throughline every card's `why_it_matters` should
    // ultimately test or advance. Edited live, the same "mutate a local copy, raise
    // an event on change" pattern every other field in this module already uses,
    // rather than requiring its own Save step.
    egui::Grid::new("protagonist_third_rail_grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Desire:");
            let mut desire = project.meta.protagonist_desire.clone();
            if ui.text_edit_singleline(&mut desire).changed() {
                event = Some(CorkboardEvent::SetProtagonistDesire(desire));
            }
            ui.end_row();

            ui.label("Misbelief:");
            let mut misbelief = project.meta.protagonist_misbelief.clone();
            if ui.text_edit_singleline(&mut misbelief).changed() {
                event = Some(CorkboardEvent::SetProtagonistMisbelief(misbelief));
            }
            ui.end_row();
        });
    ui.separator();

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

            // `Frame::show`'s inner `Ui` inherits its parent's layout rather than
            // defaulting to one — since `show_card` is called from inside `show`'s
            // `horizontal_wrapped`, everything below would otherwise be laid out as
            // wrapped inline items (like flowing text) instead of a vertical stack,
            // garbling the card's contents. Forcing a vertical layout here is what
            // actually makes this a card rather than a run of wrapped widgets.
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Down").clicked() && index + 1 < count {
                            event = Some(CorkboardEvent::MoveCard {
                                id: card.id,
                                new_index: index + 1,
                            });
                        }
                        if ui.small_button("Up").clicked() && index > 0 {
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
                if !card.why_it_matters.is_empty() {
                    ui.label(format!(
                        "Why it matters: {}",
                        truncate(&card.why_it_matters, 90)
                    ));
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

/// Editing state for the card-editor modal — a form matching Lisa Cron's scene-card
/// schema field-for-field, not a raw YAML/frontmatter editor.
/// `subplot_tags_text`/`linked_document_text` are plain-text editing buffers for the
/// underlying `Vec<String>`/`Option<String>` fields, folded back in on save.
pub struct CardDraft {
    pub story_card: StoryCard,
    pub subplot_tags_text: String,
    pub linked_document_text: String,
    /// Whether this draft is a brand new card that hasn't been saved yet — controls
    /// whether the editor offers a "Delete" button.
    pub is_new: bool,
    /// Autocomplete state for `linked_document_text`, private to this module (unlike
    /// the fields above, `app.rs` never needs to read these).
    ///
    /// Whether the linked-document field had focus as of the end of last frame —
    /// scopes Tab/arrow key-stealing to that field alone, so pressing Tab while
    /// editing a different field (e.g. Cause) isn't hijacked by a suggestion list
    /// left over from a previously-filled-in link.
    linked_document_focused: bool,
    linked_document_selected: usize,
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
            linked_document_focused: false,
            linked_document_selected: 0,
        }
    }

    pub fn from_card(card: &StoryCard) -> Self {
        Self {
            story_card: card.clone(),
            subplot_tags_text: card.subplot_tags.join(", "),
            linked_document_text: card.linked_document_stem.clone().unwrap_or_default(),
            is_new: false,
            linked_document_focused: false,
            linked_document_selected: 0,
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

enum PopupAction {
    Next,
    Prev,
    Accept,
}

/// Consume (and act on) Tab/arrow keys meant for the linked-document suggestion
/// list, so the `TextEdit` underneath never sees them — Tab would otherwise move
/// focus off the field entirely.
fn steal_popup_key(ctx: &egui::Context) -> Option<PopupAction> {
    ctx.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            Some(PopupAction::Next)
        } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            Some(PopupAction::Prev)
        } else if i.consume_key(Modifiers::NONE, Key::Tab) {
            Some(PopupAction::Accept)
        } else {
            None
        }
    })
}

/// Renders the card-editor modal. Returns `Some` once the user confirms, deletes, or
/// cancels this frame.
pub fn show_card_editor(
    ctx: &egui::Context,
    draft: &mut CardDraft,
    note_titles: &[String],
) -> Option<CardEditorOutcome> {
    let mut outcome = None;

    // Computed from last frame's `linked_document_text`/focus, before this frame's
    // `TextEdit` is built — same "steal before building" ordering `editor_panel.rs`
    // and `command_prompt.rs` use for their own popups.
    let query = draft.linked_document_text.trim();
    let all_candidates = if query.is_empty() {
        Vec::new()
    } else {
        filter_candidates(note_titles, query)
    };
    let candidates = &all_candidates[..all_candidates.len().min(MAX_SUGGESTIONS)];
    if !candidates.is_empty() {
        draft.linked_document_selected = draft.linked_document_selected.min(candidates.len() - 1);
    }
    let popup_action = (!candidates.is_empty() && draft.linked_document_focused)
        .then(|| steal_popup_key(ctx))
        .flatten();
    match popup_action {
        Some(PopupAction::Next) => {
            draft.linked_document_selected =
                (draft.linked_document_selected + 1) % candidates.len();
        }
        Some(PopupAction::Prev) => {
            draft.linked_document_selected =
                (draft.linked_document_selected + candidates.len() - 1) % candidates.len();
        }
        Some(PopupAction::Accept) => {
            draft.linked_document_text = candidates[draft.linked_document_selected].to_string();
            draft.linked_document_selected = 0;
        }
        None => {}
    }

    egui::Modal::new(egui::Id::new("story_card_editor_modal")).show(ctx, |ui| {
        ui.set_min_width(420.0);
        ui.heading(if draft.is_new {
            "New Story Card"
        } else {
            "Edit Story Card"
        });
        ui.add_space(8.0);

        let mut linked_document_response = None;
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
                linked_document_response =
                    Some(ui.text_edit_singleline(&mut draft.linked_document_text));
                ui.end_row();
            });
        draft.linked_document_focused = linked_document_response.is_some_and(|r| r.has_focus());

        if !candidates.is_empty() {
            for (index, candidate) in candidates.iter().enumerate() {
                if ui
                    .selectable_label(index == draft.linked_document_selected, *candidate)
                    .clicked()
                {
                    draft.linked_document_text = candidate.to_string();
                    draft.linked_document_selected = 0;
                }
            }
        }

        ui.separator();
        ui.label("Cause (what happens):");
        ui.text_edit_multiline(&mut draft.story_card.cause);
        ui.add_space(6.0);
        ui.label("Effect (external and internal consequence):");
        ui.text_edit_multiline(&mut draft.story_card.effect);
        ui.add_space(6.0);
        ui.label("Why it matters (the link to the protagonist's Desire/Misbelief):");
        ui.text_edit_multiline(&mut draft.story_card.why_it_matters);
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
