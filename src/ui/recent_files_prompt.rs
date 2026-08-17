//! An IntelliJ-style "Recent Files" switcher — `ShortcutAction::RecentFiles` (Ctrl+Shift+O
//! by default). Two modes: activating the shortcut while the popup is closed shows
//! recently *edited* documents; activating it again while the popup is already open
//! toggles to recently *opened* documents (and back again on further activations) — see
//! [`RecentFilesMode`]. Otherwise mirrors `open_document_prompt.rs`'s fuzzy quick-switcher
//! almost exactly, just fed from a recency-based candidate list instead of the whole
//! project tree.

use std::path::PathBuf;

use egui::{Id, Key, Modifiers};

use crate::fuzzy::fuzzy_match_documents;

/// Cap on how many results are computed/shown at all — mirrors
/// `open_document_prompt::MAX_RESULTS`.
const MAX_RESULTS: usize = 20;

/// Max height of the scrollable results list, in points.
const RESULTS_MAX_HEIGHT: f32 = 240.0;

/// Which candidate list the popup is currently showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecentFilesMode {
    #[default]
    Edited,
    Opened,
}

impl RecentFilesMode {
    fn toggled(self) -> Self {
        match self {
            Self::Edited => Self::Opened,
            Self::Opened => Self::Edited,
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::Edited => "Recently Edited",
            Self::Opened => "Recently Opened",
        }
    }
}

/// UI state, owned by `app.rs` for the app's lifetime.
#[derive(Default)]
pub struct RecentFilesPromptState {
    pub open: bool,
    /// Set alongside `open` so `show` focuses the input once rather than fighting the
    /// user for focus on every frame the modal is visible.
    pub focus_requested: bool,
    pub query: String,
    /// Index into the current frame's filtered results, clamped to bounds each frame
    /// since the result list changes as the user types.
    pub selected: usize,
    pub mode: RecentFilesMode,
}

impl RecentFilesPromptState {
    /// Called every time `ShortcutAction::RecentFiles` fires. If the popup is
    /// already open, this is a repeat activation while it's still on screen — toggle
    /// the mode instead of just resetting, so "press again immediately" cycles
    /// between recently edited and recently opened. If it's closed, open it fresh in
    /// the default (Edited) mode.
    pub fn request_open(&mut self) {
        if self.open {
            self.mode = self.mode.toggled();
        } else {
            self.open = true;
            self.mode = RecentFilesMode::default();
        }
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
/// underneath never sees them — matches `open_document_prompt.rs`'s `steal_nav_key`.
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

/// Renders the Recent Files modal. Returns `Some(path)` the frame the user opens a
/// document, via Enter or clicking a result. `candidates` is `(display_path,
/// absolute_path)` pairs for whichever mode `state.mode` currently selects — recomputed
/// by the caller only while `state.open`, mirroring `open_document_prompt`'s own
/// convention.
pub fn show(
    ctx: &egui::Context,
    state: &mut RecentFilesPromptState,
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
    egui::Modal::new(Id::new("recent_files_prompt_modal")).show(ctx, |ui| {
        ui.set_min_width(420.0);
        ui.heading(state.mode.heading());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives `show` with synthetic input across frames — mirrors
    /// `open_document_prompt.rs`'s own test harness.
    #[derive(Default)]
    struct Harness {
        ctx: egui::Context,
    }

    impl Harness {
        fn frame(
            &self,
            state: &mut RecentFilesPromptState,
            candidates: &[(String, PathBuf)],
            events: Vec<egui::Event>,
        ) -> Option<PathBuf> {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut outcome = None;
            crate::egui_test_support::run_ui_and_discard(&self.ctx, input, |ui| {
                outcome = show(ui.ctx(), state, candidates);
            });
            outcome
        }

        fn idle(&self, state: &mut RecentFilesPromptState, candidates: &[(String, PathBuf)]) {
            self.frame(state, candidates, vec![]);
        }

        fn press_enter(
            &self,
            state: &mut RecentFilesPromptState,
            candidates: &[(String, PathBuf)],
        ) -> Option<PathBuf> {
            self.frame(
                state,
                candidates,
                vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            )
        }
    }

    #[test]
    fn enter_opens_the_first_result_once_focus_has_settled() {
        let harness = Harness::default();
        let mut state = RecentFilesPromptState::default();
        state.request_open();
        let candidates = vec![
            ("Alpha".to_string(), PathBuf::from("/tmp/alpha.md")),
            ("Beta".to_string(), PathBuf::from("/tmp/beta.md")),
        ];

        // First frame grants focus via `request_focus`; a second (idle) frame
        // lets that settle before a keypress, same as `open_document_prompt.rs`'s
        // test.
        harness.idle(&mut state, &candidates);
        harness.idle(&mut state, &candidates);
        let outcome = harness.press_enter(&mut state, &candidates);

        assert_eq!(outcome, Some(PathBuf::from("/tmp/alpha.md")));
    }

    #[test]
    fn request_open_defaults_to_edited_mode() {
        let mut state = RecentFilesPromptState::default();
        state.request_open();
        assert_eq!(state.mode, RecentFilesMode::Edited);
    }

    #[test]
    fn requesting_open_again_while_already_open_toggles_the_mode() {
        let mut state = RecentFilesPromptState::default();
        state.request_open();
        assert_eq!(state.mode, RecentFilesMode::Edited);

        state.request_open();
        assert_eq!(state.mode, RecentFilesMode::Opened);

        state.request_open();
        assert_eq!(state.mode, RecentFilesMode::Edited);
    }

    #[test]
    fn requesting_open_after_closing_resets_to_edited_mode() {
        let mut state = RecentFilesPromptState::default();
        state.request_open();
        state.request_open(); // toggles to Opened
        state.open = false; // simulate the popup having been closed

        state.request_open();

        assert_eq!(state.mode, RecentFilesMode::Edited);
    }
}
