//! An fzf-style quick-switcher for opening a document by name — File > Open
//! Document / `ShortcutAction::OpenDocument` (Ctrl+P by default). Unlike the `:`
//! command prompt's `:open <title>` (plain prefix/substring matching via
//! `autocomplete::filter_candidates`, and a two-step "accept completion, then press
//! Enter again" flow), this fuzzy-matches subsequences (`crate::fuzzy`) and opens the
//! highlighted result directly on Enter or click.

use std::path::PathBuf;

use egui::{Id, Key, Modifiers};

use crate::fuzzy::fuzzy_match_documents;

/// Cap on how many results are computed/shown at all — mostly a safety valve against
/// scoring every document in a huge project on every keystroke, not a visible-rows
/// limit (the list scrolls, see `show`).
const MAX_RESULTS: usize = 20;

/// Max height of the scrollable results list, in points — tall enough to show a
/// handful of rows without the modal growing to fill the screen.
const RESULTS_MAX_HEIGHT: f32 = 240.0;

/// UI state, owned by `app.rs` for the app's lifetime.
#[derive(Default)]
pub struct OpenDocumentPromptState {
    pub open: bool,
    /// Set alongside `open` so `show` focuses the input once rather than fighting the
    /// user for focus on every frame the modal is visible.
    pub focus_requested: bool,
    pub query: String,
    /// Index into the current frame's filtered results, clamped to bounds each frame
    /// since the result list changes as the user types.
    pub selected: usize,
}

impl OpenDocumentPromptState {
    pub fn request_open(&mut self) {
        self.open = true;
        self.focus_requested = true;
        self.query.clear();
        self.selected = 0;
    }
}

enum NavAction {
    Next,
    Prev,
}

/// Consume (and act on) arrow keys meant for the result list, so the `TextEdit`
/// underneath never sees them — matches `command_prompt.rs`'s `steal_popup_key`,
/// minus the Tab-to-accept-completion action, since there's no completion step here.
fn steal_nav_key(ctx: &egui::Context) -> Option<NavAction> {
    ctx.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            Some(NavAction::Next)
        } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            Some(NavAction::Prev)
        } else {
            None
        }
    })
}

/// Renders the quick-switcher modal. Returns `Some(path)` the frame the user opens a
/// document, via Enter or clicking a result. `candidates` is `(display_path,
/// absolute_path)` pairs — recomputed by the caller only while `state.open`, mirroring
/// the "only walk the document tree while the popup is actually visible" convention
/// already used for `:open`'s completion and the command prompt in general.
pub fn show(
    ctx: &egui::Context,
    state: &mut OpenDocumentPromptState,
    candidates: &[(String, PathBuf)],
) -> Option<PathBuf> {
    if !state.open {
        return None;
    }

    let matches = fuzzy_match_documents(candidates, &state.query, MAX_RESULTS);
    if !matches.is_empty() {
        state.selected = state.selected.min(matches.len() - 1);
    }
    let nav_action = (!matches.is_empty()).then(|| steal_nav_key(ctx)).flatten();
    match nav_action {
        Some(NavAction::Next) => state.selected = (state.selected + 1) % matches.len(),
        Some(NavAction::Prev) => {
            state.selected = (state.selected + matches.len() - 1) % matches.len();
        }
        None => {}
    }
    // Only scroll the selection into view when it moved via the keyboard — scrolling
    // on every frame regardless would fight the user if they're scrolling with the
    // mouse instead.
    let just_navigated = nav_action.is_some();

    let mut outcome = None;
    let mut close = false;
    egui::Modal::new(Id::new("open_document_prompt_modal")).show(ctx, |ui| {
        ui.set_min_width(420.0);
        ui.heading("Open Document");
        ui.add_space(8.0);

        let response = ui.text_edit_singleline(&mut state.query);
        if state.focus_requested {
            response.request_focus();
            state.focus_requested = false;
        }
        if response.lost_focus()
            && ui.input(|i| i.key_pressed(Key::Enter))
            && let Some((_, path)) = matches.get(state.selected)
        {
            outcome = Some(path.clone());
        }

        ui.add_space(8.0);
        if matches.is_empty() {
            ui.weak("No matching documents.");
        }
        egui::ScrollArea::vertical()
            .max_height(RESULTS_MAX_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (index, (name, path)) in matches.iter().enumerate() {
                    let selected = index == state.selected;
                    let response = ui.selectable_label(selected, name);
                    if selected && just_navigated {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    if response.clicked() {
                        outcome = Some(path.clone());
                    }
                }
            });

        if ui.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
    });

    if outcome.is_some() || close {
        state.open = false;
    }
    outcome
}
