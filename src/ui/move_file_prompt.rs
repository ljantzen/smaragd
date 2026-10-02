//! An fzf-style folder picker for the Editor pane's ☰ menu "Move file to..."
//! item — deliberately not an OS-native folder dialog (that would let the user
//! navigate outside the project, which a move into has no meaning for, and
//! every other "pick a thing in this project" picker already works this way,
//! see `open_document_prompt.rs`). Fuzzy-matches subsequences against every
//! folder in the project (`crate::fuzzy`), same engine and interaction model
//! as `open_document_prompt`'s document picker.

use std::path::PathBuf;

use egui::{Id, Key, Modifiers};

use crate::fuzzy::fuzzy_match_documents;

const MAX_RESULTS: usize = 20;
const RESULTS_MAX_HEIGHT: f32 = 240.0;

/// UI state for one "Move file to..." dialog — created fresh (via `new`) each
/// time the menu item is clicked, since (unlike `OpenDocumentPromptState`,
/// reused across repeated Ctrl+P's) it needs to remember which file it's
/// moving for the duration of the dialog. Owned by `SmaragdApp` as
/// `Option<MoveFilePromptState>`: `None` means the dialog isn't open.
pub struct MoveFilePromptState {
    /// The file being moved — unrelated to whatever's selected in the results
    /// list below.
    pub moving: PathBuf,
    focus_requested: bool,
    query: String,
    /// Index into the current frame's filtered results, clamped to bounds
    /// each frame since the result list changes as the user types.
    selected: usize,
}

impl MoveFilePromptState {
    pub fn new(moving: PathBuf) -> Self {
        Self {
            moving,
            focus_requested: true,
            query: String::new(),
            selected: 0,
        }
    }
}

/// What the user decided — unlike `open_document_prompt::show`'s bare
/// `Option<PathBuf>`, a dedicated `Cancelled` variant is needed here because
/// the caller holds this dialog's very existence as `Option<MoveFilePromptState>`
/// (no separate `open` flag to clear): `show` must be able to say "done,
/// discard me" without that being confused with "no decision yet this frame".
pub enum MoveFilePromptOutcome {
    /// The absolute path of the chosen destination folder.
    Chosen(PathBuf),
    Cancelled,
}

enum NavAction {
    Next,
    Prev,
}

/// Consume arrow keys meant for the result list before the `TextEdit`
/// underneath sees them — same as `open_document_prompt.rs`'s `steal_nav_key`.
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

/// Renders the picker modal. Returns `Some` the frame the user confirms (Enter
/// or clicking a result) or cancels (Escape) — `None` while still open and
/// awaiting input. `candidates` is `(display_path, absolute_path)` pairs (see
/// `Project::folder_candidates`), recomputed by the caller only while the
/// dialog is open.
pub fn show(
    ctx: &egui::Context,
    state: &mut MoveFilePromptState,
    candidates: &[(String, PathBuf)],
) -> Option<MoveFilePromptOutcome> {
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
    let just_navigated = nav_action.is_some();

    let mut outcome = None;
    egui::Modal::new(Id::new("move_file_prompt_modal")).show(ctx, |ui| {
        ui.set_min_width(420.0);
        ui.heading("Move File To…");
        ui.add_space(4.0);
        ui.weak(state.moving.display().to_string());
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
            outcome = Some(MoveFilePromptOutcome::Chosen(path.clone()));
        }

        ui.add_space(8.0);
        if matches.is_empty() {
            ui.weak("No matching folders.");
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
                        outcome = Some(MoveFilePromptOutcome::Chosen(path.clone()));
                    }
                }
            });

        if ui.input(|i| i.key_pressed(Key::Escape)) {
            outcome = Some(MoveFilePromptOutcome::Cancelled);
        }
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives `show` with synthetic input across frames — mirrors
    /// `open_document_prompt.rs`'s own test harness for the same reason.
    #[derive(Default)]
    struct Harness {
        ctx: egui::Context,
    }

    impl Harness {
        fn frame(
            &self,
            state: &mut MoveFilePromptState,
            candidates: &[(String, PathBuf)],
            events: Vec<egui::Event>,
        ) -> Option<MoveFilePromptOutcome> {
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

        fn idle(&self, state: &mut MoveFilePromptState, candidates: &[(String, PathBuf)]) {
            self.frame(state, candidates, vec![]);
        }

        fn press_enter(
            &self,
            state: &mut MoveFilePromptState,
            candidates: &[(String, PathBuf)],
        ) -> Option<MoveFilePromptOutcome> {
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

        fn press_escape(
            &self,
            state: &mut MoveFilePromptState,
            candidates: &[(String, PathBuf)],
        ) -> Option<MoveFilePromptOutcome> {
            self.frame(
                state,
                candidates,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            )
        }
    }

    #[test]
    fn enter_chooses_the_first_result_once_focus_has_settled() {
        let harness = Harness::default();
        let mut state = MoveFilePromptState::new(PathBuf::from("/project/scene.md"));
        let candidates = vec![
            ("Chapters".to_string(), PathBuf::from("/project/Chapters")),
            ("Research".to_string(), PathBuf::from("/project/Research")),
        ];

        // First frame grants focus via `request_focus`; a second (idle) frame
        // lets that settle before a keypress, same as `open_document_prompt.rs`.
        harness.idle(&mut state, &candidates);
        harness.idle(&mut state, &candidates);
        let outcome = harness.press_enter(&mut state, &candidates);

        assert!(matches!(
            outcome,
            Some(MoveFilePromptOutcome::Chosen(path))
                if path == std::path::Path::new("/project/Chapters")
        ));
    }

    #[test]
    fn escape_cancels() {
        let harness = Harness::default();
        let mut state = MoveFilePromptState::new(PathBuf::from("/project/scene.md"));
        let candidates = vec![("Chapters".to_string(), PathBuf::from("/project/Chapters"))];

        let outcome = harness.press_escape(&mut state, &candidates);

        assert!(matches!(outcome, Some(MoveFilePromptOutcome::Cancelled)));
    }
}
