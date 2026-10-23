use super::*;

/// Which value drives a binder row's background color — see
/// `ProjectMeta::binder_color_mode`. The three non-`Off` modes all read from
/// data that already exists per-row (a document's frontmatter, or a folder's
/// `folder_meta`), so switching modes never requires re-entering data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BinderColorMode {
    /// No background coloring at all — the default, so a fresh project's
    /// binder starts uncolored rather than opting new users into a scheme
    /// they haven't chosen.
    #[default]
    Off,
    /// See `Project::status_color_hex`.
    Status,
    /// Color by the row's assigned POV — see `Project::pov_color_hex`.
    Pov,
    /// A red→yellow→green gradient toward `word_count_target`: a document's
    /// own word count for a document row, or the *combined* descendant word
    /// count for a folder row — see `Project::folder_word_counts` and
    /// `color_theme::word_count_progress_color`.
    WordCountProgress,
}

impl BinderColorMode {
    /// Short, user-facing name — shared by the View menu's radio items and
    /// the status bar's active-mode indicator, so the two always agree.
    pub fn label(self) -> &'static str {
        match self {
            BinderColorMode::Off => "Off",
            BinderColorMode::Status => "Status",
            BinderColorMode::Pov => "POV",
            BinderColorMode::WordCountProgress => "Word Count",
        }
    }

    /// The next mode in the View menu's/shortcut's cycle order — see
    /// `SmaragdApp::cycle_binder_color_mode`.
    pub fn next(self) -> Self {
        match self {
            BinderColorMode::Off => BinderColorMode::Status,
            BinderColorMode::Status => BinderColorMode::Pov,
            BinderColorMode::Pov => BinderColorMode::WordCountProgress,
            BinderColorMode::WordCountProgress => BinderColorMode::Off,
        }
    }
}

impl Project {
    /// Switch which value drives a binder row's background color — see
    /// [`BinderColorMode`].
    pub fn set_binder_color_mode(&mut self, mode: BinderColorMode) -> io::Result<()> {
        self.meta.binder_color_mode = mode;
        self.save_metadata()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_names_each_mode() {
        assert_eq!(BinderColorMode::Off.label(), "Off");
        assert_eq!(BinderColorMode::Status.label(), "Status");
        assert_eq!(BinderColorMode::Pov.label(), "POV");
        assert_eq!(BinderColorMode::WordCountProgress.label(), "Word Count");
    }

    #[test]
    fn next_cycles_off_status_pov_word_count_progress_and_back_to_off() {
        assert_eq!(BinderColorMode::Off.next(), BinderColorMode::Status);
        assert_eq!(BinderColorMode::Status.next(), BinderColorMode::Pov);
        assert_eq!(
            BinderColorMode::Pov.next(),
            BinderColorMode::WordCountProgress
        );
        assert_eq!(
            BinderColorMode::WordCountProgress.next(),
            BinderColorMode::Off
        );
    }

    #[test]
    fn binder_color_mode_defaults_to_off() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        assert_eq!(project.meta.binder_color_mode, BinderColorMode::Off);
    }

    #[test]
    fn set_binder_color_mode_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project
            .set_binder_color_mode(BinderColorMode::WordCountProgress)
            .unwrap();

        assert_eq!(
            project.meta.binder_color_mode,
            BinderColorMode::WordCountProgress
        );
        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(
            reloaded.meta.binder_color_mode,
            BinderColorMode::WordCountProgress
        );
    }
}
