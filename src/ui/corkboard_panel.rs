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
            let width = ui.available_width();
            if ui
                .add(egui::TextEdit::singleline(&mut desire).desired_width(width))
                .changed()
            {
                event = Some(CorkboardEvent::SetProtagonistDesire(desire));
            }
            ui.end_row();

            ui.label("Misbelief:");
            let mut misbelief = project.meta.protagonist_misbelief.clone();
            let width = ui.available_width();
            if ui
                .add(egui::TextEdit::singleline(&mut misbelief).desired_width(width))
                .changed()
            {
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

                if !card.pov_character.is_empty() {
                    ui.horizontal(|ui| {
                        if let Some(color) = resolve_pov_color(project, &card.pov_character) {
                            let (rect, _response) = ui
                                .allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 4.0, color);
                        }
                        ui.label(&card.pov_character);
                    });
                }
                if !card.prior_belief.is_empty() || !card.new_belief.is_empty() {
                    // Plain ASCII "->", not a Unicode arrow glyph: egui's bundled
                    // default font has no glyph for U+2192 (unlike 🔗/⚠, which are
                    // covered by its emoji-icon fallback), so it rendered as a tofu
                    // box.
                    ui.label(format!(
                        "{} -> {}",
                        truncate(&card.prior_belief, 40),
                        truncate(&card.new_belief, 40)
                    ));
                }

                if !card.linked_document_stems.is_empty() {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        for stem in &card.linked_document_stems {
                            let resolved = project.tree.find_document_by_stem(stem);
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
                    });
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

/// The POV character's dot color, if that name has one assigned in
/// `ProjectMeta::pov_colors` — same lookup `story_grid_panel::resolve_pov_color` uses,
/// duplicated rather than shared (it's two lines, same precedent as `truncate` below).
fn resolve_pov_color(project: &Project, pov_character: &str) -> Option<egui::Color32> {
    project
        .pov_color_hex(pov_character)
        .and_then(crate::color_theme::parse_hex_color)
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

/// Which of the card editor's three inner tabs is showing, below the always-visible
/// Scene#/Alpha Point/Subplots/POV/Linked-documents header — same "mini `egui_dock`"
/// idiom `ui::streak_panel::StreakSubTab` uses for its Streak/Configure tabs.
/// `And So` lives on `ThirdRail`, not `Plot`: it's the decision that falls out of
/// Realization, the same internal/psychological throughline Why It Matters and
/// Realization are part of, not an external plot beat like Cause/Effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardEditorTab {
    Plot,
    BeliefAndKnowledge,
    ThirdRail,
}

/// Editing state for the card-editor modal — a form matching Lisa Cron's scene-card
/// schema field-for-field, not a raw YAML/frontmatter editor.
/// `subplot_tags_text`/`linked_documents_text`/`knowledge_text` are plain-text editing
/// buffers for the underlying `Vec<String>` fields, folded back in on save.
pub struct CardDraft {
    pub story_card: StoryCard,
    pub subplot_tags_text: String,
    /// Comma-separated linked document stems — same convention as
    /// `subplot_tags_text`, now that a card can link to more than one document.
    pub linked_documents_text: String,
    /// Comma-separated `StoryCard::new_knowledge` entries, same convention.
    pub knowledge_text: String,
    /// Which inner tab is showing — purely UI navigation state, not part of
    /// `StoryCard` itself, so it isn't touched by `finalize`. Always starts on
    /// `Plot`: both `new()` and `from_card()` open the editor fresh, so there's no
    /// "last tab you were on" to restore.
    pub active_tab: CardEditorTab,
    /// Whether this draft is a brand new card that hasn't been saved yet — controls
    /// whether the editor offers a "Delete" button.
    pub is_new: bool,
    /// Autocomplete state for `linked_documents_text`, private to this module (unlike
    /// the fields above, `app.rs` never needs to read these).
    ///
    /// Whether the linked-documents field had focus as of the end of last frame —
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
            linked_documents_text: String::new(),
            knowledge_text: String::new(),
            active_tab: CardEditorTab::Plot,
            is_new: true,
            linked_document_focused: false,
            linked_document_selected: 0,
        }
    }

    pub fn from_card(card: &StoryCard) -> Self {
        Self {
            story_card: card.clone(),
            subplot_tags_text: card.subplot_tags.join(", "),
            linked_documents_text: card.linked_document_stems.join(", "),
            knowledge_text: card.new_knowledge.join(", "),
            active_tab: CardEditorTab::Plot,
            is_new: false,
            linked_document_focused: false,
            linked_document_selected: 0,
        }
    }

    /// Fold the editing buffers back into `story_card`, ready to persist.
    pub fn finalize(mut self) -> StoryCard {
        self.story_card.subplot_tags = split_comma_list(&self.subplot_tags_text);
        self.story_card.new_knowledge = split_comma_list(&self.knowledge_text);
        self.story_card.linked_document_stems = split_comma_list(&self.linked_documents_text);
        self.story_card
    }
}

/// Splits a comma-separated editing buffer into a trimmed, non-empty-entry list —
/// the shared shape `subplot_tags_text`/`knowledge_text`/`linked_documents_text` all
/// fold back into their underlying `Vec<String>` fields with.
fn split_comma_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
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

/// Splits a comma-separated editing buffer into the text before (and including) its
/// last comma, and the segment after it — the piece currently being typed. `prefix`
/// is `""` when there's no comma yet (a single, still-untyped entry).
fn last_comma_segment(text: &str) -> (&str, &str) {
    match text.rfind(',') {
        Some(idx) => (&text[..=idx], &text[idx + 1..]),
        None => ("", text),
    }
}

/// Replaces the comma-segment currently being typed in `linked_documents_text` with
/// `candidate`, and appends a trailing ", " — so accepting a suggestion (by keyboard
/// or by click) leaves the field immediately ready to type a second linked document,
/// rather than requiring the user to notice on their own that this field takes a
/// comma-separated list.
fn accept_linked_document_candidate(draft: &mut CardDraft, candidate: &str) {
    let (prefix, _) = last_comma_segment(&draft.linked_documents_text);
    let prefix = prefix.to_string();
    draft.linked_documents_text = if prefix.is_empty() {
        format!("{candidate}, ")
    } else {
        format!("{prefix} {candidate}, ")
    };
    draft.linked_document_selected = 0;
}

/// Renders the card-editor modal. Returns `Some` once the user confirms, deletes, or
/// cancels this frame. `pov_titles` are the picklist-folder-sourced POV options (see
/// `ui::metadata_panel`'s `MetadataPicklists` for the parallel Metadata-panel usage);
/// an empty list falls back to a plain text field, same as `metadata_panel::pov_row`.
/// The card editor's "Plot" tab: Cause/Effect.
fn show_plot_tab(ui: &mut egui::Ui, draft: &mut CardDraft, field_width: f32) {
    ui.label("Cause (what happens):");
    ui.add(egui::TextEdit::multiline(&mut draft.story_card.cause).desired_width(field_width));
    ui.add_space(6.0);
    ui.label("Effect (external and internal consequence):");
    ui.add(egui::TextEdit::multiline(&mut draft.story_card.effect).desired_width(field_width));
}

/// The card editor's "Third Rail" tab: Why It Matters/Realization/And So.
fn show_third_rail_tab(ui: &mut egui::Ui, draft: &mut CardDraft, field_width: f32) {
    ui.label("Why it matters (the link to the protagonist's Desire/Misbelief):");
    ui.add(
        egui::TextEdit::multiline(&mut draft.story_card.why_it_matters).desired_width(field_width),
    );
    ui.add_space(6.0);
    ui.label("Realization:");
    ui.add(egui::TextEdit::multiline(&mut draft.story_card.realization).desired_width(field_width));
    ui.add_space(6.0);
    ui.label("And so? (what they do next):");
    ui.add(egui::TextEdit::multiline(&mut draft.story_card.and_so).desired_width(field_width));
}

/// The card editor's "Belief and Knowledge" tab: Prior/New Belief, Value Shift,
/// Knowledge Gained.
fn show_belief_and_knowledge_tab(ui: &mut egui::Ui, draft: &mut CardDraft, field_width: f32) {
    ui.label("Prior belief (going into this card):");
    ui.add(
        egui::TextEdit::singleline(&mut draft.story_card.prior_belief).desired_width(field_width),
    );
    ui.add_space(6.0);
    ui.label("New belief (coming out of it):");
    ui.add(egui::TextEdit::singleline(&mut draft.story_card.new_belief).desired_width(field_width));
    ui.add_space(6.0);
    ui.label("Value shift (e.g. \"Trust -> Distrust\"):");
    ui.add(
        egui::TextEdit::singleline(&mut draft.story_card.value_shift).desired_width(field_width),
    );
    ui.add_space(6.0);
    ui.label("Knowledge gained (comma-separated):");
    ui.add(egui::TextEdit::singleline(&mut draft.knowledge_text).desired_width(field_width));
}

pub fn show_card_editor(
    ctx: &egui::Context,
    draft: &mut CardDraft,
    note_titles: &[String],
    pov_titles: &[String],
) -> Option<CardEditorOutcome> {
    let mut outcome = None;

    // Computed from last frame's `linked_documents_text`/focus, before this frame's
    // `TextEdit` is built — same "steal before building" ordering `editor_panel.rs`
    // and `command_prompt.rs` use for their own popups. Only the comma-segment
    // currently being typed drives suggestions, since the field can hold several
    // linked documents.
    let (_, last_segment) = last_comma_segment(&draft.linked_documents_text);
    let query = last_segment.trim();
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
            let candidate = candidates[draft.linked_document_selected].to_string();
            accept_linked_document_candidate(draft, &candidate);
        }
        None => {}
    }

    egui::Modal::new(egui::Id::new("story_card_editor_modal")).show(ctx, |ui| {
        ui.set_min_width(640.0);
        ui.heading(if draft.is_new {
            "New Story Card"
        } else {
            "Edit Story Card"
        });
        ui.add_space(8.0);

        let mut scene_response = None;
        let mut alpha_response = None;
        let mut subplot_response = None;
        let mut linked_document_response = None;
        egui::Grid::new("story_card_editor_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Scene #:");
                let width = ui.available_width();
                scene_response = Some(
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.story_card.scene_number)
                            .desired_width(width),
                    ),
                );
                ui.end_row();

                ui.label("Alpha Point:");
                let width = ui.available_width();
                alpha_response = Some(
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.story_card.alpha_point)
                            .desired_width(width),
                    ),
                );
                ui.end_row();

                ui.label("Subplots:");
                let width = ui.available_width();
                subplot_response = Some(ui.add(
                    egui::TextEdit::singleline(&mut draft.subplot_tags_text).desired_width(width),
                ));
                ui.end_row();

                ui.label("POV Character:");
                let width = ui.available_width();
                if pov_titles.is_empty() {
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.story_card.pov_character)
                            .desired_width(width),
                    );
                } else {
                    let selected_text = if draft.story_card.pov_character.is_empty() {
                        "(none)"
                    } else {
                        draft.story_card.pov_character.as_str()
                    };
                    // No `.width(width)` here: a `ComboBox`'s rendered width (button +
                    // dropdown arrow + padding) runs slightly wider than whatever width
                    // it's asked for, and this sits inside a `Grid` cell whose column
                    // width only ever grows to fit the widest content seen, never
                    // shrinks — feeding `available_width()` back in as the requested
                    // width would grow the column a little every frame, then the next
                    // frame's `available_width()` a little more, forever. Same
                    // size-to-content convention `metadata_panel.rs`'s `pov_row`/
                    // `status_row` combo boxes already use for exactly this reason.
                    egui::ComboBox::new("story_card_editor_pov_combo", "")
                        .selected_text(selected_text)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut draft.story_card.pov_character,
                                String::new(),
                                "(none)",
                            );
                            for option in pov_titles {
                                ui.selectable_value(
                                    &mut draft.story_card.pov_character,
                                    option.clone(),
                                    option,
                                );
                            }
                        });
                }
                ui.end_row();

                ui.label("Linked documents (comma-separated):");
                let width = ui.available_width();
                linked_document_response = Some(
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.linked_documents_text)
                            .desired_width(width),
                    ),
                );
                ui.end_row();
            });
        draft.linked_document_focused = linked_document_response
            .as_ref()
            .is_some_and(|r| r.has_focus());
        // Enter confirms (like every other modal) only when it made one of this
        // form's *single-line* fields lose focus — the multiline fields below
        // (Cause, Effect, etc.) never lose focus on Enter in the first place (it
        // just inserts a newline, as it should), so this can't misfire while
        // editing prose.
        let confirmed_by_enter = ui.input(|i| i.key_pressed(egui::Key::Enter))
            && [
                &scene_response,
                &alpha_response,
                &subplot_response,
                &linked_document_response,
            ]
            .into_iter()
            .any(|r| r.as_ref().is_some_and(|r| r.lost_focus()));

        if !candidates.is_empty() {
            for (index, candidate) in candidates.iter().enumerate() {
                if ui
                    .selectable_label(index == draft.linked_document_selected, *candidate)
                    .clicked()
                {
                    let candidate = candidate.to_string();
                    accept_linked_document_candidate(draft, &candidate);
                }
            }
        }

        // Fill the modal's width rather than the fixed `text_edit_width` `TextEdit`
        // defaults to — the modal is wide precisely so these prose fields have room
        // to breathe. The modal's own frame padding already gives an equal margin on
        // both sides, so filling the remaining available width (rather than shaving
        // more off just the right edge) keeps left and right margins matching.
        let field_width = ui.available_width();

        ui.separator();
        ui.horizontal(|ui| {
            for (tab, label) in [
                (CardEditorTab::Plot, "Plot"),
                (CardEditorTab::BeliefAndKnowledge, "Belief and Knowledge"),
                (CardEditorTab::ThirdRail, "Third Rail"),
            ] {
                if ui
                    .selectable_label(draft.active_tab == tab, label)
                    .clicked()
                {
                    draft.active_tab = tab;
                }
            }
        });
        ui.separator();

        match draft.active_tab {
            CardEditorTab::Plot => show_plot_tab(ui, draft, field_width),
            CardEditorTab::ThirdRail => show_third_rail_tab(ui, draft, field_width),
            CardEditorTab::BeliefAndKnowledge => {
                show_belief_and_knowledge_tab(ui, draft, field_width)
            }
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() || confirmed_by_enter {
                outcome = Some(CardEditorOutcome::Save);
            }
            if !draft.is_new && ui.button("Delete").clicked() {
                outcome = Some(CardEditorOutcome::Delete(draft.story_card.id));
            }
            if ui.button("Cancel").clicked() {
                outcome = Some(CardEditorOutcome::Cancel);
            }
        });

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            outcome = Some(CardEditorOutcome::Cancel);
        }
    });

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::StoryCard;

    #[test]
    fn card_draft_new_starts_blank_and_marked_as_new() {
        let draft = CardDraft::new();
        assert!(draft.is_new);
        assert_eq!(draft.subplot_tags_text, "");
        assert_eq!(draft.linked_documents_text, "");
        assert_eq!(draft.knowledge_text, "");
        assert_eq!(draft.active_tab, CardEditorTab::Plot);
    }

    #[test]
    fn card_draft_from_card_is_not_new_and_joins_subplot_tags() {
        let mut card = StoryCard::new();
        card.subplot_tags = vec!["heist".to_string(), "betrayal".to_string()];
        card.linked_document_stems = vec!["Chapter 3".to_string(), "Chapter 4".to_string()];
        card.new_knowledge = vec!["the letter exists".to_string()];

        let draft = CardDraft::from_card(&card);

        assert!(!draft.is_new);
        assert_eq!(draft.subplot_tags_text, "heist, betrayal");
        assert_eq!(draft.linked_documents_text, "Chapter 3, Chapter 4");
        assert_eq!(draft.knowledge_text, "the letter exists");
    }

    #[test]
    fn finalize_splits_trims_and_filters_subplot_tags() {
        let mut draft = CardDraft::new();
        draft.subplot_tags_text = " heist ,, betrayal ,".to_string();

        let card = draft.finalize();

        assert_eq!(card.subplot_tags, vec!["heist", "betrayal"]);
    }

    #[test]
    fn finalize_treats_a_blank_linked_documents_field_as_unlinked() {
        let mut draft = CardDraft::new();
        draft.linked_documents_text = "   ".to_string();

        let card = draft.finalize();

        assert!(card.linked_document_stems.is_empty());
    }

    #[test]
    fn finalize_splits_trims_and_filters_linked_document_stems() {
        let mut draft = CardDraft::new();
        draft.linked_documents_text = "  Chapter 3 , Chapter 4 ,".to_string();

        let card = draft.finalize();

        assert_eq!(card.linked_document_stems, vec!["Chapter 3", "Chapter 4"]);
    }

    #[test]
    fn finalize_splits_trims_and_filters_new_knowledge() {
        let mut draft = CardDraft::new();
        draft.knowledge_text = " the letter exists ,, she's alive ,".to_string();

        let card = draft.finalize();

        assert_eq!(card.new_knowledge, vec!["the letter exists", "she's alive"]);
    }

    #[test]
    fn truncate_leaves_short_text_unchanged() {
        assert_eq!(truncate("Short cause", 40), "Short cause");
    }

    #[test]
    fn truncate_only_keeps_the_first_line() {
        assert_eq!(truncate("First line\nSecond line", 40), "First line");
    }

    #[test]
    fn truncate_adds_an_ellipsis_past_the_limit() {
        let long = "a".repeat(50);
        let result = truncate(&long, 10);
        assert_eq!(result.chars().count(), 11); // 10 chars + the ellipsis
        assert!(result.ends_with('\u{2026}'));
    }

    fn key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Drives `show_card_editor` with synthetic input, mirroring
    /// `binder_panel.rs`'s harness — worth it here specifically for the
    /// Escape-cancels and suggestion-popup key-stealing behavior, neither of
    /// which a plain unit test over `CardDraft` alone can exercise.
    fn run_show_card_editor(
        ctx: &egui::Context,
        draft: &mut CardDraft,
        note_titles: &[String],
        events: Vec<egui::Event>,
    ) -> Option<CardEditorOutcome> {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut outcome = None;
        crate::egui_test_support::run_ui_and_discard(ctx, input, |ui| {
            outcome = show_card_editor(ui.ctx(), draft, note_titles, &[]);
        });
        outcome
    }

    #[test]
    fn escape_cancels_the_card_editor() {
        let ctx = egui::Context::default();
        let mut draft = CardDraft::new();

        // First frame: just render, so the modal and its fields exist.
        run_show_card_editor(&ctx, &mut draft, &[], vec![]);

        let outcome =
            run_show_card_editor(&ctx, &mut draft, &[], vec![key_event(egui::Key::Escape)]);

        assert!(matches!(outcome, Some(CardEditorOutcome::Cancel)));
    }

    #[test]
    fn with_no_input_the_card_editor_produces_no_outcome() {
        let ctx = egui::Context::default();
        let mut draft = CardDraft::new();

        let outcome = run_show_card_editor(&ctx, &mut draft, &[], vec![]);

        assert!(outcome.is_none());
    }

    #[test]
    fn last_comma_segment_splits_the_segment_being_typed() {
        assert_eq!(
            last_comma_segment("Chapter 3, Chap"),
            ("Chapter 3,", " Chap")
        );
        assert_eq!(last_comma_segment("Chapter 3"), ("", "Chapter 3"));
        assert_eq!(last_comma_segment(""), ("", ""));
    }

    #[test]
    fn accept_linked_document_candidate_appends_a_trailing_comma_for_the_first_entry() {
        let mut draft = CardDraft::new();
        draft.linked_documents_text = "Chap".to_string();

        accept_linked_document_candidate(&mut draft, "Chapter 3");

        assert_eq!(draft.linked_documents_text, "Chapter 3, ");
    }

    #[test]
    fn accept_linked_document_candidate_appends_a_second_entry_after_the_first() {
        let mut draft = CardDraft::new();
        draft.linked_documents_text = "Chapter 3, Chap".to_string();

        accept_linked_document_candidate(&mut draft, "Chapter 4");

        assert_eq!(draft.linked_documents_text, "Chapter 3, Chapter 4, ");
    }
}
