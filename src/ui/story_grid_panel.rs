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

/// One of a card's `linked_document_stems`, resolved against the binder tree.
pub(crate) struct ResolvedLink {
    pub(crate) stem: String,
    /// `None` — a stale link — mirrors Corkboard's ⚠ treatment of the same state.
    pub(crate) path: Option<PathBuf>,
    /// The document's 1-based position in `Project::manuscript_document_order`, if
    /// it resolves to one (e.g. not `None` if the document now lives under
    /// Trash/Templates, outside manuscript order).
    pub(crate) position: Option<usize>,
}

/// One story card resolved against the current manuscript order, ready to render
/// as a row. `pub(crate)`, along with `resolve_row`/`ResolvedLink` below: shared with
/// `ui::belief_timeline_panel`, the other view that resolves cards against
/// manuscript order, rather than duplicating this resolution logic a second time.
pub(crate) struct ResolvedRow<'a> {
    pub(crate) card: &'a StoryCard,
    pub(crate) links: Vec<ResolvedLink>,
}

impl ResolvedRow<'_> {
    /// The row's manuscript position for sorting/placement purposes: the earliest
    /// position among its links, if any resolves to one. `None` — an "unplaced"
    /// row — covers no links at all, every link stale, and every resolved link
    /// falling outside manuscript order, alike.
    pub(crate) fn min_position(&self) -> Option<usize> {
        self.links.iter().filter_map(|link| link.position).min()
    }

    pub(crate) fn resolved_paths(&self) -> Vec<&Path> {
        self.links
            .iter()
            .filter_map(|link| link.path.as_deref())
            .collect()
    }
}

pub(crate) fn resolve_row<'a>(
    project: &Project,
    manuscript_order: &[PathBuf],
    card: &'a StoryCard,
) -> ResolvedRow<'a> {
    let links = card
        .linked_document_stems
        .iter()
        .map(|stem| match project.tree.find_document_by_stem(stem) {
            Some(node) => {
                let position = manuscript_order
                    .iter()
                    .position(|path| path == &node.path)
                    .map(|index| index + 1);
                ResolvedLink {
                    stem: stem.clone(),
                    path: Some(node.path.clone()),
                    position,
                }
            }
            None => ResolvedLink {
                stem: stem.clone(),
                path: None,
                position: None,
            },
        })
        .collect();
    ResolvedRow { card, links }
}

/// A document's POV, word count, and word count target, read live from disk —
/// same on-demand, never-persisted pattern `word_count.rs`/`metadata_panel.rs`
/// use, reused here rather than caching anything on `StoryCard` itself.
fn document_summary(path: &Path) -> (Option<String>, Option<usize>, Option<u32>) {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return (None, None, None);
    };
    let meta = crate::frontmatter::parse(&contents);
    (
        meta.pov,
        Some(crate::frontmatter::count_words(&contents)),
        meta.word_count_target,
    )
}

/// Aggregates `document_summary` across every one of a row's resolved linked
/// documents, since a card can now span several scenes: word counts are summed
/// (a card's total length across all its scenes), while POV and word-count target
/// are taken from the first resolved document that has one set — multiple linked
/// documents rarely disagree on POV, and summing per-scene targets across scenes
/// wouldn't mean anything.
fn aggregate_document_summary(paths: &[&Path]) -> (Option<String>, Option<usize>, Option<u32>) {
    let mut pov = None;
    let mut target = None;
    let mut total_words = 0usize;
    let mut any_words = false;
    for path in paths {
        let (doc_pov, words, doc_target) = document_summary(path);
        if pov.is_none() {
            pov = doc_pov;
        }
        if target.is_none() {
            target = doc_target;
        }
        if let Some(words) = words {
            total_words += words;
            any_words = true;
        }
    }
    (pov, any_words.then_some(total_words), target)
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
    let (mut placed, unplaced): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .partition(|row| row.min_position().is_some());
    placed.sort_by_key(|row| row.min_position());

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
            .column(Column::initial(140.0).at_least(80.0)) // Prior Belief
            .column(Column::initial(140.0).at_least(80.0)) // New Belief
            .column(Column::initial(140.0).at_least(80.0)) // Value Shift
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
                    "Prior Belief",
                    "New Belief",
                    "Value Shift",
                    "Tags",
                ] {
                    header.col(|ui| {
                        ui.strong(label);
                    });
                }
            })
            .body(|mut body| {
                for row in &ordered {
                    if let Some(row_event) = show_row(&mut body, project, row) {
                        event = Some(row_event);
                    }
                }
            });
    });

    event
}

fn show_row(
    body: &mut egui_extras::TableBody,
    project: &Project,
    row: &ResolvedRow,
) -> Option<StoryGridEvent> {
    let mut event = None;
    let card = row.card;
    let paths = row.resolved_paths();
    let (doc_pov, words, word_count_target) = aggregate_document_summary(&paths);
    // A card's own `pov_character` (added alongside multi-scene linking) takes
    // precedence over the linked documents' frontmatter POV — but falls back to it
    // for cards that haven't set one, so existing cards' rows don't change until
    // edited.
    let pov = if card.pov_character.trim().is_empty() {
        doc_pov
    } else {
        Some(card.pov_character.clone())
    };
    let pov_color = resolve_pov_color(project, pov.as_deref());
    let word_count_color = resolve_word_count_color(words, word_count_target);

    body.row(22.0, |mut table_row| {
        table_row.col(|ui| {
            ui.label(
                row.min_position()
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
            if row.links.is_empty() {
                ui.weak("(no document)");
            } else {
                ui.horizontal_wrapped(|ui| {
                    for link in &row.links {
                        match &link.path {
                            Some(path) => {
                                let label = document_label(path);
                                if ui.link(format!("\u{1F517} {label}")).clicked() {
                                    event = Some(StoryGridEvent::OpenLinkedDocument(path.clone()));
                                }
                            }
                            None => {
                                ui.label(format!("\u{26A0} {} (not found)", link.stem));
                            }
                        }
                    }
                });
            }
        });
        table_row.col(|ui| match (pov.as_deref(), pov_color) {
            (Some(pov), Some(color)) => {
                ui.horizontal(|ui| {
                    let (rect, _response) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, color);
                    ui.label(pov);
                });
            }
            (Some(pov), None) => {
                ui.label(pov);
            }
            (None, _) => {
                ui.label("\u{2014}");
            }
        });
        table_row.col(|ui| {
            let text = words
                .map(|words| words.to_string())
                .unwrap_or_else(|| "\u{2014}".to_string());
            match word_count_color {
                Some(color) => {
                    ui.colored_label(color, text);
                }
                None => {
                    ui.label(text);
                }
            }
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
            ui.label(truncate(&card.prior_belief));
        });
        table_row.col(|ui| {
            ui.label(truncate(&card.new_belief));
        });
        table_row.col(|ui| {
            ui.label(truncate(&card.value_shift));
        });
        table_row.col(|ui| {
            ui.label(card.subplot_tags.join(", "));
        });
    });

    event
}

/// The Words cell's red→yellow→green progress color, if a target is set —
/// same `color_theme::word_count_progress_color` gradient the Binder uses for
/// `BinderColorMode::WordCountProgress`, applied here unconditionally (Story
/// Grid isn't mode-switched). `None` (no color) when there's no target, or
/// the target is `0` — nothing to measure progress against.
fn resolve_word_count_color(words: Option<usize>, target: Option<u32>) -> Option<egui::Color32> {
    let words = words?;
    let target = target.filter(|&t| t > 0)?;
    Some(crate::color_theme::word_count_progress_color(
        words as f32 / target as f32,
    ))
}

/// The row's POV dot color, if that POV has one assigned — same
/// `Project::pov_color_hex` lookup the Binder uses for `BinderColorMode::Pov`,
/// applied here unconditionally (Story Grid isn't mode-switched).
fn resolve_pov_color(project: &Project, pov: Option<&str>) -> Option<egui::Color32> {
    pov.and_then(|pov| project.pov_color_hex(pov))
        .and_then(crate::color_theme::parse_hex_color)
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

        assert_eq!(row.min_position(), None);
        assert!(row.resolved_paths().is_empty());
    }

    #[test]
    fn resolve_row_flags_a_stale_link_as_unplaced() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut card = StoryCard::new();
        card.linked_document_stems = vec!["Missing Scene".to_string()];

        let row = resolve_row(&project, &[], &card);

        assert_eq!(row.min_position(), None);
        assert_eq!(row.links.len(), 1);
        assert!(row.links[0].path.is_none());
    }

    #[test]
    fn resolve_row_finds_the_manuscript_position_of_a_resolved_link() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        let mut card = StoryCard::new();
        card.linked_document_stems = vec!["Scene 1".to_string()];
        let order = vec![doc.clone()];

        let row = resolve_row(&project, &order, &card);

        assert_eq!(row.min_position(), Some(1));
        assert_eq!(row.resolved_paths(), vec![doc.as_path()]);
    }

    #[test]
    fn resolve_row_min_position_is_the_earliest_of_several_linked_documents() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let first = project.create_document(dir.path(), "Scene 1").unwrap();
        let second = project.create_document(dir.path(), "Scene 2").unwrap();
        let mut card = StoryCard::new();
        card.linked_document_stems = vec!["Scene 2".to_string(), "Scene 1".to_string()];
        let order = vec![first.clone(), second.clone()];

        let row = resolve_row(&project, &order, &card);

        assert_eq!(row.min_position(), Some(1));
    }

    #[test]
    fn aggregate_document_summary_sums_word_counts_across_resolved_documents() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        std::fs::write(&a, "one two three").unwrap();
        std::fs::write(&b, "four five").unwrap();

        let (_, words, _) = aggregate_document_summary(&[&a, &b]);

        assert_eq!(words, Some(5));
    }

    #[test]
    fn aggregate_document_summary_is_none_words_when_no_paths_resolve() {
        let (_, words, _) = aggregate_document_summary(&[]);
        assert_eq!(words, None);
    }

    #[test]
    fn resolve_word_count_color_is_none_without_a_target() {
        assert_eq!(resolve_word_count_color(Some(500), None), None);
    }

    #[test]
    fn resolve_word_count_color_is_none_for_a_zero_target() {
        assert_eq!(resolve_word_count_color(Some(500), Some(0)), None);
    }

    #[test]
    fn resolve_word_count_color_is_none_without_a_word_count() {
        assert_eq!(resolve_word_count_color(None, Some(1000)), None);
    }

    #[test]
    fn resolve_word_count_color_matches_the_progress_gradient() {
        assert_eq!(
            resolve_word_count_color(Some(500), Some(1000)),
            Some(crate::color_theme::word_count_progress_color(0.5))
        );
    }

    #[test]
    fn resolve_pov_color_is_none_without_a_pov() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(resolve_pov_color(&project, None), None);
    }

    #[test]
    fn resolve_pov_color_is_none_for_a_pov_with_no_assigned_color() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(resolve_pov_color(&project, Some("Alice")), None);
    }

    #[test]
    fn resolve_pov_color_reads_the_pov_s_assigned_color() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project
            .set_pov_color_hex("Alice", "#8800ff".to_string())
            .unwrap();

        assert_eq!(
            resolve_pov_color(&project, Some("Alice")),
            crate::color_theme::parse_hex_color("#8800ff")
        );
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
