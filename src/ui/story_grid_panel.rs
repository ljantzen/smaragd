use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui_extras::{Column, TableBuilder};
use uuid::Uuid;

use crate::project::{Project, StoryCard, StoryGridOrderMode};
use crate::settings::{StoryGridColumn, UnplacedCardsPosition};

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
    /// Switch between manuscript-order and manual (Corkboard-order) rows — see
    /// `StoryGridOrderMode`.
    SetOrderMode(StoryGridOrderMode),
    /// Reorder a card while `StoryGridOrderMode::Manual` is active — the Story
    /// Grid's own Up/Down buttons in the `#` column, raised identically to
    /// `CorkboardEvent::MoveCard` (same underlying `Project::move_story_card`),
    /// since it's the same freeform order Corkboard edits, just viewed here too.
    MoveCard {
        id: Uuid,
        new_index: usize,
    },
    /// Show/hide a column via the "Columns" menu.
    SetColumnHidden(StoryGridColumn, bool),
    /// The full new column order, after a drag-free "Up"/"Down" reorder in the
    /// "Columns" menu — same whole-vec-per-move shape as the menu's up/down
    /// buttons themselves, simpler than an index-based move since there's no
    /// fallible lookup involved (contrast `CorkboardEvent::MoveCard`, which needs
    /// a card id because cards live in `Project`, not `Settings`).
    SetColumnOrder(Vec<StoryGridColumn>),
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

/// Orders resolved rows for display, per `StoryGridOrderMode`. `Manuscript`
/// (the default) sorts by manuscript position, tie-broken by card id — a stable
/// key, deliberately not "whatever order `rows` arrived in" (that's
/// `ProjectMeta::story_cards`'s own vec order, which `Manual` mode's Up/Down
/// buttons mutate): without this tie-break, cards sharing a manuscript position,
/// or with none at all (Unplaced), would silently keep reflecting Manual mode's
/// leftover order after switching back, rather than `Manuscript` always
/// reproducing the same order regardless of what Manual mode last did. `Manual`
/// shows `rows` as-is — no Unplaced split, since manuscript position no longer
/// determines placement.
pub(crate) fn order_rows(
    rows: Vec<ResolvedRow>,
    order_mode: StoryGridOrderMode,
    unplaced_position: UnplacedCardsPosition,
) -> Vec<ResolvedRow> {
    if order_mode == StoryGridOrderMode::Manual {
        return rows;
    }
    let (mut placed, mut unplaced): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .partition(|row| row.min_position().is_some());
    placed.sort_by_key(|row| (row.min_position(), row.card.id));
    unplaced.sort_by_key(|row| row.card.id);
    match unplaced_position {
        UnplacedCardsPosition::Top => unplaced.into_iter().chain(placed).collect(),
        UnplacedCardsPosition::Bottom => placed.into_iter().chain(unplaced).collect(),
    }
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

/// Renders the Story Grid: a table view of the same cards Corkboard edits — see
/// this module's doc comment on `StoryGridEvent`. Row order follows
/// `project.meta.story_grid_order_mode`: `Manuscript` (the default) mirrors
/// wherever each card's linked document sits in the binder today, read-only;
/// `Manual` shows — and lets you reorder — the same freeform order Corkboard
/// uses.
pub fn show(
    ui: &mut egui::Ui,
    project: &Project,
    unplaced_position: UnplacedCardsPosition,
    column_order: &[StoryGridColumn],
    hidden_columns: &BTreeSet<StoryGridColumn>,
) -> Option<StoryGridEvent> {
    let mut event = None;
    let order_mode = project.meta.story_grid_order_mode;

    ui.horizontal(|ui| {
        ui.label("Order:");
        let mut mode = order_mode;
        egui::ComboBox::new("story_grid_order_mode_combo", "")
            .selected_text(mode.label())
            .show_ui(ui, |ui| {
                for candidate in StoryGridOrderMode::ALL {
                    ui.selectable_value(&mut mode, candidate, candidate.label());
                }
            });
        if mode != order_mode {
            event = Some(StoryGridEvent::SetOrderMode(mode));
        }

        ui.add_space(12.0);

        // Only meaningful in `Manuscript` mode — `Manual` shows every card in its
        // own freeform order, with no separate Unplaced section to place.
        ui.add_enabled_ui(order_mode == StoryGridOrderMode::Manuscript, |ui| {
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

        // Pushed to the dock's right edge, away from the left-aligned controls,
        // rather than sitting immediately next to them.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // `egui::menu::MenuButton` directly, not the `ui.menu_button` shorthand:
            // that shorthand always uses `PopupCloseBehavior`'s default
            // (`CloseOnClick`, closing on *any* click inside the popup, not just
            // outside it) — wrong here, since toggling a checkbox or clicking an
            // Up/Down arrow is meant to be repeatable without reopening the menu
            // each time.
            egui::menu::MenuButton::new("Columns")
                .config(
                    egui::containers::menu::MenuConfig::new()
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
                )
                .ui(ui, |ui| {
                    let count = column_order.len();
                    for (index, kind) in column_order.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let mut visible = !hidden_columns.contains(kind);
                            // A fixed-width label column, not flush against the label
                            // (too cramped, and ragged across rows of differing label
                            // length) nor pushed to the row's far edge (`Layout::
                            // right_to_left`, tried first — stretched every row to the
                            // menu's full available width, leaving a wide gap for every
                            // label shorter than the longest one, "Why It Matters").
                            // This keeps a modest, consistent gap and lines the arrows
                            // up in a column across every row.
                            let checkbox = ui
                                .scope(|ui| {
                                    ui.set_min_width(130.0);
                                    ui.checkbox(&mut visible, kind.label())
                                })
                                .inner;
                            if checkbox.changed() {
                                event = Some(StoryGridEvent::SetColumnHidden(*kind, !visible));
                            }
                            // `⬆`/`⬇` (U+2B06/U+2B07), not the plain `↑`/`↓` or `▲`/`▼`
                            // arrows/triangles — none of those are covered by any font in
                            // egui's default `Proportional` fallback chain (Ubuntu-Light/
                            // NotoEmoji/emoji-icon-font, see `editor_font::install`, which
                            // starts from `FontDefinitions::default()` and only adds named
                            // families, leaving that chain untouched), and render as tofu.
                            // U+2B06/U+2B07 are the ones both NotoEmoji-Regular and
                            // emoji-icon-font actually carry glyphs for — checked directly
                            // against `epaint_default_fonts`' bundled .ttf files with
                            // `ttf_parser::Face::glyph_index`.
                            if ui.small_button("\u{2b06}").clicked() && index > 0 {
                                let mut order = column_order.to_vec();
                                order.swap(index, index - 1);
                                event = Some(StoryGridEvent::SetColumnOrder(order));
                            }
                            if ui.small_button("\u{2b07}").clicked() && index + 1 < count {
                                let mut order = column_order.to_vec();
                                order.swap(index, index + 1);
                                event = Some(StoryGridEvent::SetColumnOrder(order));
                            }
                        });
                    }
                });
        });
    });
    ui.separator();

    let manuscript_order = project.manuscript_document_order();
    let rows: Vec<ResolvedRow> = project
        .meta
        .story_cards
        .iter()
        .map(|card| resolve_row(project, &manuscript_order, card))
        .collect();

    let ordered = order_rows(rows, order_mode, unplaced_position);

    let visible_columns: Vec<StoryGridColumn> = column_order
        .iter()
        .copied()
        .filter(|kind| !hidden_columns.contains(kind))
        .collect();

    egui::ScrollArea::horizontal().show(ui, |ui| {
        if visible_columns.is_empty() {
            ui.weak("All columns are hidden — use the Columns menu above to show one.");
            return;
        }

        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .vscroll(true);
        for (index, kind) in visible_columns.iter().enumerate() {
            builder = builder.column(column_size(*kind, index + 1 == visible_columns.len()));
        }
        builder
            .header(20.0, |mut header| {
                for kind in &visible_columns {
                    header.col(|ui| {
                        ui.strong(kind.label());
                    });
                }
            })
            .body(|mut body| {
                let count = ordered.len();
                for (index, row) in ordered.iter().enumerate() {
                    if let Some(row_event) = show_row(
                        &mut body,
                        project,
                        row,
                        &visible_columns,
                        order_mode,
                        index,
                        count,
                    ) {
                        event = Some(row_event);
                    }
                }
            });
    });

    event
}

/// Sizing for one `StoryGridColumn` — carried over 1:1 from the previous
/// hardcoded per-column widths, except the last *visible* column always claims
/// remaining width (`Column::remainder()`) regardless of which column it is, so
/// hiding/moving `SubplotTags` (formerly the sole `remainder()` column) doesn't
/// leave the table failing to fill available space.
fn column_size(kind: StoryGridColumn, is_last_visible: bool) -> Column {
    if is_last_visible {
        return Column::remainder().at_least(80.0);
    }
    match kind {
        // Wide enough for the `Manual`-mode Up/Down buttons alongside the number,
        // not just the number alone.
        StoryGridColumn::Index => Column::auto().at_least(60.0),
        StoryGridColumn::Scene => Column::initial(70.0).at_least(40.0),
        StoryGridColumn::Document => Column::initial(150.0).at_least(80.0),
        StoryGridColumn::Pov => Column::initial(90.0).at_least(60.0),
        StoryGridColumn::Words => Column::initial(70.0).at_least(50.0),
        StoryGridColumn::Cause
        | StoryGridColumn::Effect
        | StoryGridColumn::WhyItMatters
        | StoryGridColumn::Realization
        | StoryGridColumn::AndSo => Column::initial(160.0).at_least(80.0),
        StoryGridColumn::PriorBelief | StoryGridColumn::NewBelief | StoryGridColumn::ValueShift => {
            Column::initial(140.0).at_least(80.0)
        }
        StoryGridColumn::SubplotTags => Column::initial(80.0).at_least(80.0),
    }
}

/// A row's precomputed, column-independent values — resolved once per row rather
/// than once per visible cell, since several (`pov`, `words`) are already
/// aggregated across a card's linked documents before any column-specific
/// rendering happens.
struct RowContext<'a> {
    row: &'a ResolvedRow<'a>,
    pov: Option<String>,
    pov_color: Option<egui::Color32>,
    words: Option<usize>,
    word_count_color: Option<egui::Color32>,
    order_mode: StoryGridOrderMode,
    /// This row's position within `ordered` — only meaningful (as an Up/Down
    /// reorder target) when `order_mode` is `Manual`, where `ordered` is exactly
    /// `project.meta.story_cards`'s own order.
    row_index: usize,
    row_count: usize,
}

#[allow(clippy::too_many_arguments)]
fn show_row(
    body: &mut egui_extras::TableBody,
    project: &Project,
    row: &ResolvedRow,
    visible_columns: &[StoryGridColumn],
    order_mode: StoryGridOrderMode,
    row_index: usize,
    row_count: usize,
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
    let ctx = RowContext {
        row,
        pov,
        pov_color,
        words,
        word_count_color,
        order_mode,
        row_index,
        row_count,
    };

    body.row(22.0, |mut table_row| {
        for kind in visible_columns {
            table_row.col(|ui| {
                if let Some(cell_event) = render_cell(ui, *kind, &ctx) {
                    event = Some(cell_event);
                }
            });
        }
    });

    event
}

/// Renders one cell, dispatched by `StoryGridColumn` — each arm's body is the
/// same rendering logic the column had when this was a straight-line sequence of
/// `table_row.col(...)` calls, unchanged, just reachable by column identity now
/// that columns can be reordered/hidden.
fn render_cell(
    ui: &mut egui::Ui,
    kind: StoryGridColumn,
    ctx: &RowContext,
) -> Option<StoryGridEvent> {
    let card = ctx.row.card;
    match kind {
        StoryGridColumn::Index => {
            let mut event = None;
            ui.horizontal(|ui| {
                // Up/Down buttons only in `Manual` mode — same U+2B06/U+2B07 icons
                // the Columns menu uses for the same purpose (see its doc comment
                // for why those specific codepoints), reordering the same
                // `ProjectMeta::story_cards` vec Corkboard's own Up/Down buttons do.
                if ctx.order_mode == StoryGridOrderMode::Manual {
                    if ui.small_button("\u{2b06}").clicked() && ctx.row_index > 0 {
                        event = Some(StoryGridEvent::MoveCard {
                            id: card.id,
                            new_index: ctx.row_index - 1,
                        });
                    }
                    if ui.small_button("\u{2b07}").clicked() && ctx.row_index + 1 < ctx.row_count {
                        event = Some(StoryGridEvent::MoveCard {
                            id: card.id,
                            new_index: ctx.row_index + 1,
                        });
                    }
                }
                ui.label(
                    ctx.row
                        .min_position()
                        .map(|position| position.to_string())
                        .unwrap_or_else(|| "\u{2014}".to_string()),
                );
            });
            event
        }
        StoryGridColumn::Scene => {
            if ui.link(&card.scene_number).clicked() {
                Some(StoryGridEvent::EditCard(card.id))
            } else {
                None
            }
        }
        StoryGridColumn::Document => {
            let mut event = None;
            if ctx.row.links.is_empty() {
                ui.weak("(no document)");
            } else {
                ui.horizontal_wrapped(|ui| {
                    for link in &ctx.row.links {
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
            event
        }
        StoryGridColumn::Pov => {
            match (ctx.pov.as_deref(), ctx.pov_color) {
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
            }
            None
        }
        StoryGridColumn::Words => {
            let text = ctx
                .words
                .map(|words| words.to_string())
                .unwrap_or_else(|| "\u{2014}".to_string());
            match ctx.word_count_color {
                Some(color) => {
                    ui.colored_label(color, text);
                }
                None => {
                    ui.label(text);
                }
            }
            None
        }
        StoryGridColumn::Cause => {
            ui.label(truncate(&card.cause));
            None
        }
        StoryGridColumn::Effect => {
            ui.label(truncate(&card.effect));
            None
        }
        StoryGridColumn::WhyItMatters => {
            ui.label(truncate(&card.why_it_matters));
            None
        }
        StoryGridColumn::Realization => {
            ui.label(truncate(&card.realization));
            None
        }
        StoryGridColumn::AndSo => {
            ui.label(truncate(&card.and_so));
            None
        }
        StoryGridColumn::PriorBelief => {
            ui.label(truncate(&card.prior_belief));
            None
        }
        StoryGridColumn::NewBelief => {
            ui.label(truncate(&card.new_belief));
            None
        }
        StoryGridColumn::ValueShift => {
            ui.label(truncate(&card.value_shift));
            None
        }
        StoryGridColumn::SubplotTags => {
            ui.label(card.subplot_tags.join(", "));
            None
        }
    }
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
/// `project::model::document_label`.
fn document_label(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    crate::project::model::document_label(name).to_string()
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

    fn placed_row(card: &StoryCard, position: usize) -> ResolvedRow<'_> {
        ResolvedRow {
            card,
            links: vec![ResolvedLink {
                stem: "stub".to_string(),
                path: None,
                position: Some(position),
            }],
        }
    }

    fn unplaced_row(card: &StoryCard) -> ResolvedRow<'_> {
        ResolvedRow {
            card,
            links: vec![],
        }
    }

    fn row_ids(rows: &[ResolvedRow]) -> Vec<Uuid> {
        rows.iter().map(|row| row.card.id).collect()
    }

    #[test]
    fn order_rows_in_manuscript_mode_sorts_by_position_regardless_of_input_order() {
        let a = StoryCard::new();
        let b = StoryCard::new();
        let rows = vec![placed_row(&b, 2), placed_row(&a, 1)];

        let ordered = order_rows(
            rows,
            StoryGridOrderMode::Manuscript,
            UnplacedCardsPosition::Bottom,
        );

        assert_eq!(row_ids(&ordered), vec![a.id, b.id]);
    }

    #[test]
    fn order_rows_in_manuscript_mode_breaks_position_ties_by_card_id_not_input_order() {
        // Sorted so the assertion doesn't depend on `Uuid::new_v4`'s randomness.
        let (mut a, mut b) = (StoryCard::new(), StoryCard::new());
        if a.id > b.id {
            std::mem::swap(&mut a, &mut b);
        }

        let forward = order_rows(
            vec![placed_row(&a, 1), placed_row(&b, 1)],
            StoryGridOrderMode::Manuscript,
            UnplacedCardsPosition::Bottom,
        );
        // Reversed input order, e.g. left behind by a `Manual`-mode reorder.
        let reversed = order_rows(
            vec![placed_row(&b, 1), placed_row(&a, 1)],
            StoryGridOrderMode::Manuscript,
            UnplacedCardsPosition::Bottom,
        );

        assert_eq!(row_ids(&forward), vec![a.id, b.id]);
        assert_eq!(row_ids(&reversed), vec![a.id, b.id]);
    }

    #[test]
    fn order_rows_in_manuscript_mode_breaks_unplaced_ties_by_card_id_not_input_order() {
        let (mut a, mut b) = (StoryCard::new(), StoryCard::new());
        if a.id > b.id {
            std::mem::swap(&mut a, &mut b);
        }

        let forward = order_rows(
            vec![unplaced_row(&a), unplaced_row(&b)],
            StoryGridOrderMode::Manuscript,
            UnplacedCardsPosition::Bottom,
        );
        let reversed = order_rows(
            vec![unplaced_row(&b), unplaced_row(&a)],
            StoryGridOrderMode::Manuscript,
            UnplacedCardsPosition::Bottom,
        );

        assert_eq!(row_ids(&forward), vec![a.id, b.id]);
        assert_eq!(row_ids(&reversed), vec![a.id, b.id]);
    }

    #[test]
    fn order_rows_in_manual_mode_preserves_input_order() {
        let a = StoryCard::new();
        let b = StoryCard::new();
        let rows = vec![placed_row(&b, 1), unplaced_row(&a)];

        let ordered = order_rows(rows, StoryGridOrderMode::Manual, UnplacedCardsPosition::Top);

        assert_eq!(row_ids(&ordered), vec![b.id, a.id]);
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
