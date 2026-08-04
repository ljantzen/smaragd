use std::path::{Path, PathBuf};

use egui_extras::{Column, TableBuilder};
use uuid::Uuid;

use crate::project::{Project, StoryCard};
use crate::settings::UnplacedCardsPosition;

/// Outcomes of user interaction with the Story Grid, handled by the caller
/// (`app.rs`) rather than mutated here — same pure-rendering-layer pattern
/// `CorkboardEvent` uses in `corkboard_panel.rs`. `OpenLinkedDocument`/`EditCard`
/// are deliberately handled identically to their Corkboard counterparts by the
/// caller (see `handle_story_grid_event`), since this is a second view over the
/// same cards, not a separate feature.
pub enum StoryGridEvent {
    OpenLinkedDocument(PathBuf),
    EditCard(Uuid),
    SetUnplacedPosition(UnplacedCardsPosition),
}

/// One story card resolved against the current manuscript order, ready to render
/// as a row.
struct ResolvedRow<'a> {
    card: &'a StoryCard,
    /// The card's position among `Project::manuscript_document_order` (1-based,
    /// for display), if its linked document resolves to one. `None` — an
    /// "unplaced" card — covers three cases alike: no link at all, a stale link
    /// (the document was deleted or renamed), and a link that resolves but to a
    /// document outside manuscript order (e.g. now under Trash/Templates).
    position: Option<usize>,
    resolved_path: Option<PathBuf>,
    /// A `linked_document_stem` is set but doesn't resolve to any document —
    /// mirrors Corkboard's ⚠ treatment of the same state.
    stale_link: bool,
}

fn resolve_row<'a>(
    project: &Project,
    manuscript_order: &[PathBuf],
    card: &'a StoryCard,
) -> ResolvedRow<'a> {
    let Some(stem) = card.linked_document_stem.as_deref() else {
        return ResolvedRow {
            card,
            position: None,
            resolved_path: None,
            stale_link: false,
        };
    };
    match project.tree.find_document_by_stem(stem) {
        Some(node) => {
            let position = manuscript_order
                .iter()
                .position(|path| path == &node.path)
                .map(|index| index + 1);
            ResolvedRow {
                card,
                position,
                resolved_path: Some(node.path.clone()),
                stale_link: false,
            }
        }
        None => ResolvedRow {
            card,
            position: None,
            resolved_path: None,
            stale_link: true,
        },
    }
}

/// A document's POV and word count, read live from disk — same on-demand,
/// never-persisted pattern `word_count.rs`/`metadata_panel.rs` use, reused here
/// rather than caching anything on `StoryCard` itself.
fn document_summary(path: &Path) -> (Option<String>, Option<usize>) {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return (None, None);
    };
    let meta = crate::frontmatter::parse(&contents);
    (meta.pov, Some(crate::frontmatter::count_words(&contents)))
}

/// Renders the Story Grid: a read-only, manuscript-ordered table view of the same
/// cards Corkboard edits — see this module's doc comment on `StoryGridEvent`.
/// Row order always mirrors wherever each card's linked document sits in the
/// binder today; reordering the manuscript itself still happens from the Binder,
/// not from here.
pub fn show(
    ui: &mut egui::Ui,
    project: &Project,
    unplaced_position: UnplacedCardsPosition,
) -> Option<StoryGridEvent> {
    let mut event = None;

    ui.horizontal(|ui| {
        ui.label("Unplaced cards:");
        let mut position = unplaced_position;
        egui::ComboBox::new("story_grid_unplaced_position_combo", "")
            .selected_text(position.label())
            .show_ui(ui, |ui| {
                for candidate in UnplacedCardsPosition::ALL {
                    ui.selectable_value(&mut position, candidate, candidate.label());
                }
            });
        if position != unplaced_position {
            event = Some(StoryGridEvent::SetUnplacedPosition(position));
        }
    });
    ui.separator();

    let manuscript_order = project.manuscript_document_order();
    let rows: Vec<ResolvedRow> = project
        .meta
        .story_cards
        .iter()
        .map(|card| resolve_row(project, &manuscript_order, card))
        .collect();
    let (mut placed, unplaced): (Vec<_>, Vec<_>) =
        rows.into_iter().partition(|row| row.position.is_some());
    placed.sort_by_key(|row| row.position);

    let ordered: Vec<ResolvedRow> = match unplaced_position {
        UnplacedCardsPosition::Top => unplaced.into_iter().chain(placed).collect(),
        UnplacedCardsPosition::Bottom => placed.into_iter().chain(unplaced).collect(),
    };

    egui::ScrollArea::horizontal().show(ui, |ui| {
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .vscroll(true)
            .column(Column::auto().at_least(28.0)) // #
            .column(Column::initial(70.0).at_least(40.0)) // Scene
            .column(Column::initial(150.0).at_least(80.0)) // Document
            .column(Column::initial(90.0).at_least(60.0)) // POV
            .column(Column::initial(70.0).at_least(50.0)) // Words
            .column(Column::initial(160.0).at_least(80.0)) // Cause
            .column(Column::initial(160.0).at_least(80.0)) // Effect
            .column(Column::initial(160.0).at_least(80.0)) // Why It Matters
            .column(Column::initial(160.0).at_least(80.0)) // Realization
            .column(Column::initial(160.0).at_least(80.0)) // And So
            .column(Column::remainder().at_least(80.0)) // Subplot tags
            .header(20.0, |mut header| {
                for label in [
                    "#",
                    "Scene",
                    "Document",
                    "POV",
                    "Words",
                    "Cause",
                    "Effect",
                    "Why It Matters",
                    "Realization",
                    "And So",
                    "Tags",
                ] {
                    header.col(|ui| {
                        ui.strong(label);
                    });
                }
            })
            .body(|mut body| {
                for row in &ordered {
                    if let Some(row_event) = show_row(&mut body, row) {
                        event = Some(row_event);
                    }
                }
            });
    });

    event
}

fn show_row(body: &mut egui_extras::TableBody, row: &ResolvedRow) -> Option<StoryGridEvent> {
    let mut event = None;
    let card = row.card;
    let (pov, words) = row
        .resolved_path
        .as_deref()
        .map(document_summary)
        .unwrap_or((None, None));

    body.row(22.0, |mut table_row| {
        table_row.col(|ui| {
            ui.label(
                row.position
                    .map(|position| position.to_string())
                    .unwrap_or_else(|| "\u{2014}".to_string()),
            );
        });
        table_row.col(|ui| {
            if ui.link(&card.scene_number).clicked() {
                event = Some(StoryGridEvent::EditCard(card.id));
            }
        });
        table_row.col(|ui| {
            if let Some(path) = &row.resolved_path {
                let label = document_label(path);
                if ui.link(format!("\u{1F517} {label}")).clicked() {
                    event = Some(StoryGridEvent::OpenLinkedDocument(path.clone()));
                }
            } else if row.stale_link {
                let stem = card.linked_document_stem.as_deref().unwrap_or("");
                ui.label(format!("\u{26A0} {stem} (not found)"));
            } else {
                ui.weak("(no document)");
            }
        });
        table_row.col(|ui| {
            ui.label(pov.as_deref().unwrap_or("\u{2014}"));
        });
        table_row.col(|ui| {
            ui.label(
                words
                    .map(|words| words.to_string())
                    .unwrap_or_else(|| "\u{2014}".to_string()),
            );
        });
        table_row.col(|ui| {
            ui.label(truncate(&card.cause));
        });
        table_row.col(|ui| {
            ui.label(truncate(&card.effect));
        });
        table_row.col(|ui| {
            ui.label(truncate(&card.why_it_matters));
        });
        table_row.col(|ui| {
            ui.label(truncate(&card.realization));
        });
        table_row.col(|ui| {
            ui.label(truncate(&card.and_so));
        });
        table_row.col(|ui| {
            ui.label(card.subplot_tags.join(", "));
        });
    });

    event
}

/// The current document title for `path`, without the `.md` extension — see
/// `ui::binder_panel::document_label`.
fn document_label(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    crate::ui::binder_panel::document_label(name).to_string()
}

const TRUNCATE_CHARS: usize = 60;

fn truncate(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or("");
    if first_line.chars().count() <= TRUNCATE_CHARS {
        first_line.to_string()
    } else {
        let mut truncated: String = first_line.chars().take(TRUNCATE_CHARS).collect();
        truncated.push('\u{2026}');
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_row_is_unplaced_when_no_document_is_linked() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let card = StoryCard::new();

        let row = resolve_row(&project, &[], &card);

        assert_eq!(row.position, None);
        assert_eq!(row.resolved_path, None);
        assert!(!row.stale_link);
    }

    #[test]
    fn resolve_row_flags_a_stale_link_as_unplaced() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut card = StoryCard::new();
        card.linked_document_stem = Some("Missing Scene".to_string());

        let row = resolve_row(&project, &[], &card);

        assert_eq!(row.position, None);
        assert!(row.stale_link);
    }

    #[test]
    fn resolve_row_finds_the_manuscript_position_of_a_resolved_link() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        let mut card = StoryCard::new();
        card.linked_document_stem = Some("Scene 1".to_string());
        let order = vec![doc.clone()];

        let row = resolve_row(&project, &order, &card);

        assert_eq!(row.position, Some(1));
        assert_eq!(row.resolved_path, Some(doc));
        assert!(!row.stale_link);
    }

    #[test]
    fn truncate_leaves_short_text_unchanged() {
        assert_eq!(truncate("A short cause"), "A short cause");
    }

    #[test]
    fn truncate_only_keeps_the_first_line() {
        assert_eq!(truncate("First line\nSecond line"), "First line");
    }
}
