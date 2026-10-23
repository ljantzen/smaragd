use super::*;

impl Project {
    /// The hex color string assigned to `pov` (trimmed, matched exactly as
    /// typed, case-sensitive), if any — see `ProjectMeta::pov_colors`. `None`
    /// for a blank or unassigned POV. The `ui`/`app` layers convert this to
    /// `egui::Color32` via `color_theme::parse_hex_color`, since this module
    /// (like the rest of `project/`) stays free of an `egui` dependency.
    pub fn pov_color_hex(&self, pov: &str) -> Option<&str> {
        let pov = pov.trim();
        if pov.is_empty() {
            return None;
        }
        self.meta.pov_colors.get(pov).map(String::as_str)
    }

    /// Assign `hex` (e.g. `"#RRGGBB"`, from `color_theme::to_hex_string`) as
    /// `pov`'s binder background color. A no-op (not an error) for a blank
    /// `pov` — there's nothing meaningful to key it by.
    pub fn set_pov_color_hex(&mut self, pov: &str, hex: String) -> io::Result<()> {
        let pov = pov.trim();
        if pov.is_empty() {
            return Ok(());
        }
        self.meta.pov_colors.insert(pov.to_string(), hex);
        self.save_metadata()
    }

    /// Clear `pov`'s assigned color, if any.
    pub fn clear_pov_color(&mut self, pov: &str) -> io::Result<()> {
        self.meta.pov_colors.remove(pov.trim());
        self.save_metadata()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pov_color_hex_returns_none_for_an_unassigned_pov() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        assert_eq!(project.pov_color_hex("Alice"), None);
    }

    #[test]
    fn set_pov_color_then_pov_color_hex_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project
            .set_pov_color_hex("Alice", "#8800ff".to_string())
            .unwrap();

        assert_eq!(project.pov_color_hex("Alice"), Some("#8800ff"));
        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(reloaded.pov_color_hex("Alice"), Some("#8800ff"));
    }

    #[test]
    fn set_pov_color_is_a_no_op_for_a_blank_pov() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project
            .set_pov_color_hex("   ", "#8800ff".to_string())
            .unwrap();

        assert!(project.meta.pov_colors.is_empty());
    }

    #[test]
    fn clear_pov_color_removes_an_assigned_color() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project
            .set_pov_color_hex("Alice", "#8800ff".to_string())
            .unwrap();

        project.clear_pov_color("Alice").unwrap();

        assert_eq!(project.pov_color_hex("Alice"), None);
    }

    #[test]
    fn pov_color_hex_trims_and_matches_case_sensitively() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project
            .set_pov_color_hex("Alice", "#8800ff".to_string())
            .unwrap();

        assert_eq!(project.pov_color_hex("  Alice  "), Some("#8800ff"));
        assert_eq!(project.pov_color_hex("alice"), None);
    }
}
