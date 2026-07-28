//! Help > About: app name, version, and build info (git commit, build date) — set
//! by `build.rs` at compile time — mostly to make "which exact build is this?"
//! answerable when debugging, since this app has no auto-update/release channel of
//! its own.

/// Renders the About modal. Returns `true` the frame the user dismisses it —
/// there's only one action here (Close), so unlike every other modal, Enter and
/// Escape both resolve to the same outcome rather than confirm/cancel.
pub fn show(ctx: &egui::Context) -> bool {
    let mut close = false;
    egui::Modal::new(egui::Id::new("about_modal")).show(ctx, |ui| {
        ui.set_min_width(280.0);
        ui.heading("Tachylite");
        ui.add_space(8.0);

        egui::Grid::new("about_grid").num_columns(2).show(ui, |ui| {
            ui.label("Version:");
            ui.label(env!("CARGO_PKG_VERSION"));
            ui.end_row();

            ui.label("Commit:");
            ui.label(concat!(
                env!("TACHYLITE_GIT_HASH"),
                env!("TACHYLITE_GIT_DIRTY")
            ));
            ui.end_row();

            ui.label("Built:");
            ui.label(env!("TACHYLITE_BUILD_DATE"));
            ui.end_row();
        });

        ui.add_space(8.0);
        if ui.button("Close").clicked() {
            close = true;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
    });
    close
}
