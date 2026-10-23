/// Shown once, right after a project finishes opening, when
/// `project::sanity_check::check` found anything — see that module's doc
/// comment for exactly what counts and why. Purely informational: the only
/// action here is dismissing it, since nothing it reports is safe to
/// auto-repair (same reasoning as that module's own doc comment).
///
/// Returns `true` the frame the user dismisses it (OK or Escape), for the
/// caller to clear `SmaragdApp::sanity_check_issues` — so, like
/// `external_conflict_prompt`, it shows again only the next time a project
/// open actually finds something, never spuriously on an unrelated later
/// frame.
pub fn show(ctx: &egui::Context, issues: &[String]) -> bool {
    let mut dismissed = false;
    egui::Modal::new(egui::Id::new("sanity_check_modal")).show(ctx, |ui| {
        ui.set_min_width(420.0);
        ui.set_max_width(560.0);
        ui.heading("Project Sanity Check");
        ui.add_space(8.0);
        ui.label(format!(
            "Found {} issue{} comparing this project's saved metadata against \
             what's actually on disk — most likely from something changing the \
             vault outside Smaragd (a manual edit, a git pull, a sync merge):",
            issues.len(),
            if issues.len() == 1 { "" } else { "s" }
        ));
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .max_height(240.0)
            .show(ui, |ui| {
                for issue in issues {
                    ui.label(format!("\u{2022} {issue}"));
                }
            });
        ui.add_space(8.0);
        if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            dismissed = true;
        }
    });
    dismissed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Harness {
        ctx: egui::Context,
    }

    impl Harness {
        fn frame(&self, issues: &[String]) -> bool {
            let mut dismissed = false;
            crate::egui_test_support::run_ui_and_discard(
                &self.ctx,
                egui::RawInput::default(),
                |ui| {
                    dismissed = show(ui.ctx(), issues);
                },
            );
            dismissed
        }
    }

    #[test]
    fn not_dismissed_until_ok_is_clicked_or_escape_is_pressed() {
        let harness = Harness::default();
        assert!(!harness.frame(&["Something is wrong.".to_string()]));
    }
}
