use super::*;

use crate::double_tap::{DoubleTapDetector, tap_input};
use crate::ui::search_everywhere::{ActionCandidate, SearchEverywhereOutcome, TextDocument};

impl SmaragdApp {
    /// Every document in the open project as `(display_path, absolute_path)` —
    /// the candidate list shared by the Open Document switcher and Search
    /// Everywhere. Empty if no project is open.
    pub(super) fn document_candidates(&self) -> Vec<(String, PathBuf)> {
        let Some(project) = &self.project else {
            return Vec::new();
        };
        project
            .tree
            .document_paths()
            .into_iter()
            .map(|path| {
                let relative = path.strip_prefix(&project.root).unwrap_or(&path);
                let display =
                    crate::project::model::document_label(&relative.to_string_lossy()).to_string();
                (display, path)
            })
            .collect()
    }

    /// Every action Search Everywhere can run right now: the built-in
    /// `ShortcutAction`s that make sense outside the editor, then plugin `:`
    /// commands.
    fn search_everywhere_actions(&self, ctx: &egui::Context) -> Vec<ActionCandidate> {
        let git_enabled = self.settings.git_integration_enabled();
        let built_in = ShortcutAction::ALL.iter().copied().filter(|action| {
            !matches!(
                action,
                // Both need the editor's live `TextEdit` cursor (see their doc
                // comments), which a click in this modal doesn't have.
                ShortcutAction::ActivateWikilink
                    | ShortcutAction::ToggleBookmark
                    | ShortcutAction::SearchEverywhere
            ) && (git_enabled || action.category() != crate::shortcuts::ShortcutCategory::Git)
                && (*action != ShortcutAction::SyncNow || self.sync_is_running())
        });
        let mut actions: Vec<ActionCandidate> = built_in
            .map(|action| ActionCandidate {
                label: action.label().to_string(),
                shortcut: self
                    .settings
                    .shortcuts
                    .get(action)
                    .map(|shortcut| ctx.format_shortcut(&shortcut)),
                target: ShortcutTarget::BuiltIn(action),
            })
            .collect();
        actions.extend(self.plugin_engine.command_names().map(|name| {
            let shortcut = self
                .plugin_shortcuts
                .iter()
                .find(|(plugin, _)| plugin == name)
                .map(|(_, shortcut)| ctx.format_shortcut(shortcut));
            ActionCandidate {
                label: format!(":{name}"),
                shortcut,
                target: ShortcutTarget::Plugin(name.to_string()),
            }
        }));
        actions
    }

    /// Read every document's text once for Search Everywhere's text search —
    /// the open document from its live (possibly unsaved) buffer instead of
    /// disk, same as Find and Replace does.
    fn search_text_cache(&self, documents: &[(String, PathBuf)]) -> Vec<TextDocument> {
        let Some(project) = &self.project else {
            return Vec::new();
        };
        documents
            .iter()
            .filter_map(|(display, path)| {
                let content = if self.editor.open_path.as_deref() == Some(path.as_path()) {
                    self.editor.buffer.clone()
                } else {
                    project.store.read_to_string(path).ok()?
                };
                Some(TextDocument {
                    display: display.clone(),
                    path: path.clone(),
                    content,
                })
            })
            .collect()
    }

    /// Renders Search Everywhere if it's open, and carries out whatever the
    /// user picks.
    pub(super) fn show_search_everywhere(&mut self, ui: &mut egui::Ui) {
        if !self.search_everywhere.open {
            return;
        }
        let ctx = ui.ctx().clone();
        // Only walk the document tree while the modal is actually visible, same
        // as the Open Document switcher.
        let documents = self.document_candidates();
        if self.search_everywhere.text_cache.is_none() {
            self.search_everywhere.text_cache = Some(self.search_text_cache(&documents));
        }
        let actions = self.search_everywhere_actions(&ctx);
        let Some(outcome) =
            ui::search_everywhere::show(&ctx, &mut self.search_everywhere, &documents, &actions)
        else {
            return;
        };
        match outcome {
            SearchEverywhereOutcome::OpenDocument(path) => self.open_document(&path),
            SearchEverywhereOutcome::OpenAt(path, byte) => {
                self.open_document_at(&ctx, &path, byte);
            }
            SearchEverywhereOutcome::Run(ShortcutTarget::BuiltIn(action)) => {
                self.dispatch_shortcut_action(&ctx, action);
            }
            SearchEverywhereOutcome::Run(ShortcutTarget::Plugin(name)) => {
                self.run_plugin_command(&name, "");
            }
            SearchEverywhereOutcome::OpenSettings(category) => {
                self.settings_category = category;
                self.show_settings = true;
            }
        }
    }

    /// Open Search Everywhere on a double tap of Shift, unless the gesture is
    /// turned off, a shortcut is being recorded, or some other modal is already
    /// up (egui reports last frame's top modal, which is what the user saw when
    /// they tapped). The detector sees every frame's events regardless, so its
    /// state stays accurate across a modal closing.
    pub(super) fn detect_double_shift(&mut self, ctx: &egui::Context) {
        let detector: &mut DoubleTapDetector = &mut self.double_shift;
        let triggered = ctx.input(|i| {
            // Every event must reach the detector, so no short-circuiting `any`.
            let mut triggered = false;
            for input in i.events.iter().filter_map(tap_input) {
                triggered |= detector.feed(i.time, input);
            }
            triggered
        });
        if triggered
            && self.settings.double_shift_search_enabled()
            && self.recording_shortcut.is_none()
            && ctx.memory(|m| m.top_modal_layer()).is_none()
        {
            self.search_everywhere.request_open();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shift(down: bool) -> egui::Event {
        egui::Event::ModifiersChanged(egui::Modifiers {
            shift: down,
            ..Default::default()
        })
    }

    /// What egui-winit actually sends for one Shift press or release: the
    /// modifier change *and* a `Key::ShiftLeft` event.
    fn shift_events(down: bool) -> Vec<egui::Event> {
        vec![
            shift(down),
            egui::Event::Key {
                key: egui::Key::ShiftLeft,
                physical_key: Some(egui::Key::ShiftLeft),
                pressed: down,
                repeat: false,
                modifiers: egui::Modifiers {
                    shift: down,
                    ..Default::default()
                },
            },
        ]
    }

    fn frame(app: &mut SmaragdApp, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            time: Some(time),
            events,
            ..Default::default()
        };
        crate::egui_test_support::run_ui_and_discard(ctx, input, |ui| {
            app.detect_double_shift(ui.ctx());
        });
    }

    #[test]
    fn double_shift_opens_search_everywhere() {
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, shift_events(true));
        frame(&mut app, &ctx, 0.05, shift_events(false));
        frame(&mut app, &ctx, 0.1, shift_events(true));
        assert!(!app.search_everywhere.open);
        frame(&mut app, &ctx, 0.15, shift_events(false));
        assert!(app.search_everywhere.open);
    }
}
