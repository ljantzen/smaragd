use egui::text::{CCursor, CCursorRange};
use egui::widgets::text_edit::TextEditOutput;
use egui::{Id, Key, KeyboardShortcut, Modifiers};

use crate::autocomplete::{
    active_wikilink_query, apply_wikilink_completion, byte_offset_to_char, char_offset_to_byte,
    filter_candidates,
};
use crate::editor::EditorState;
use crate::editor_font::EditorFont;
use crate::markdown::wikilink_target_at;
use crate::ui::WikilinkActivation;

/// What happened this frame, for the caller to react to: a failed autosave, or the
/// user asking (via Ctrl+Enter) to follow the `[[wikilink]]` the cursor is on.
pub enum EditorEvent {
    SaveError(String),
    Wikilink(WikilinkActivation),
}

/// Cap on how many `[[wikilink]]` suggestions are shown at once, matching Obsidian's
/// short, scannable popup rather than dumping the whole vault into a list.
const MAX_SUGGESTIONS: usize = 8;

/// Cross-frame state for the wikilink autocomplete popup, stored in egui's temporary
/// widget memory (keyed off the editor's `TextEdit` id) rather than in `EditorState`,
/// since it's transient UI state, not part of the document being edited.
#[derive(Clone, Default)]
struct AutocompleteState {
    open: bool,
    /// The `query_start` (byte offset) of the wikilink this popup belongs to, used to
    /// tell "still the same link" from "cursor moved to a different `[[`".
    query_start: Option<usize>,
    /// The `query_start` of a wikilink the user dismissed with Escape, so it doesn't
    /// immediately reopen every frame while the cursor stays inside it.
    dismissed_at: Option<usize>,
    selected: usize,
}

enum PopupAction {
    Next,
    Prev,
    Confirm,
    Dismiss,
}

/// Stable id for the document `TextEdit`, independent of whatever panel happens to
/// host it this frame — lets `app.rs` move its cursor (e.g. jumping to a
/// find-and-replace result) without needing a `Ui` of its own to derive an id from.
pub fn editor_text_edit_id() -> Id {
    Id::new("smaragd_editor_text_edit")
}

/// Renders the document editor, including an Obsidian-style `[[wikilink]]`
/// autocomplete popup driven by `note_titles`. Returns `Some` if an autosave
/// triggered by focus loss failed, or the user pressed `activate_wikilink_shortcut`
/// (the remappable `ShortcutAction::ActivateWikilink`, `Ctrl+Enter`/`Cmd+Enter` by
/// default — `None` if the user unbound it) on a wikilink to follow it — the caller
/// decides what to do with either.
///
/// `focus_mode` enables Focus Mode's "typewriter" effect: the paragraph
/// containing the cursor renders at full strength, every other paragraph
/// dimmed — see `paragraph_byte_range`. `false` (normal dock-tab editing)
/// renders exactly as before, with no custom layouter at all.
///
/// `font`/`font_size` are `Settings::editor_font`/`editor_font_size` (already
/// resolved via `editor_font::resolve_size` — this takes a real point size, not
/// the raw possibly-`0.0` setting), shared with the Preview.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    note_titles: &[String],
    activate_wikilink_shortcut: Option<KeyboardShortcut>,
    focus_mode: bool,
    font: EditorFont,
    font_size: f32,
    collaborating: bool,
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
    let mut focus_mode_layouter =
        move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let text = buf.as_str();
            let font_id = font.font_id(font_size);
            let mut job = egui::text::LayoutJob {
                wrap: egui::text::TextWrapping {
                    max_width: wrap_width,
                    ..Default::default()
                },
                ..Default::default()
            };
            let normal = ui.visuals().text_color();
            match focus_mode_cursor_byte.map(|b| b.min(text.len())) {
                Some(cursor_byte) => {
                    let range = paragraph_byte_range(text, cursor_byte);
                    let dim = ui.visuals().weak_text_color();
                    if range.start > 0 {
                        job.append(
                            &text[..range.start],
                            0.0,
                            egui::TextFormat {
                                font_id: font_id.clone(),
                                color: dim,
                                ..Default::default()
                            },
                        );
                    }
                    job.append(
                        &text[range.start..range.end],
                        0.0,
                        egui::TextFormat {
                            font_id: font_id.clone(),
                            color: normal,
                            ..Default::default()
                        },
                    );
                    if range.end < text.len() {
                        job.append(
                            &text[range.end..],
                            0.0,
                            egui::TextFormat {
                                font_id,
                                color: dim,
                                ..Default::default()
                            },
                        );
                    }
                }
                None => job.append(
                    text,
                    0.0,
                    egui::TextFormat {
                        font_id,
                        color: normal,
                        ..Default::default()
                    },
                ),
            }
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

    let output = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // No border: `TextEdit`'s own frame only ever wraps its *content*
            // height (a few short paragraphs, say), never the full `ScrollArea`
            // around it — with the frame left on, a short document renders as a
            // small boxed page sitting in a lot of otherwise-dead-looking empty
            // space below it, rather than one editable area that fills the tab.
            // `lock_focus` (normally bundled into `.code_editor()`, which also
            // hardcodes the Monospace font — not wanted now that the font is
            // configurable) keeps Tab inserting a tab character instead of
            // leaving the field, still desirable for a plain-text editor.
            let mut text_edit = egui::TextEdit::multiline(&mut editor.buffer)
                .desired_width(f32::INFINITY)
                .desired_rows(desired_rows)
                .font(font.font_id(font_size))
                .lock_focus(true)
                .frame(egui::Frame::NONE)
                .id(text_edit_id);
            if focus_mode {
                text_edit = text_edit.layouter(&mut focus_mode_layouter);
            }
            text_edit.show(ui)
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
        active_wikilink_query(&editor.buffer, cursor_byte)
            .map(|query| (cursor_char, cursor_byte, query))
    });

    let mut completion: Option<(usize, usize, String)> = None; // (query_start, cursor_byte, chosen)

    let new_state = match active {
        None => AutocompleteState::default(),
        Some((_, _, query)) if state.dismissed_at == Some(query.query_start) => AutocompleteState {
            dismissed_at: state.dismissed_at,
            ..AutocompleteState::default()
        },
        Some((cursor_char, cursor_byte, query)) => {
            let all_candidates = filter_candidates(note_titles, &query.query);
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

    if let Some((query_start, cursor_byte, chosen)) = completion {
        let (new_text, new_cursor_byte) =
            apply_wikilink_completion(&editor.buffer, query_start, cursor_byte, &chosen);
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
    use super::{editor_text_edit_id, paragraph_byte_range, show};
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
                None,
                false,
                EditorFont::Monospace,
                14.0,
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
                None,
                false,
                EditorFont::Monospace,
                14.0,
                true,
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
                None,
                false,
                EditorFont::Monospace,
                14.0,
                false,
            );
        });

        assert!(
            ctx.read_response(editor_text_edit_id()).is_none(),
            "expected no TextEdit to render with nothing open and no collaboration session"
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
