use crate::settings::Settings;
use crate::shortcuts::{ShortcutAction, is_safe_binding};

/// Renders the settings window when `open` is true (closing it via the window's own
/// close button flips `open` back to `false`). `recording_shortcut` tracks which
/// action, if any, is currently capturing its next keypress — it must persist across
/// frames while the recording modal is open, so the caller owns it alongside
/// `settings`. Returns `true` if `settings` changed this frame, so the caller can
/// persist it to disk.
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    settings: &mut Settings,
    recording_shortcut: &mut Option<ShortcutAction>,
) -> bool {
    let mut changed = false;
    egui::Window::new("Settings")
        .open(open)
        .resizable(false)
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
                    "Create Research and Trash folders in new projects",
                )
                .changed();

            ui.separator();
            ui.heading("Keyboard Shortcuts");
            egui::Grid::new("shortcuts_grid")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    for action in ShortcutAction::ALL {
                        ui.label(action.label());
                        let text = settings
                            .shortcuts
                            .get(*action)
                            .map(|s| ctx.format_shortcut(&s))
                            .unwrap_or_else(|| "Unbound".to_string());
                        ui.label(text);
                        ui.horizontal(|ui| {
                            if ui.button("Change").clicked() {
                                *recording_shortcut = Some(*action);
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

    if let Some(action) = *recording_shortcut {
        changed |= show_recording_modal(ctx, settings, recording_shortcut, action);
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
    recording_shortcut: &mut Option<ShortcutAction>,
    action: ShortcutAction,
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
            settings.shortcuts.set(action, Some(shortcut));
            changed = true;
            *recording_shortcut = None;
        } else {
            rejected = true;
        }
    }

    egui::Modal::new(egui::Id::new("shortcut_recording_modal")).show(ctx, |ui| {
        ui.set_min_width(280.0);
        ui.heading(format!("Press a new shortcut for \"{}\"", action.label()));
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
