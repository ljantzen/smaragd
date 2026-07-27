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
        .default_height(480.0)
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
                .max_height(320.0)
                .show(ui, |ui| {
                    // Grouped by functional category, alphabetically by label within
                    // each group — independent of `ShortcutAction::ALL`'s own
                    // (declaration-order) sequence, which has no bearing on how this
                    // list reads best to a user scanning for a specific action.
                    for category in ShortcutCategory::ALL {
                        let mut actions: Vec<ShortcutAction> = ShortcutAction::ALL
                            .iter()
                            .copied()
                            .filter(|action| action.category() == *category)
                            .collect();
                        actions.sort_by_key(|action| action.label());

                        ui.add_space(8.0);
                        ui.strong(category.label());
                        egui::Grid::new(format!("shortcuts_grid_{}", category.label()))
                            .num_columns(3)
                            .striped(true)
                            .show(ui, |ui| {
                                for action in &actions {
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
                    }
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
