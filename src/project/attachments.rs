use super::*;

/// `ProjectMeta::clipboard_image_size_limit_mb == 0` (not yet configured)
/// resolves to this — see `Project::clipboard_image_size_limit_mb`.
const DEFAULT_CLIPBOARD_IMAGE_SIZE_LIMIT_MB: u32 = 20;

/// Where a pasted/dropped attachment (image, PDF, ...) is saved — mirrors
/// Obsidian's "default location for new attachments" setting. Deliberately
/// *not* a [`FolderRole`]: see [`PicklistField`]'s doc comment for the same
/// argument — a folder assigned here gets no other special behavior, so an
/// existing folder that already serves another purpose can double as the
/// attachments destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AttachmentDestination {
    /// Next to the document being edited — the default: zero setup, and
    /// `ui::markdown_preview::resolve_image_uri` already joins a relative
    /// image `src` onto the document's own directory, so this needs no path
    /// rewriting at all.
    #[default]
    SameAsDocument,
    /// A single project-relative folder for every attachment, regardless of
    /// which document they're inserted from.
    ConfiguredFolder,
}

impl Project {
    pub fn attachment_destination(&self) -> AttachmentDestination {
        self.meta.attachment_destination
    }

    pub fn set_attachment_destination(&mut self, mode: AttachmentDestination) -> io::Result<()> {
        self.meta.attachment_destination = mode;
        self.save_metadata()
    }

    /// The absolute path of the configured attachments folder, if one is set.
    pub fn attachments_folder(&self) -> Option<PathBuf> {
        self.meta.attachments_folder.as_deref().map(|key| {
            if key.is_empty() {
                self.root.clone()
            } else {
                self.root.join(key)
            }
        })
    }

    /// Assign `path` as the attachments folder (`None` clears it). Errors if
    /// `path` isn't a directory inside the project — same guard shape as
    /// [`Project::set_picklist_folder`], and for the same reason: the stored
    /// value is a project-relative key, which has no valid encoding for a
    /// path outside `self.root`.
    pub fn set_attachments_folder(&mut self, path: Option<&Path>) -> io::Result<()> {
        if let Some(path) = path {
            if !self.store.is_dir(path) {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a folder"));
            }
            if path.strip_prefix(&self.root).is_err() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "attachments folder must be inside the project",
                ));
            }
        }
        self.meta.attachments_folder = path.map(|path| relative_key(&self.root, path));
        self.save_metadata()
    }

    pub fn clipboard_image_size_limit_enabled(&self) -> bool {
        self.meta.clipboard_image_size_limit_enabled
    }

    /// `clipboard_image_size_limit_mb == 0` (not yet configured) resolves to
    /// this — same blank-means-unset convention as
    /// `ProjectMeta::resolve_git_auto_commit_interval_minutes`. Also keeps an
    /// enabled-but-never-set limit from meaning "block every image" (0 MB).
    pub fn clipboard_image_size_limit_mb(&self) -> u32 {
        if self.meta.clipboard_image_size_limit_mb > 0 {
            self.meta.clipboard_image_size_limit_mb
        } else {
            DEFAULT_CLIPBOARD_IMAGE_SIZE_LIMIT_MB
        }
    }

    /// The configured clipboard-image cap in bytes, or `None` when disabled —
    /// what [`crate::attachments::clipboard_image_png`] enforces.
    pub fn clipboard_image_size_limit_bytes(&self) -> Option<u64> {
        self.meta
            .clipboard_image_size_limit_enabled
            .then_some(u64::from(self.clipboard_image_size_limit_mb()) * 1024 * 1024)
    }

    pub fn set_clipboard_image_size_limit_enabled(&mut self, enabled: bool) -> io::Result<()> {
        self.meta.clipboard_image_size_limit_enabled = enabled;
        self.save_metadata()
    }

    pub fn set_clipboard_image_size_limit_mb(&mut self, mb: u32) -> io::Result<()> {
        self.meta.clipboard_image_size_limit_mb = mb;
        self.save_metadata()
    }

    /// Where a new attachment should be written: the configured folder if
    /// `attachment_destination()` is `ConfiguredFolder` and one is actually
    /// set, else `document_dir` (the open document's own directory), else the
    /// project root — same three-level fallback idiom as
    /// `app::import::finish_import`'s `selected_path` parent resolution.
    pub fn resolve_attachment_dir(&self, document_dir: Option<&Path>) -> PathBuf {
        if self.attachment_destination() == AttachmentDestination::ConfiguredFolder
            && let Some(folder) = self.attachments_folder()
        {
            return folder;
        }
        document_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.root.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_attachment_dir_defaults_to_the_document_directory() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let docs = dir.path().join("Chapters");

        assert_eq!(project.resolve_attachment_dir(Some(&docs)), docs);
    }

    #[test]
    fn resolve_attachment_dir_falls_back_to_project_root_with_no_document_open() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(project.resolve_attachment_dir(None), project.root);
    }

    #[test]
    fn resolve_attachment_dir_uses_the_configured_folder_when_set() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let attachments = project.create_folder(dir.path(), "Attachments").unwrap();
        project
            .set_attachment_destination(AttachmentDestination::ConfiguredFolder)
            .unwrap();
        project
            .set_attachments_folder(Some(&attachments))
            .unwrap();
        let docs = dir.path().join("Chapters");

        assert_eq!(project.resolve_attachment_dir(Some(&docs)), attachments);
    }

    #[test]
    fn resolve_attachment_dir_falls_back_when_configured_mode_has_no_folder_set() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project
            .set_attachment_destination(AttachmentDestination::ConfiguredFolder)
            .unwrap();
        let docs = dir.path().join("Chapters");

        assert_eq!(project.resolve_attachment_dir(Some(&docs)), docs);
    }

    #[test]
    fn set_attachments_folder_rejects_a_path_outside_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let outside = tempfile::tempdir().unwrap();

        assert!(
            project
                .set_attachments_folder(Some(outside.path()))
                .is_err()
        );
    }

    #[test]
    fn set_attachments_folder_rejects_a_document_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Note").unwrap();

        assert!(project.set_attachments_folder(Some(&doc)).is_err());
    }

    #[test]
    fn set_attachments_folder_none_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let attachments = project.create_folder(dir.path(), "Attachments").unwrap();
        project
            .set_attachments_folder(Some(&attachments))
            .unwrap();
        project.set_attachments_folder(None).unwrap();

        assert!(project.attachments_folder().is_none());
    }

    #[test]
    fn clipboard_image_size_limit_bytes_is_none_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.set_clipboard_image_size_limit_mb(20).unwrap();

        assert_eq!(project.clipboard_image_size_limit_bytes(), None);
    }

    #[test]
    fn clipboard_image_size_limit_bytes_converts_megabytes() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.set_clipboard_image_size_limit_enabled(true).unwrap();
        project.set_clipboard_image_size_limit_mb(20).unwrap();

        assert_eq!(
            project.clipboard_image_size_limit_bytes(),
            Some(20 * 1024 * 1024)
        );
    }

    #[test]
    fn clipboard_image_size_limit_mb_resolves_an_unset_zero_to_a_real_default() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project
            .set_clipboard_image_size_limit_enabled(true)
            .unwrap();

        assert_ne!(project.clipboard_image_size_limit_mb(), 0);
        assert_ne!(project.clipboard_image_size_limit_bytes(), Some(0));
    }
}
