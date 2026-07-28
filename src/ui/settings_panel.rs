use crate::settings::Settings;
use crate::shortcuts::{ShortcutAction, ShortcutCategory, ShortcutTarget, is_safe_binding};

/// Renders the settings window when `open` is true (closing it via the window's own
/// close button flips `open` back to `false`). `recording_shortcut` tracks which
/// binding, if any, is currently capturing its next keypress (built-in or plugin —
/// see `ShortcutTarget`) — it must persist across frames while the recording modal
/// is open, so the caller owns it alongside `settings`. `plugin_shortcut_rows` is
/// every plugin `:` command that declared a shortcut (`register_shortcut`) paired
/// with its current effective binding, if any (`app.rs`'s
/// `compute_effective_plugin_shortcuts`) — `Settings` alone doesn't know which
/// plugins are loaded. Returns `true` if `settings` changed this frame, so the
/// caller can persist it to disk (and recompute the effective plugin shortcuts,
/// since an edit here can change which ones are free).
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    settings: &mut Settings,
    recording_shortcut: &mut Option<ShortcutTarget>,
    plugin_shortcut_rows: &[(String, Option<egui::KeyboardShortcut>)],
) -> bool {
    let mut changed = false;
    egui::Window::new("Settings")
        .open(open)
        .resizable(true)
        .default_width(560.0)
        .default_height(680.0)
        .collapsible(false)
        .show(ctx, |ui| {
            changed |= ui
                .checkbox(
                    &mut settings.reopen_last_project,
                    "Reopen project on launch",
                )
                .changed();
            changed |= ui
                .checkbox(
                    &mut settings.create_starter_folders,
                    "Ensure Research and Trash folders exist in every project",
                )
                .changed();

            ui.separator();
            ui.heading("Theme");
            let previous_theme = settings.theme_preference;
            settings.theme_preference.radio_buttons(ui);
            if settings.theme_preference != previous_theme {
                ctx.set_theme(settings.theme_preference);
                changed = true;
            }

            ui.separator();
            ui.heading("Keyboard Shortcuts");
            // A scroll area of its own, not the whole window: with `ShortcutAction::ALL`
            // now well past a dozen entries, letting it grow the window unbounded made
            // Settings unmanageable, and scrolling the whole window would also push the
            // checkboxes/Theme section above out of view along with it.
            egui::ScrollArea::vertical()
                .max_height(440.0)
                .show(ui, |ui| {
                    // Sorted by functional category (`ShortcutCategory::ALL`'s order),
                    // then alphabetically by label within each — shown as a "Category"
                    // column on every row rather than a heading per group, so the whole
                    // list stays one scannable grid instead of several disjoint ones.
                    let mut actions: Vec<ShortcutAction> = ShortcutAction::ALL.to_vec();
                    actions.sort_by_key(|action| {
                        let category_index = ShortcutCategory::ALL
                            .iter()
                            .position(|category| *category == action.category())
                            .unwrap_or(usize::MAX);
                        (category_index, action.label())
                    });

                    egui::Grid::new("shortcuts_grid")
                        .num_columns(4)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Category");
                            ui.strong("Action");
                            ui.strong("Shortcut");
                            ui.end_row();

                            for action in &actions {
                                ui.label(action.category().label());
                                ui.label(action.label());
                                let text = settings
                                    .shortcuts
                                    .get(*action)
                                    .map(|s| ctx.format_shortcut(&s))
                                    .unwrap_or_else(|| "Unbound".to_string());
                                ui.label(text);
                                ui.horizontal(|ui| {
                                    if ui.button("Change").clicked() {
                                        *recording_shortcut =
                                            Some(ShortcutTarget::BuiltIn(*action));
                                    }
                                    if ui.button("Clear").clicked() {
                                        settings.shortcuts.set(*action, None);
                                        changed = true;
                                    }
                                });
                                ui.end_row();
                            }
                        });
                });

            if !plugin_shortcut_rows.is_empty() {
                ui.separator();
                ui.heading("Plugin Shortcuts");
                egui::Grid::new("plugin_shortcuts_grid")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        for (name, current) in plugin_shortcut_rows {
                            ui.label(format!(":{name}"));
                            let text = current
                                .map(|s| ctx.format_shortcut(&s))
                                .unwrap_or_else(|| "Unbound".to_string());
                            ui.label(text);
                            ui.horizontal(|ui| {
                                if ui.button("Change").clicked() {
                                    *recording_shortcut =
                                        Some(ShortcutTarget::Plugin(name.clone()));
                                }
                                if ui.button("Clear").clicked() {
                                    settings.set_plugin_shortcut(name, None, plugin_shortcut_rows);
                                    changed = true;
                                }
                            });
                            ui.end_row();
                        }
                    });
            }
        });

    // Gated on no recording in progress: `show_recording_modal` below also reads
    // Escape (to cancel just the recording, not the whole window) — checking here
    // unconditionally would close Settings out from under it on the same keypress.
    if *open && recording_shortcut.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *open = false;
    }

    if let Some(target) = recording_shortcut.clone() {
        changed |= show_recording_modal(
            ctx,
            settings,
            recording_shortcut,
            target,
            plugin_shortcut_rows,
        );
    }

    changed
}

/// Modal capturing the next keypress as `action`'s new shortcut. Runs as an
/// `egui::Modal` (matching `name_prompt.rs`'s pattern) to block clicks elsewhere
/// while recording. The captured key event is also explicitly removed from the
/// input queue (rather than just peeked at) so it can't *also* reach whatever
/// widget happens to have focus in the background this same frame — e.g. typing
/// into an open document while trying to record a shortcut.
fn show_recording_modal(
    ctx: &egui::Context,
    settings: &mut Settings,
    recording_shortcut: &mut Option<ShortcutTarget>,
    target: ShortcutTarget,
    plugin_shortcut_rows: &[(String, Option<egui::KeyboardShortcut>)],
) -> bool {
    let mut changed = false;
    let mut cancelled = false;
    let mut captured = None;
    let mut rejected = false;

    ctx.input_mut(|input| {
        let mut handled = false;
        input.events.retain(|event| {
            if handled {
                return true;
            }
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            // A bare modifier press (e.g. just Shift, held down before the real key
            // arrives) shows up as its own `Event::Key` with `key` set to one of
            // egui's physical `ShiftLeft`/`ControlRight`/etc. variants — never a
            // valid shortcut on its own. Drop it without setting `handled`, so
            // scanning continues for the actual key the user is holding it to
            // combine with, rather than capturing e.g. "Ctrl+ShiftLeft" as the
            // whole shortcut the instant Shift goes down.
            if is_bare_modifier_key(*key) {
                return false;
            }
            handled = true;
            if *key == egui::Key::Escape && modifiers.is_none() {
                cancelled = true;
            } else {
                captured = Some(egui::KeyboardShortcut::new(*modifiers, *key));
            }
            false
        });
    });

    if cancelled {
        *recording_shortcut = None;
    } else if let Some(shortcut) = captured {
        if is_safe_binding(&shortcut) {
            match &target {
                ShortcutTarget::BuiltIn(action) => settings.shortcuts.set(*action, Some(shortcut)),
                ShortcutTarget::Plugin(name) => {
                    settings.set_plugin_shortcut(name, Some(shortcut), plugin_shortcut_rows)
                }
            }
            changed = true;
            *recording_shortcut = None;
        } else {
            rejected = true;
        }
    }

    let label = match &target {
        ShortcutTarget::BuiltIn(action) => action.label().to_string(),
        ShortcutTarget::Plugin(name) => format!(":{name}"),
    };
    egui::Modal::new(egui::Id::new("shortcut_recording_modal")).show(ctx, |ui| {
        ui.set_min_width(280.0);
        ui.heading(format!("Press a new shortcut for \"{label}\""));
        ui.label("Press Escape to cancel.");
        if rejected {
            ui.colored_label(
                egui::Color32::from_rgb(200, 60, 60),
                "Shortcuts need Ctrl, Alt, or Shift (function keys and Escape are exempt).",
            );
        }
    });

    changed
}

/// True for the physical "which side" modifier-key variants (`ShiftLeft`,
/// `ControlRight`, etc.) `egui::Key` reports pressing a modifier on its own as —
/// never a valid shortcut by itself, so the recording modal above must skip past
/// these rather than capturing one as the whole shortcut the moment a user starts
/// holding Ctrl or Shift, before the actual key.
fn is_bare_modifier_key(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ShiftLeft
            | egui::Key::ShiftRight
            | egui::Key::ControlLeft
            | egui::Key::ControlRight
            | egui::Key::AltLeft
            | egui::Key::AltRight
            | egui::Key::SuperLeft
            | egui::Key::SuperRight
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escape_event() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn frame(
        ctx: &egui::Context,
        open: &mut bool,
        settings: &mut Settings,
        recording_shortcut: &mut Option<ShortcutTarget>,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            show(ui.ctx(), open, settings, recording_shortcut, &[]);
        });
    }

    #[test]
    fn escape_closes_the_window_when_nothing_is_being_recorded() {
        let ctx = egui::Context::default();
        let mut settings = Settings::default();
        let mut open = true;
        let mut recording_shortcut = None;

        frame(
            &ctx,
            &mut open,
            &mut settings,
            &mut recording_shortcut,
            vec![],
        );
        assert!(open, "window should still be open before Escape");

        frame(
            &ctx,
            &mut open,
            &mut settings,
            &mut recording_shortcut,
            vec![escape_event()],
        );
        assert!(!open, "Escape should close the Settings window");
    }

    #[test]
    fn escape_cancels_recording_instead_of_closing_the_window() {
        let ctx = egui::Context::default();
        let mut settings = Settings::default();
        let mut open = true;
        let mut recording_shortcut = Some(ShortcutTarget::BuiltIn(ShortcutAction::Save));

        frame(
            &ctx,
            &mut open,
            &mut settings,
            &mut recording_shortcut,
            vec![escape_event()],
        );
        assert!(
            open,
            "Escape while recording a shortcut should cancel the recording, not close Settings"
        );
        assert!(recording_shortcut.is_none());
    }
}
