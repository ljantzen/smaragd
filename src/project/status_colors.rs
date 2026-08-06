use super::*;

impl Project {
    /// The hex color string assigned to `status` (trimmed, matched exactly as
    /// typed, case-sensitive), if any — see `ProjectMeta::status_colors`.
    /// `None` for a blank or unassigned status. The `ui`/`app` layers convert
    /// this to `egui::Color32` via `color_theme::parse_hex_color`, since this
    /// module (like the rest of `project/`) stays free of an `egui` dependency.
    pub fn status_color_hex(&self, status: &str) -> Option<&str> {
        let status = status.trim();
        if status.is_empty() {
            return None;
        }
        self.meta.status_colors.get(status).map(String::as_str)
    }

    /// Assign `hex` (e.g. `"#RRGGBB"`, from `color_theme::to_hex_string`) as
    /// `status`'s binder background color. A no-op (not an error) for a blank
    /// `status` — there's nothing meaningful to key it by.
    pub fn set_status_color_hex(&mut self, status: &str, hex: String) -> io::Result<()> {
        let status = status.trim();
        if status.is_empty() {
            return Ok(());
        }
        self.meta.status_colors.insert(status.to_string(), hex);
        self.save_metadata()
    }

    /// Clear `status`'s assigned color, if any.
    pub fn clear_status_color(&mut self, status: &str) -> io::Result<()> {
        self.meta.status_colors.remove(status.trim());
        self.save_metadata()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_color_hex_returns_none_for_an_unassigned_status() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        assert_eq!(project.status_color_hex("draft"), None);
    }

    #[test]
    fn set_status_color_then_status_color_hex_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project
            .set_status_color_hex("draft", "#ff8800".to_string())
            .unwrap();

        assert_eq!(project.status_color_hex("draft"), Some("#ff8800"));
        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(reloaded.status_color_hex("draft"), Some("#ff8800"));
    }

    #[test]
    fn set_status_color_is_a_no_op_for_a_blank_status() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project
            .set_status_color_hex("   ", "#ff8800".to_string())
            .unwrap();

        assert!(project.meta.status_colors.is_empty());
    }

    #[test]
    fn clear_status_color_removes_an_assigned_color() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project
            .set_status_color_hex("draft", "#ff8800".to_string())
            .unwrap();

        project.clear_status_color("draft").unwrap();

        assert_eq!(project.status_color_hex("draft"), None);
    }

    #[test]
    fn status_color_hex_trims_and_matches_case_sensitively() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project
            .set_status_color_hex("Draft", "#ff8800".to_string())
            .unwrap();

        assert_eq!(project.status_color_hex("  Draft  "), Some("#ff8800"));
        assert_eq!(project.status_color_hex("draft"), None);
    }
}
