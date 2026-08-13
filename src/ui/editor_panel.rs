use egui::text::{CCursor, CCursorRange};
use egui::widgets::text_edit::TextEditOutput;
use egui::{Id, Key, KeyboardShortcut, Modifiers};

use crate::autocomplete::{
    active_tag_query, active_wikilink_query, apply_tag_completion, apply_wikilink_completion,
    byte_offset_to_char, char_offset_to_byte, filter_candidates,
};
use crate::editor::EditorState;
use crate::editor_font::EditorFont;
use crate::markdown::{wikilink_resolves, wikilink_spans, wikilink_target_at};
use crate::spellcheck::SpellCheckLanguage;
use crate::ui::WikilinkActivation;

/// What happened this frame, for the caller to react to: a failed autosave, or the
/// user asking (via Ctrl+Enter) to follow the `[[wikilink]]` the cursor is on.
pub enum EditorEvent {
    SaveError(String),
    Wikilink(WikilinkActivation),
}

/// Cap on how many `[[wikilink]]`/`#tag` suggestions are shown at once, matching
/// Obsidian's short, scannable popup rather than dumping the whole vault into a list.
const MAX_SUGGESTIONS: usize = 8;

/// Cross-frame state for the wikilink/tag autocomplete popup (shared between the two —
/// only one can ever be active at a given cursor position, see `ActiveQuery`), stored
/// in egui's temporary widget memory (keyed off the editor's `TextEdit` id) rather than
/// in `EditorState`, since it's transient UI state, not part of the document being
/// edited.
#[derive(Clone, Default)]
struct AutocompleteState {
    open: bool,
    /// The `query_start` (byte offset) of the wikilink/tag this popup belongs to, used
    /// to tell "still the same query" from "cursor moved to a different `[[`/`#`".
    query_start: Option<usize>,
    /// The `query_start` of a query the user dismissed with Escape, so it doesn't
    /// immediately reopen every frame while the cursor stays inside it.
    dismissed_at: Option<usize>,
    selected: usize,
}

/// Which kind of in-progress query the autocomplete popup is currently open for —
/// determines both the candidate source (`note_titles` vs `tag_names`) and how an
/// accepted suggestion gets spliced into the buffer (`apply_wikilink_completion`'s
/// bracket handling vs `apply_tag_completion`'s plain replacement).
#[derive(Clone, Copy, PartialEq, Eq)]
enum QueryKind {
    Wikilink,
    Tag,
}

/// An in-progress `[[query`/`#query` at the cursor, tagged with which one it is —
/// `active_wikilink_query`/`active_tag_query` unified into one type so the rest of
/// `show` doesn't need two parallel copies of the popup-driving state machine.
/// Wikilink takes priority when (in some contrived input) both could match at once,
/// simply because it's checked first — an edge case not worth resolving more cleverly.
struct ActiveQuery {
    kind: QueryKind,
    query_start: usize,
    query: String,
}

fn active_query(text: &str, cursor: usize) -> Option<ActiveQuery> {
    if let Some(q) = active_wikilink_query(text, cursor) {
        return Some(ActiveQuery {
            kind: QueryKind::Wikilink,
            query_start: q.query_start,
            query: q.query,
        });
    }
    active_tag_query(text, cursor).map(|q| ActiveQuery {
        kind: QueryKind::Tag,
        query_start: q.query_start,
        query: q.query,
    })
}

enum PopupAction {
    Next,
    Prev,
    Confirm,
    Dismiss,
}

/// Cross-frame memo of `spellcheck::misspelled_word_spans`'s result, stored in
/// egui's temporary widget memory the same way `AutocompleteState` is — keyed
/// off the editor's `TextEdit` id, not part of `EditorState`'s own data model.
/// Dictionary lookups are real per-word CPU work (unlike the cheap regex-ish
/// span scans `build_editor_layout_job` already runs every frame), so this
/// avoids re-running them on every frame the buffer hasn't actually changed —
/// recomputed only when the buffer's hash or the active language changes.
#[derive(Clone, Default)]
struct MisspelledCache {
    text_hash: u64,
    language: SpellCheckLanguage,
    spans: Vec<std::ops::Range<usize>>,
}

fn hash_text(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Stable id for the document `TextEdit`, independent of whatever panel happens to
/// host it this frame — lets `app.rs` move its cursor (e.g. jumping to a
/// find-and-replace result) without needing a `Ui` of its own to derive an id from.
pub fn editor_text_edit_id() -> Id {
    Id::new("smaragd_editor_text_edit")
}

/// Renders the document editor, including a `[[wikilink]]`/`#tag` autocomplete
/// popup driven by `note_titles`/`tag_names` respectively (see `ActiveQuery`).
/// Returns `Some` if an autosave triggered by focus loss failed, or the user
/// pressed `activate_wikilink_shortcut` (the remappable
/// `ShortcutAction::ActivateWikilink`, `Ctrl+Enter`/`Cmd+Enter` by default —
/// `None` if the user unbound it) on a wikilink to follow it — the caller
/// decides what to do with either.
///
/// `focus_mode` enables Focus Mode's "typewriter" effect: the paragraph
/// containing the cursor renders at full strength, every other paragraph
/// dimmed — see `paragraph_byte_range`. Independent of `focus_mode`, every
/// `[[wikilink]]` in the buffer is always colored, the app's
/// normal link color if `note_titles` has a matching document, a distinct
/// "broken link" color otherwise — see `build_editor_layout_job`.
///
/// `font`/`font_size` are `Settings::editor_font`/`editor_font_size` (already
/// resolved via `editor_font::resolve_size` — this takes a real point size, not
/// the raw possibly-`0.0` setting), shared with the Preview.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    note_titles: &[String],
    tag_names: &[String],
    activate_wikilink_shortcut: Option<KeyboardShortcut>,
    focus_mode: bool,
    font: EditorFont,
    font_size: f32,
    collaborating: bool,
    spell_check_language: SpellCheckLanguage,
    show_gutter: bool,
) -> Option<EditorEvent> {
    // A joined collaboration session deliberately has no `open_path` (it
    // isn't tied to any of the joiner's own files — see `CollabSession`'s
    // module doc), but its shared content still needs to be visible and
    // editable: without this, there'd be nothing to look at but "Select a
    // file..." even once real content has arrived, an irresistible pull to
    // go click something in the binder — which would then end the session
    // (see `open_document`'s teardown guard) with no obvious explanation.
    // `EditorState::save` already no-ops safely with no `open_path`, so
    // rendering here doesn't risk trying to save collaboratively-received
    // content to a file that doesn't exist.
    if editor.open_path.is_none() && !collaborating {
        ui.label("Select a file from the binder to start editing.");
        return None;
    }

    // Obsidian-style document title at the top of the pane, above the text itself —
    // the dock tab's own label just says "Editor" (see `DockTab::title`), so without
    // this there's nothing in the pane confirming which document is actually open.
    // The filename stem matches every other place a document's title is shown
    // (binder rows, `Project::tree.document_names`, wikilink targets). Skipped for a
    // joined collaboration session, which has no `open_path` of its own to name.
    if let Some(title) = editor
        .open_path
        .as_deref()
        .and_then(|path| path.file_stem())
        .and_then(|stem| stem.to_str())
    {
        ui.heading(title);
        ui.add_space(4.0);
    }

    let text_edit_id = editor_text_edit_id();

    // A document switch (`SmaragdApp::load_document`) leaves a byte offset here
    // for the cursor to jump to on the document's first render — restoring
    // where the user last left off in it (tracked by `document_history`),
    // rather than carrying over wherever the cursor happened to be in
    // whichever document was open before. Consumed (and cleared) unconditionally
    // so it never re-fires on a later frame once the `TextEdit` below has
    // already picked it up.
    if let Some(byte_offset) = editor.pending_cursor.take() {
        move_cursor_to(ui.ctx(), text_edit_id, &editor.buffer, byte_offset);
    }

    let state_id = text_edit_id.with("wikilink_autocomplete");
    let state: AutocompleteState = ui
        .ctx()
        .data_mut(|d| d.get_temp(state_id))
        .unwrap_or_default();

    // If the popup was showing after the last frame, steal navigation keys before the
    // `TextEdit` below gets a chance to treat them as ordinary cursor movement or
    // newline insertion. `activate_wikilink_shortcut` is stolen unconditionally —
    // the `TextEdit` would otherwise turn a bare Enter-based binding into a plain
    // newline.
    let pending_action = state.open.then(|| steal_popup_key(ui)).flatten();
    let activate_wikilink_requested = activate_wikilink_shortcut
        .is_some_and(|shortcut| ui.ctx().input_mut(|i| i.consume_shortcut(&shortcut)));

    // Cursor position as of the *previous* frame (read from egui's own persisted
    // `TextEdit` state, the same mechanism `move_cursor_to` below uses) — the
    // layouter runs as part of building this frame's output, so it has no way to
    // see this frame's own cursor position; a one-frame lag here is standard
    // practice and imperceptible.
    let focus_mode_cursor_byte = focus_mode
        .then(|| {
            egui::TextEdit::load_state(ui.ctx(), text_edit_id)
                .and_then(|state| state.cursor.char_range())
                .map(|range| char_offset_to_byte(&editor.buffer, range.primary.index.0))
        })
        .flatten();
    let misspelled_cache_id = text_edit_id.with("misspelled_cache");
    let mut editor_layouter = move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let text = buf.as_str();

        let misspelled = if spell_check_language == SpellCheckLanguage::Off {
            Vec::new()
        } else {
            let cache: MisspelledCache = ui
                .ctx()
                .data_mut(|d| d.get_temp(misspelled_cache_id))
                .unwrap_or_default();
            let text_hash = hash_text(text);
            if cache.text_hash == text_hash && cache.language == spell_check_language {
                cache.spans
            } else {
                let spans = crate::spellcheck::misspelled_word_spans(text, |word| {
                    crate::spellcheck::is_misspelled(word, spell_check_language)
                });
                ui.ctx().data_mut(|d| {
                    d.insert_temp(
                        misspelled_cache_id,
                        MisspelledCache {
                            text_hash,
                            language: spell_check_language,
                            spans: spans.clone(),
                        },
                    )
                });
                spans
            }
        };

        let job = build_editor_layout_job(
            ui,
            text,
            font.font_id(font_size),
            focus_mode_cursor_byte.map(|b| b.min(text.len())),
            note_titles,
            wrap_width,
            &misspelled,
        );
        ui.fonts_mut(|f| f.layout_job(job))
    };

    // `TextEdit`'s allocated (and thus *interactive* — this isn't just cosmetic)
    // height is `desired_rows` rows at minimum, regardless of how much content it
    // actually holds — unlike `min_size`'s height component, which despite its
    // name is silently ignored by egui. A short document left at the default of
    // 4 rows would visually fill the `ScrollArea` below (background color, no
    // border) but not actually respond to clicks past its last real line, since
    // the widget's own hit-test rect stops at content height — so clicking lower
    // in what looks like the editor does nothing. Sizing `desired_rows` to the
    // tab's currently available height (computed before entering the
    // `ScrollArea`, whose own inner `Ui` reports a much larger "available"
    // height to allow scrolling past it) makes the widget's real interactive
    // area match what it looks like it covers; a longer document still grows
    // and scrolls past that minimum exactly as before.
    let available_height = ui.available_height();
    let row_height = ui.fonts_mut(|f| f.row_height(&font.font_id(font_size)));
    let desired_rows = ((available_height / row_height).floor() as usize).max(1);

    // Gutter sizing, done up front so it can be reserved (via `add_space`,
    // below) before the `TextEdit` itself is laid out — it needs to already
    // be narrower by this much for `editor_layouter`'s `wrap_width` to wrap
    // text before it would run under the gutter. `icon_width` (a
    // one-row-tall/wide blank square, left of the numbers) is unused today —
    // reserved space for a future per-line bookmark icon, see `paint_gutter`.
    // Numbers right-align, so the column only needs to be as wide as the
    // document's own line count actually requires, not a fixed guess.
    let icon_width = row_height;
    let line_count = editor.buffer.matches('\n').count() + 1;
    let digit_count = line_count.to_string().len().max(2);
    let digit_width = ui.fonts_mut(|f| f.glyph_width(&font.font_id(font_size), '0'));
    let number_width = digit_width * digit_count as f32;
    let gutter_padding = 8.0;
    let gutter_width = icon_width + number_width + gutter_padding;

    let output = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if show_gutter {
                    ui.add_space(gutter_width);
                }
                // No border: `TextEdit`'s own frame only ever wraps its *content*
                // height (a few short paragraphs, say), never the full `ScrollArea`
                // around it — with the frame left on, a short document renders as a
                // small boxed page sitting in a lot of otherwise-dead-looking empty
                // space below it, rather than one editable area that fills the tab.
                // `lock_focus` (normally bundled into `.code_editor()`, which also
                // hardcodes the Monospace font — not wanted now that the font is
                // configurable) keeps Tab inserting a tab character instead of
                // leaving the field, still desirable for a plain-text editor.
                let text_edit = egui::TextEdit::multiline(&mut editor.buffer)
                    .desired_width(f32::INFINITY)
                    .desired_rows(desired_rows)
                    .font(font.font_id(font_size))
                    .lock_focus(true)
                    .frame(egui::Frame::NONE)
                    .id(text_edit_id)
                    .layouter(&mut editor_layouter);
                let text_output = text_edit.show(ui);
                if show_gutter {
                    paint_gutter(
                        ui,
                        &text_output.galley,
                        text_output.galley_pos,
                        text_output.galley_pos.x - gutter_padding,
                        font.font_id(font_size),
                    );
                }
                text_output
            })
            .inner
        })
        .inner;

    if output.response.changed() {
        editor.mark_dirty();
    }

    // Kept fresh every frame the editor renders, regardless of whether the
    // cursor actually moved this frame — `document_history` reads this
    // whenever the user navigates to a *different* document, to remember
    // where they were leaving this one.
    if let Some(range) = output.cursor_range {
        editor.cursor_byte = char_offset_to_byte(&editor.buffer, range.primary.index.0);
    }

    if activate_wikilink_requested && let Some(range) = output.cursor_range {
        let cursor_byte = char_offset_to_byte(&editor.buffer, range.primary.index.0);
        if let Some(target) = wikilink_target_at(&editor.buffer, cursor_byte) {
            return Some(EditorEvent::Wikilink(WikilinkActivation {
                target,
                force_create: true,
            }));
        }
    }

    let active = output.cursor_range.and_then(|range| {
        let cursor_char = range.primary.index.0;
        let cursor_byte = char_offset_to_byte(&editor.buffer, cursor_char);
        active_query(&editor.buffer, cursor_byte).map(|query| (cursor_char, cursor_byte, query))
    });

    let mut completion: Option<(usize, usize, QueryKind, String)> = None; // (query_start, cursor_byte, kind, chosen)

    let new_state = match active {
        None => AutocompleteState::default(),
        Some((_, _, query)) if state.dismissed_at == Some(query.query_start) => AutocompleteState {
            dismissed_at: state.dismissed_at,
            ..AutocompleteState::default()
        },
        Some((cursor_char, cursor_byte, query)) => {
            let source = match query.kind {
                QueryKind::Wikilink => note_titles,
                QueryKind::Tag => tag_names,
            };
            let all_candidates = filter_candidates(source, &query.query);
            let candidates = &all_candidates[..all_candidates.len().min(MAX_SUGGESTIONS)];

            if candidates.is_empty() {
                AutocompleteState::default()
            } else if matches!(pending_action, Some(PopupAction::Dismiss)) {
                AutocompleteState {
                    dismissed_at: Some(query.query_start),
                    ..AutocompleteState::default()
                }
            } else {
                let is_new_query = state.query_start != Some(query.query_start);
                let mut selected = if is_new_query {
                    0
                } else {
                    state.selected.min(candidates.len() - 1)
                };
                match pending_action {
                    Some(PopupAction::Next) => selected = (selected + 1) % candidates.len(),
                    Some(PopupAction::Prev) => {
                        selected = (selected + candidates.len() - 1) % candidates.len();
                    }
                    _ => {}
                }

                let confirmed_by_key = matches!(pending_action, Some(PopupAction::Confirm));
                let clicked = if confirmed_by_key {
                    None
                } else {
                    render_popup(ui, &output, cursor_char, candidates, selected)
                };

                if let Some(index) = clicked.or(confirmed_by_key.then_some(selected)) {
                    completion = Some((
                        query.query_start,
                        cursor_byte,
                        query.kind,
                        candidates[index].to_string(),
                    ));
                    AutocompleteState::default()
                } else {
                    AutocompleteState {
                        open: true,
                        query_start: Some(query.query_start),
                        dismissed_at: None,
                        selected,
                    }
                }
            }
        }
    };

    ui.ctx().data_mut(|d| d.insert_temp(state_id, new_state));

    if let Some((query_start, cursor_byte, kind, chosen)) = completion {
        let (new_text, new_cursor_byte) = match kind {
            QueryKind::Wikilink => {
                apply_wikilink_completion(&editor.buffer, query_start, cursor_byte, &chosen)
            }
            QueryKind::Tag => {
                apply_tag_completion(&editor.buffer, query_start, cursor_byte, &chosen)
            }
        };
        editor.buffer = new_text;
        editor.mark_dirty();
        move_cursor_to(ui.ctx(), text_edit_id, &editor.buffer, new_cursor_byte);
    }

    if output.response.lost_focus()
        && let Err(err) = editor.save()
    {
        return Some(EditorEvent::SaveError(format!("Save failed: {err}")));
    }

    None
}

/// The byte range `[start, end)` of the paragraph (a run of text between blank
/// lines, or the start/end of the buffer) containing `cursor_byte` — used by
/// Focus Mode's typewriter dimming to decide which paragraph stays at full
/// strength. `cursor_byte` must be `<= text.len()`.
fn paragraph_byte_range(text: &str, cursor_byte: usize) -> std::ops::Range<usize> {
    let start = text[..cursor_byte].rfind("\n\n").map_or(0, |i| i + 2);
    let end = text[cursor_byte..]
        .find("\n\n")
        .map_or(text.len(), |i| cursor_byte + i);
    start..end
}

/// Builds the `TextEdit` layouter's output for the document editor: Focus
/// Mode's typewriter dimming (the paragraph containing `focus_cursor_byte`,
/// if given, at full strength, everything else in `ui.visuals().weak_text_color()`)
/// composed with `[[wikilink]]` coloring — every wikilink in
/// `text` (found via `wikilink_spans`, which covers the whole `[[...]]` span
/// including its brackets) renders in the app's normal hyperlink color if its
/// target matches one of `note_titles`, or a distinct "broken link" color
/// (`ui.visuals().error_fg_color`) otherwise — regardless of whether it falls
/// inside or outside the dimmed range, so an unresolved link stays equally
/// visible either way. Composed further with a spell-check underline: each
/// range in `misspelled` (see `spellcheck::misspelled_word_spans`, precomputed
/// by the caller — this function does no dictionary lookups itself) gets a
/// solid underline in `ui.visuals().warn_fg_color`, unless that run is already
/// inside a wikilink — a wikilink keeps its link/broken-link color with no
/// underline added even if its display text also happens to be a dictionary
/// miss, so the two indicators never compete on the same text.
///
/// Works by splitting `text` at every boundary any of these rules cares about
/// (the focus-paragraph edges, each wikilink's edges, each misspelled word's
/// edges), then picking one `TextFormat` per resulting run: a run inside a
/// wikilink always uses that link's color, overriding the dim/normal choice
/// Focus Mode would otherwise make for it.
fn build_editor_layout_job(
    ui: &egui::Ui,
    text: &str,
    font_id: egui::FontId,
    focus_cursor_byte: Option<usize>,
    note_titles: &[String],
    wrap_width: f32,
    misspelled: &[std::ops::Range<usize>],
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width: wrap_width,
            ..Default::default()
        },
        ..Default::default()
    };

    let normal = ui.visuals().text_color();
    let dim = ui.visuals().weak_text_color();
    let link_color = ui.visuals().hyperlink_color;
    let broken_link_color = ui.visuals().error_fg_color;
    let misspell_color = ui.visuals().warn_fg_color;

    let focus_range = focus_cursor_byte.map(|cursor_byte| paragraph_byte_range(text, cursor_byte));
    let wikilinks = wikilink_spans(text);

    let mut boundaries: Vec<usize> = vec![0, text.len()];
    if let Some(range) = &focus_range {
        boundaries.push(range.start);
        boundaries.push(range.end);
    }
    for (range, _) in &wikilinks {
        boundaries.push(range.start);
        boundaries.push(range.end);
    }
    for range in misspelled {
        boundaries.push(range.start);
        boundaries.push(range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    for pair in boundaries.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start == end {
            continue;
        }
        let wikilink_here = wikilinks
            .iter()
            .find(|(range, _)| range.start <= start && end <= range.end);
        let color = if let Some((_, target)) = wikilink_here {
            if wikilink_resolves(target, note_titles) {
                link_color
            } else {
                broken_link_color
            }
        } else if focus_range
            .as_ref()
            .is_some_and(|range| range.start <= start && end <= range.end)
            || focus_range.is_none()
        {
            normal
        } else {
            dim
        };
        let underline = if wikilink_here.is_none()
            && misspelled
                .iter()
                .any(|range| range.start <= start && end <= range.end)
        {
            egui::Stroke::new(1.0, misspell_color)
        } else {
            egui::Stroke::default()
        };
        job.append(
            &text[start..end],
            0.0,
            egui::TextFormat {
                font_id: font_id.clone(),
                color,
                underline,
                ..Default::default()
            },
        );
    }
    job
}

/// Draws the line-number column to the left of the editor's `TextEdit`, one
/// number per *logical* line — a run of text ending in a real `\n` — rather
/// than one per wrapped visual row: a long paragraph's wrapped continuation
/// rows get no number of their own, the convention word-wrap-aware code
/// editors (VS Code, Sublime, etc.) use. Reads row positions straight off
/// the `TextEdit`'s own just-shown `galley`/`galley_pos` (`ends_with_newline`
/// on each `PlacedRow` marks the end of a logical line) instead of
/// re-deriving them from the buffer itself, so numbering can never drift out
/// of sync with what the `TextEdit` actually painted this frame.
///
/// `number_right_x` is where each number right-aligns to; the caller derives
/// it from `galley_pos.x` (see `show`), since that already reflects however
/// much space was reserved for the whole gutter. The reserved space further
/// left of it (`icon_width` in `show`) is left blank here — a slot for a
/// future per-line bookmark icon, not implemented yet.
fn paint_gutter(
    ui: &egui::Ui,
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    number_right_x: f32,
    font_id: egui::FontId,
) {
    let painter = ui.painter();
    let color = ui.visuals().weak_text_color();
    let mut line_number: usize = 1;
    let mut at_line_start = true;
    for row in &galley.rows {
        if at_line_start {
            painter.text(
                egui::pos2(number_right_x, galley_pos.y + row.pos.y),
                egui::Align2::RIGHT_TOP,
                line_number.to_string(),
                font_id.clone(),
                color,
            );
        }
        at_line_start = row.ends_with_newline;
        if row.ends_with_newline {
            line_number += 1;
        }
    }
}

/// Consume (and act on) a keypress meant for the autocomplete popup, so the `TextEdit`
/// underneath never sees it — otherwise Enter would insert a newline and the arrow
/// keys would move the text cursor instead of the popup's selection.
fn steal_popup_key(ui: &mut egui::Ui) -> Option<PopupAction> {
    ui.ctx().input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            Some(PopupAction::Next)
        } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            Some(PopupAction::Prev)
        } else if i.consume_key(Modifiers::NONE, Key::Enter)
            || i.consume_key(Modifiers::NONE, Key::Tab)
        {
            Some(PopupAction::Confirm)
        } else if i.consume_key(Modifiers::NONE, Key::Escape) {
            Some(PopupAction::Dismiss)
        } else {
            None
        }
    })
}

/// Draw the suggestion list just below the cursor. Returns the clicked candidate's
/// index, if the user picked one with the mouse this frame.
fn render_popup(
    ui: &mut egui::Ui,
    output: &TextEditOutput,
    cursor_char: usize,
    candidates: &[&str],
    selected: usize,
) -> Option<usize> {
    let cursor_rect = output.galley.pos_from_cursor(CCursor::new(cursor_char));
    let popup_pos = output.galley_pos + cursor_rect.left_bottom().to_vec2();

    let mut clicked = None;
    egui::Area::new(Id::new("wikilink_autocomplete_popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(popup_pos)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                for (index, candidate) in candidates.iter().enumerate() {
                    if ui.selectable_label(index == selected, *candidate).clicked() {
                        clicked = Some(index);
                    }
                }
            });
        });
    clicked
}

/// Move the `TextEdit`'s cursor to `byte_offset` and give it focus back. Used both to
/// leave the caret right after an accepted wikilink suggestion, and by `app.rs` to
/// jump to a find-and-replace result. A no-op if the `TextEdit` has never been shown
/// yet this session (e.g. jumping to a result before any document has been opened).
pub fn move_cursor_to(ctx: &egui::Context, id: Id, text: &str, byte_offset: usize) {
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        let char_offset = byte_offset_to_char(text, byte_offset);
        state
            .cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(char_offset))));
        egui::TextEdit::store_state(ctx, id, state);
        ctx.memory_mut(|m| m.request_focus(id));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SpellCheckLanguage, build_editor_layout_job, editor_text_edit_id, paragraph_byte_range,
        show,
    };
    use crate::editor::EditorState;
    use crate::editor_font::EditorFont;

    /// Reproduces the reported bug: a short document's actual *interactive* area
    /// (what you can click to focus/place the cursor) used to stop at content
    /// height — a handful of lines — even once the visible background had
    /// already been made to fill the whole tab, leaving a large dead zone below
    /// the last line that looked editable but wasn't. Drives `show` inside a
    /// fixed-size viewport and checks the `TextEdit`'s own allocated rect (read
    /// back via `Context::read_response`, since `show`'s return value doesn't
    /// expose it) actually reaches down close to the bottom of that viewport.
    /// Regression test for the Obsidian-style document title: the open document's
    /// filename stem is shown as a heading above the editable area, pushing the
    /// `TextEdit` down from the top of the pane — checked geometrically (the
    /// `TextEdit`'s own top y-coordinate) rather than by inspecting rendered text,
    /// matching this file's other layout regression test
    /// (`a_short_document_s_editable_area_fills_the_available_height`) — egui gives
    /// no simpler way to assert "this text was painted" than measuring what it
    /// pushed around it. Needs `editor_font::install` first: without a real font
    /// registered, the heading measures to zero height and the regression this
    /// guards against would go undetected.
    #[test]
    fn the_open_document_s_title_is_shown_as_a_heading_above_the_editor() {
        let ctx = egui::Context::default();
        crate::editor_font::install(&ctx);
        let mut editor = EditorState {
            open_path: Some(std::path::PathBuf::from("Chapter One.md")),
            buffer: "Some text.".to_string(),
            ..Default::default()
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };

        let _ = ctx.run_ui(input, |ui| {
            show(
                ui,
                &mut editor,
                &[],
                &[],
                None,
                false,
                EditorFont::Monospace,
                14.0,
                false,
                SpellCheckLanguage::Off,
                false,
            );
        });

        let top = ctx
            .read_response(editor_text_edit_id())
            .expect("TextEdit renders once a document is open")
            .rect
            .top();
        assert!(
            top > 15.0,
            "expected the TextEdit to sit below a document-title heading, but it starts at y={top}"
        );
    }

    /// A joined collaboration session has no `open_path` of its own to name (see
    /// `collaborating_with_no_open_path_still_renders_the_editor`), so no title
    /// heading should be shown — the `TextEdit` should sit right at the top of the
    /// pane, the same as before this heading existed.
    #[test]
    fn no_title_heading_is_shown_while_collaborating_with_no_open_document() {
        let ctx = egui::Context::default();
        crate::editor_font::install(&ctx);
        let mut editor = EditorState {
            open_path: None,
            buffer: "Shared content from a peer".to_string(),
            ..Default::default()
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };

        let _ = ctx.run_ui(input, |ui| {
            show(
                ui,
                &mut editor,
                &[],
                &[],
                None,
                false,
                EditorFont::Monospace,
                14.0,
                true,
                SpellCheckLanguage::Off,
                false,
            );
        });

        let top = ctx
            .read_response(editor_text_edit_id())
            .expect("TextEdit renders while collaborating even with no open_path")
            .rect
            .top();
        assert!(
            top < 15.0,
            "expected no title heading (no open_path to name) so the TextEdit sits at the \
             top of the pane, but it starts at y={top}"
        );
    }

    #[test]
    fn a_short_document_s_editable_area_fills_the_available_height() {
        let ctx = egui::Context::default();
        let mut editor = EditorState {
            open_path: Some(std::path::PathBuf::from("scene.md")),
            buffer: "One short line.".to_string(),
            ..Default::default()
        };
        let viewport_height = 600.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, viewport_height),
            )),
            ..Default::default()
        };

        let _ = ctx.run_ui(input, |ui| {
            show(
                ui,
                &mut editor,
                &[],
                &[],
                None,
                false,
                EditorFont::Monospace,
                14.0,
                false,
                SpellCheckLanguage::Off,
                false,
            );
        });

        let response = ctx
            .read_response(editor_text_edit_id())
            .expect("TextEdit renders once a document is open");
        assert!(
            response.rect.height() > viewport_height * 0.8,
            "expected the editable area to fill most of a {viewport_height}px-tall \
             viewport for a one-line document, got {}px",
            response.rect.height()
        );
    }

    /// `show_gutter: true` must reserve real horizontal space for the
    /// line-number column *before* the `TextEdit` itself, not paint numbers
    /// on top of the text — checked geometrically (the `TextEdit`'s own left
    /// x-coordinate) since, like the title-heading regression test above,
    /// there's no simpler way to assert "this took up space" than measuring
    /// what it pushed the next thing away from.
    #[test]
    fn the_text_edit_starts_further_right_when_the_gutter_is_shown() {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };

        let left_without_gutter = {
            let ctx = egui::Context::default();
            let mut editor = EditorState {
                open_path: Some(std::path::PathBuf::from("scene.md")),
                buffer: "One short line.".to_string(),
                ..Default::default()
            };
            let _ = ctx.run_ui(input.clone(), |ui| {
                show(
                    ui,
                    &mut editor,
                    &[],
                    &[],
                    None,
                    false,
                    EditorFont::Monospace,
                    14.0,
                    false,
                    SpellCheckLanguage::Off,
                    false,
                );
            });
            ctx.read_response(editor_text_edit_id())
                .unwrap()
                .rect
                .left()
        };

        let left_with_gutter = {
            let ctx = egui::Context::default();
            let mut editor = EditorState {
                open_path: Some(std::path::PathBuf::from("scene.md")),
                buffer: "One short line.".to_string(),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                show(
                    ui,
                    &mut editor,
                    &[],
                    &[],
                    None,
                    false,
                    EditorFont::Monospace,
                    14.0,
                    false,
                    SpellCheckLanguage::Off,
                    true,
                );
            });
            ctx.read_response(editor_text_edit_id())
                .unwrap()
                .rect
                .left()
        };

        assert!(
            left_with_gutter > left_without_gutter,
            "expected the gutter to push the TextEdit right (without: {left_without_gutter}, \
             with: {left_with_gutter})"
        );
    }

    /// Regression test for a real bug: a joined collaboration session
    /// deliberately has no `open_path` (it isn't tied to any of the
    /// joiner's own files), but its shared content still needs to be
    /// visible — without the `collaborating` bypass, the joiner would see
    /// only "Select a file..." even after content arrived, with nothing to
    /// do but click around in the binder, which then silently ends the
    /// session (see `open_document`'s teardown guard).
    #[test]
    fn collaborating_with_no_open_path_still_renders_the_editor() {
        let ctx = egui::Context::default();
        let mut editor = EditorState {
            open_path: None,
            buffer: "Shared content from a peer".to_string(),
            ..Default::default()
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };

        let _ = ctx.run_ui(input, |ui| {
            show(
                ui,
                &mut editor,
                &[],
                &[],
                None,
                false,
                EditorFont::Monospace,
                14.0,
                true,
                SpellCheckLanguage::Off,
                false,
            );
        });

        assert!(
            ctx.read_response(editor_text_edit_id()).is_some(),
            "expected the TextEdit to render while collaborating, even with no open_path"
        );
    }

    #[test]
    fn not_collaborating_with_no_open_path_shows_the_placeholder_instead() {
        let ctx = egui::Context::default();
        let mut editor = EditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };

        let _ = ctx.run_ui(input, |ui| {
            show(
                ui,
                &mut editor,
                &[],
                &[],
                None,
                false,
                EditorFont::Monospace,
                14.0,
                false,
                SpellCheckLanguage::Off,
                false,
            );
        });

        assert!(
            ctx.read_response(editor_text_edit_id()).is_none(),
            "expected no TextEdit to render with nothing open and no collaboration session"
        );
    }

    /// Finds whichever `LayoutJob` section covers `byte_offset` and returns its
    /// color — the sections returned by `build_editor_layout_job` don't line up
    /// 1:1 with any particular substring, so tests locate the one they care
    /// about by position rather than by index.
    fn color_at(sections: &[egui::text::LayoutSection], byte_offset: usize) -> egui::Color32 {
        sections
            .iter()
            .find(|s| s.byte_range.start.0 <= byte_offset && byte_offset < s.byte_range.end.0)
            .unwrap_or_else(|| panic!("no section covers byte {byte_offset}"))
            .format
            .color
    }

    /// `color_at`'s sibling for the spell-check underline stroke.
    fn underline_at(sections: &[egui::text::LayoutSection], byte_offset: usize) -> egui::Stroke {
        sections
            .iter()
            .find(|s| s.byte_range.start.0 <= byte_offset && byte_offset < s.byte_range.end.0)
            .unwrap_or_else(|| panic!("no section covers byte {byte_offset}"))
            .format
            .underline
    }

    #[test]
    fn a_misspelled_word_gets_the_warning_underline_stroke() {
        let ctx = egui::Context::default();
        let text = "The wrold is big.";
        // A genuine `Vec<Range<usize>>` of misspelled-word spans, not (as the lint
        // assumes) an attempt to collect the range's own contents.
        #[allow(clippy::single_range_in_vec_init)]
        let misspelled = vec![text.find("wrold").unwrap()..text.find("wrold").unwrap() + 5];
        let (mut warn_color, mut sections) = (egui::Color32::TRANSPARENT, Vec::new());

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            warn_color = ui.visuals().warn_fg_color;
            sections = build_editor_layout_job(
                ui,
                text,
                egui::FontId::monospace(14.0),
                None,
                &[],
                1000.0,
                &misspelled,
            )
            .sections;
        });

        let stroke = underline_at(&sections, text.find("wrold").unwrap());
        assert_eq!(stroke.color, warn_color);
        assert!(stroke.width > 0.0);
        // Plain text right before it stays un-underlined.
        assert_eq!(
            underline_at(&sections, 0),
            egui::Stroke::default(),
            "plain text should not get the misspelled-word underline"
        );
    }

    /// A run inside a wikilink keeps its link/broken-link color with no
    /// underline added, even when its byte range also appears in `misspelled`
    /// — the two indicators must never compete on the same text.
    #[test]
    fn a_wikilink_run_never_gets_the_misspelled_underline_even_if_its_range_is_flagged() {
        let ctx = egui::Context::default();
        let text = "See [[Wrold]] for more.";
        let target_start = text.find("Wrold").unwrap();
        // The whole `[[Wrold]]` span, not just the word — matches how a real
        // caller would report a wikilink target/tag span as "skip" rather than
        // "flag" (see `spellcheck::misspelled_word_spans`), but this test
        // deliberately still passes the range in to prove the renderer itself
        // enforces the no-double-signal rule, not just the span-finder.
        #[allow(clippy::single_range_in_vec_init)]
        let misspelled = vec![target_start..target_start + 5];
        let mut sections = Vec::new();

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            sections = build_editor_layout_job(
                ui,
                text,
                egui::FontId::monospace(14.0),
                None,
                &[],
                1000.0,
                &misspelled,
            )
            .sections;
        });

        assert_eq!(
            underline_at(&sections, target_start),
            egui::Stroke::default(),
            "a wikilink run must never get the misspelled-word underline"
        );
    }

    #[test]
    fn a_resolved_wikilink_is_colored_differently_from_an_unresolved_one() {
        let ctx = egui::Context::default();
        let text = "See [[Known]] and [[Missing]] notes.";
        let note_titles = vec!["Known".to_string()];
        let (mut link_color, mut broken_color, mut sections) = (
            egui::Color32::TRANSPARENT,
            egui::Color32::TRANSPARENT,
            Vec::new(),
        );

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            link_color = ui.visuals().hyperlink_color;
            broken_color = ui.visuals().error_fg_color;
            sections = build_editor_layout_job(
                ui,
                text,
                egui::FontId::monospace(14.0),
                None,
                &note_titles,
                1000.0,
                &[],
            )
            .sections;
        });

        assert_ne!(link_color, broken_color, "test fixture assumption");
        assert_eq!(color_at(&sections, text.find("Known").unwrap()), link_color);
        assert_eq!(
            color_at(&sections, text.find("Missing").unwrap()),
            broken_color
        );
    }

    #[test]
    fn an_unresolved_wikilink_keeps_its_broken_color_even_outside_the_focus_paragraph() {
        let ctx = egui::Context::default();
        let text = "First paragraph, cursor here.\n\n[[Missing]] in the second paragraph.";
        let cursor_byte = 5; // inside the first paragraph
        let (mut broken_color, mut dim_color, mut sections) = (
            egui::Color32::TRANSPARENT,
            egui::Color32::TRANSPARENT,
            Vec::new(),
        );

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            broken_color = ui.visuals().error_fg_color;
            dim_color = ui.visuals().weak_text_color();
            sections = build_editor_layout_job(
                ui,
                text,
                egui::FontId::monospace(14.0),
                Some(cursor_byte),
                &[],
                1000.0,
                &[],
            )
            .sections;
        });

        assert_ne!(broken_color, dim_color, "test fixture assumption");
        // The plain text right before `[[Missing]]`, still inside the second
        // (unfocused) paragraph, should be dimmed...
        assert_eq!(
            color_at(&sections, text.find("in the second").unwrap()),
            dim_color
        );
        // ...but the wikilink itself stays at full "broken" strength rather
        // than inheriting that dimming.
        assert_eq!(
            color_at(&sections, text.find("Missing").unwrap()),
            broken_color
        );
    }

    #[test]
    fn single_paragraph_covers_the_whole_buffer() {
        let text = "Just one paragraph, no blank lines anywhere in it.";
        assert_eq!(paragraph_byte_range(text, 5), 0..text.len());
    }

    #[test]
    fn cursor_in_the_first_of_several_paragraphs() {
        let text = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
        let first_end = text.find("\n\n").unwrap();
        assert_eq!(paragraph_byte_range(text, 3), 0..first_end);
    }

    #[test]
    fn cursor_in_the_middle_of_several_paragraphs() {
        let text = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
        let second_start = text.find("\n\n").unwrap() + 2;
        let second_end = text.rfind("\n\n").unwrap();
        let cursor = second_start + 3;
        assert_eq!(paragraph_byte_range(text, cursor), second_start..second_end);
    }

    #[test]
    fn cursor_in_the_last_of_several_paragraphs() {
        let text = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
        let third_start = text.rfind("\n\n").unwrap() + 2;
        assert_eq!(
            paragraph_byte_range(text, text.len() - 2),
            third_start..text.len()
        );
    }

    #[test]
    fn cursor_at_the_very_start_of_the_buffer() {
        let text = "First paragraph.\n\nSecond paragraph.";
        let first_end = text.find("\n\n").unwrap();
        assert_eq!(paragraph_byte_range(text, 0), 0..first_end);
    }

    #[test]
    fn cursor_at_the_very_end_of_the_buffer() {
        let text = "First paragraph.\n\nSecond paragraph.";
        let second_start = text.find("\n\n").unwrap() + 2;
        assert_eq!(
            paragraph_byte_range(text, text.len()),
            second_start..text.len()
        );
    }
}
