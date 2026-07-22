use crate::settings::Settings;

/// Renders the settings window when `open` is true (closing it via the window's own
/// close button flips `open` back to `false`). Returns `true` if `settings` changed
/// this frame, so the caller can persist it to disk.
pub fn show(ctx: &egui::Context, open: &mut bool, settings: &mut Settings) -> bool {
    let mut changed = false;
    egui::Window::new("Settings")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            changed = ui
                .checkbox(
                    &mut settings.reopen_last_project,
                    "Reopen project on launch",
                )
                .changed();
        });
    changed
}
