/// Shown when the currently open document changed on disk (another program, a
/// sync tool, a `git pull`) while it also had unsaved local edits — lets the
/// user pick which version wins instead of one silently clobbering the other.
/// See `SmaragdApp::resolve_external_conflict`.
pub enum ExternalConflictOutcome {
    /// Keep the in-editor buffer as-is; the on-disk change is acknowledged
    /// (not re-prompted for again) but otherwise ignored.
    KeepMine,
    /// Discard the unsaved local edits and load the on-disk version instead.
    ReloadFromDisk,
}

/// Renders the confirmation modal for `path`. Returns `Some` the frame the user
/// picks an option (or presses Escape, treated as `KeepMine` — the safer
/// default, since it never discards anything); `None` while still open and
/// awaiting input.
pub fn show(ctx: &egui::Context, path: &std::path::Path) -> Option<ExternalConflictOutcome> {
    let mut outcome = None;
    egui::Modal::new(egui::Id::new("external_conflict_modal")).show(ctx, |ui| {
        ui.set_min_width(320.0);
        ui.heading("Changed on Disk");
        ui.add_space(8.0);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("This document");
        ui.label(format!(
            "\"{name}\" was changed outside Smaragd, and you have unsaved edits here. \
             Which version do you want to keep?"
        ));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Keep Mine").clicked() {
                outcome = Some(ExternalConflictOutcome::KeepMine);
            }
            if ui.button("Reload from Disk").clicked() {
                outcome = Some(ExternalConflictOutcome::ReloadFromDisk);
            }
        });

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            outcome = Some(ExternalConflictOutcome::KeepMine);
        }
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Harness {
        ctx: egui::Context,
    }

    impl Harness {
        fn frame(&self, path: &std::path::Path) -> Option<ExternalConflictOutcome> {
            let mut outcome = None;
            crate::egui_test_support::run_ui_and_discard(
                &self.ctx,
                egui::RawInput::default(),
                |ui| {
                    outcome = show(ui.ctx(), path);
                },
            );
            outcome
        }
    }

    #[test]
    fn no_outcome_until_a_button_is_clicked() {
        let harness = Harness::default();
        assert!(
            harness
                .frame(std::path::Path::new("/project/scene.md"))
                .is_none()
        );
    }
}
