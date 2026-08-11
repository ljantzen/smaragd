use std::path::PathBuf;

use crate::project::Project;
use crate::ui::story_grid_panel::{ResolvedRow, resolve_row};

/// Outcomes of user interaction with the Belief Timeline — same pure-rendering-layer
/// pattern `CorkboardEvent`/`StoryGridEvent` use. Character selection itself isn't an
/// event: `show` mutates the caller-owned `selected_character` buffer directly, the
/// same way `ui::streak_panel::show` mutates its `StreakSubTab` in place.
pub enum BeliefTimelineEvent {
    OpenLinkedDocument(PathBuf),
}

/// Distinct, non-empty `StoryCard::pov_character` values across the project's story
/// cards, in first-seen order — the character picker's options. Deliberately *not*
/// sourced from `Project::picklist_documents(PicklistField::Pov)`: a card can
/// describe a character's belief shift before any scene (and so any document POV)
/// exists for it, so this view is driven by the field actually set on cards.
fn known_pov_characters(project: &Project) -> Vec<String> {
    let mut seen = Vec::new();
    for card in &project.meta.story_cards {
        let pov = card.pov_character.trim();
        if !pov.is_empty() && !seen.iter().any(|existing: &String| existing == pov) {
            seen.push(pov.to_string());
        }
    }
    seen
}

/// `character`'s story cards, resolved against manuscript order and sorted by it —
/// unplaced cards (no resolvable link) trail, same convention Story Grid uses for
/// its own unplaced cards, just without a user-facing top/bottom toggle: a belief
/// chain reads front-to-back, so there's no case for surfacing them first.
fn character_rows<'a>(
    project: &'a Project,
    manuscript_order: &[PathBuf],
    character: &str,
) -> Vec<ResolvedRow<'a>> {
    let mut rows: Vec<ResolvedRow> = project
        .meta
        .story_cards
        .iter()
        .filter(|card| card.pov_character.trim() == character)
        .map(|card| resolve_row(project, manuscript_order, card))
        .collect();
    rows.sort_by_key(|row| row.min_position().unwrap_or(usize::MAX));
    rows
}

/// Whether a card's Prior Belief is worth printing: skipped when it just repeats the
/// previous card's New Belief verbatim, so the chain reads like a continuous arc
/// rather than restating the same belief twice in a row.
fn should_show_prior_belief(prior_belief: &str, previous_new_belief: Option<&str>) -> bool {
    !prior_belief.is_empty() && Some(prior_belief) != previous_new_belief
}

/// Renders the Belief Timeline: a chosen character's story cards, in manuscript
/// order, chained by Prior Belief → New Belief — the character arc the chat that
/// prompted this feature called out as the payoff of tracking belief state on
/// `StoryCard` at all.
pub fn show(
    ui: &mut egui::Ui,
    project: &Project,
    selected_character: &mut String,
) -> Option<BeliefTimelineEvent> {
    let mut event = None;
    let characters = known_pov_characters(project);

    ui.horizontal(|ui| {
        ui.label("Character:");
        let selected_text = if selected_character.is_empty() {
            "(choose a character)"
        } else {
            selected_character.as_str()
        };
        egui::ComboBox::new("belief_timeline_character_combo", "")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                for character in &characters {
                    ui.selectable_value(selected_character, character.clone(), character);
                }
            });
    });
    ui.separator();

    if characters.is_empty() {
        ui.weak("No story card has a POV Character set yet.");
        return None;
    }
    if !characters.contains(selected_character) {
        // A blank, or now-stale (its only card's POV Character was edited away),
        // selection would otherwise silently show an empty timeline.
        selected_character.clone_from(&characters[0]);
    }

    let manuscript_order = project.manuscript_document_order();
    let rows = character_rows(project, &manuscript_order, selected_character);

    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut previous_new_belief: Option<String> = None;
        for row in &rows {
            if let Some(row_event) = show_card_row(ui, row, previous_new_belief.as_deref()) {
                event = Some(row_event);
            }
            if !row.card.new_belief.is_empty() {
                previous_new_belief = Some(row.card.new_belief.clone());
            }
            ui.separator();
        }
    });

    event
}

fn show_card_row(
    ui: &mut egui::Ui,
    row: &ResolvedRow,
    previous_new_belief: Option<&str>,
) -> Option<BeliefTimelineEvent> {
    let mut event = None;
    let card = row.card;

    ui.horizontal(|ui| {
        let title = if card.scene_number.is_empty() {
            "Untitled scene".to_string()
        } else {
            format!("Scene {}", card.scene_number)
        };
        ui.strong(title);

        let paths = row.resolved_paths();
        if let Some(path) = paths.first() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let label = crate::project::model::document_label(name);
            if ui.link(format!("\u{1F517} {label}")).clicked() {
                event = Some(BeliefTimelineEvent::OpenLinkedDocument(path.to_path_buf()));
            }
        }
    });

    if should_show_prior_belief(&card.prior_belief, previous_new_belief) {
        ui.label(&card.prior_belief);
        // Plain "v", not a Unicode down-arrow glyph — see the matching comment in
        // `corkboard_panel.rs` on why: egui's bundled default font has no glyph for
        // arrow codepoints like this one.
        ui.weak("v");
    }
    if !card.new_belief.is_empty() {
        ui.label(egui::RichText::new(&card.new_belief).strong());
    }
    if !card.value_shift.is_empty() {
        ui.label(egui::RichText::new(&card.value_shift).italics().weak());
    }

    event
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::StoryCard;

    #[test]
    fn known_pov_characters_dedupes_and_preserves_first_seen_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let mut a = StoryCard::new();
        a.pov_character = "Alice".to_string();
        let mut b = StoryCard::new();
        b.pov_character = "Bob".to_string();
        let mut a2 = StoryCard::new();
        a2.pov_character = "Alice".to_string();
        project.upsert_story_card(a).unwrap();
        project.upsert_story_card(b).unwrap();
        project.upsert_story_card(a2).unwrap();

        assert_eq!(known_pov_characters(&project), vec!["Alice", "Bob"]);
    }

    #[test]
    fn known_pov_characters_ignores_blank_pov() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.upsert_story_card(StoryCard::new()).unwrap();

        assert!(known_pov_characters(&project).is_empty());
    }

    #[test]
    fn character_rows_only_includes_the_matching_character() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let mut alice_card = StoryCard::new();
        alice_card.pov_character = "Alice".to_string();
        let mut bob_card = StoryCard::new();
        bob_card.pov_character = "Bob".to_string();
        project.upsert_story_card(alice_card).unwrap();
        project.upsert_story_card(bob_card).unwrap();

        let rows = character_rows(&project, &[], "Alice");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].card.pov_character, "Alice");
    }

    #[test]
    fn character_rows_sorts_by_manuscript_position_with_unplaced_cards_trailing() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let first = project.create_document(dir.path(), "Scene 1").unwrap();
        let second = project.create_document(dir.path(), "Scene 2").unwrap();

        let mut unplaced = StoryCard::new();
        unplaced.pov_character = "Alice".to_string();
        unplaced.scene_number = "unplaced".to_string();
        let mut later = StoryCard::new();
        later.pov_character = "Alice".to_string();
        later.scene_number = "2".to_string();
        later.linked_document_stems = vec!["Scene 2".to_string()];
        let mut earlier = StoryCard::new();
        earlier.pov_character = "Alice".to_string();
        earlier.scene_number = "1".to_string();
        earlier.linked_document_stems = vec!["Scene 1".to_string()];

        project.upsert_story_card(unplaced).unwrap();
        project.upsert_story_card(later).unwrap();
        project.upsert_story_card(earlier).unwrap();

        let order = vec![first, second];
        let rows = character_rows(&project, &order, "Alice");

        let scene_numbers: Vec<&str> = rows
            .iter()
            .map(|row| row.card.scene_number.as_str())
            .collect();
        assert_eq!(scene_numbers, vec!["1", "2", "unplaced"]);
    }

    #[test]
    fn should_show_prior_belief_is_true_with_no_previous_card() {
        assert!(should_show_prior_belief("Distrust", None));
    }

    #[test]
    fn should_show_prior_belief_is_false_when_blank() {
        assert!(!should_show_prior_belief("", Some("Trust")));
    }

    #[test]
    fn should_show_prior_belief_is_false_when_it_repeats_the_previous_new_belief() {
        assert!(!should_show_prior_belief("Trust", Some("Trust")));
    }

    #[test]
    fn should_show_prior_belief_is_true_when_it_differs_from_the_previous_new_belief() {
        assert!(should_show_prior_belief("Distrust", Some("Trust")));
    }
}
