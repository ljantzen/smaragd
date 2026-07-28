use egui::text::{CCursor, CCursorRange};
use egui::widgets::text_edit::TextEditOutput;
use egui::{Id, Key, KeyboardShortcut, Modifiers};

use crate::autocomplete::{
    active_wikilink_query, apply_wikilink_completion, byte_offset_to_char, char_offset_to_byte,
    filter_candidates,
};
use crate::editor::EditorState;
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
    Id::new("tachylite_editor_text_edit")
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
pub fn show(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    note_titles: &[String],
    activate_wikilink_shortcut: Option<KeyboardShortcut>,
    focus_mode: bool,
) -> Option<EditorEvent> {
    if editor.open_path.is_none() {
        ui.label("Select a file from the binder to start editing.");
        return None;
    }

    let text_edit_id = editor_text_edit_id();
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
            let font_id = egui::TextStyle::Monospace.resolve(ui.style());
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

    // Wrapped in a `ScrollArea`, filling the full available space
    // (`auto_shrink([false, false])`): `TextEdit`'s own size is purely
    // content-driven (it sizes to however many rows/columns of text it
    // actually holds, floored at `desired_rows`'s default of 4 — `min_size`'s
    // height component, despite its name, is silently ignored by egui, only
    // ever folding into desired *width*), so without a `ScrollArea` a short
    // document leaves the rest of the container looking broken/empty (exactly
    // what made Focus Mode's fullscreen so glaring — the previous unscrolled
    // `TextEdit` alone just rendered a small box) and a long one has no way to
    // scroll to see past whatever first fit in the visible area at all.
    let output = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut text_edit = egui::TextEdit::multiline(&mut editor.buffer)
                .desired_width(f32::INFINITY)
                .code_editor()
                .id(text_edit_id);
            if focus_mode {
                // No border: `TextEdit`'s own frame only ever wraps its
                // *content* height (a few short paragraphs, say), never the
                // full `ScrollArea` around it — with the frame left on, a
                // short document still looks like a small boxed page sitting
                // in a lot of empty space, which is exactly what wasn't
                // supposed to happen. Real distraction-free views (Scrivener's
                // Composition Mode included) don't box the text at all; it
                // just sits on the background.
                text_edit = text_edit
                    .frame(egui::Frame::NONE)
                    .layouter(&mut focus_mode_layouter);
            }
            text_edit.show(ui)
        })
        .inner;

    if output.response.changed() {
        editor.mark_dirty();
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
    use super::paragraph_byte_range;

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
