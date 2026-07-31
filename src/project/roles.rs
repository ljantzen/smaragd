use super::*;

/// A Scrivener-Research/Trash/Templates/Manuscript-style role assigned to a folder,
/// decoupled from its position in the tree. At most one folder project-wide holds
/// `Research`/`Trash`/`Templates` at a time (see [`FolderRole::is_exclusive`]);
/// `Manuscript` is the one exception — a project can have several Manuscript
/// folders at once (e.g. one per book in a series, or per POV thread), so
/// assigning it to a new folder never clears it from any other. `Research` is
/// currently just a marker — a forward-looking extension point for features
/// (Compile, word-count rollups) that don't exist yet. `Trash` has a real
/// behavior change: see [`Project::delete`]. `Templates`'s direct child
/// documents become the candidate list for "New From Template": see
/// [`Project::template_documents`]. `Manuscript` designates one or more
/// Scrivener-Draft-style primary content folders — see
/// [`Project::folder_role_paths`] and its use as the source list for "Export
/// Manuscript…".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FolderRole {
    Research,
    Trash,
    Templates,
    Manuscript,
}

impl FolderRole {
    /// Whether at most one folder project-wide may hold this role at a time —
    /// true for every role except `Manuscript`. Governs whether
    /// [`Project::set_folder_role`] clears any existing holder before assigning
    /// this role to a new folder.
    fn is_exclusive(self) -> bool {
        !matches!(self, FolderRole::Manuscript)
    }
}

impl Project {
    /// The role assigned to the folder at `path`, if any.
    pub fn folder_role(&self, path: &Path) -> Option<FolderRole> {
        self.meta
            .folder_roles
            .get(&relative_key(&self.root, path))
            .copied()
    }

    /// Assign `role` to the folder at `path` (`None` clears it). For an exclusive
    /// role (see [`FolderRole::is_exclusive`]) this clears it from wherever it was
    /// previously assigned first, mirroring Scrivener's singular Draft/Trash;
    /// `Manuscript` isn't exclusive, so assigning it to a new folder leaves any
    /// other Manuscript folder untouched. Errors if `path` isn't a directory.
    pub fn set_folder_role(&mut self, path: &Path, role: Option<FolderRole>) -> io::Result<()> {
        if !path.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a folder"));
        }
        let key = relative_key(&self.root, path);
        match role {
            Some(role) => {
                if role.is_exclusive() {
                    self.meta.folder_roles.retain(|_, r| *r != role);
                }
                self.meta.folder_roles.insert(key, role);
            }
            None => {
                self.meta.folder_roles.remove(&key);
            }
        }
        self.save_metadata()
    }

    /// Ensure a folder holding `role` exists, independently of any other role.
    /// If a folder is already assigned `role` but has been deleted from disk since
    /// (e.g. outside the app), recreates it at that same path, keeping the existing
    /// assignment. If no folder holds `role` at all, creates a new one named
    /// `default_name` at the project root (disambiguated via `unique_child_name` if
    /// that name's already taken there) and assigns it. A no-op if the assigned
    /// folder already exists.
    pub fn ensure_role_folder(&mut self, role: FolderRole, default_name: &str) -> io::Result<()> {
        let existing = self
            .meta
            .folder_roles
            .iter()
            .find(|(_, r)| **r == role)
            .map(|(key, _)| key.clone());

        if let Some(key) = existing {
            let path = if key.is_empty() {
                self.root.clone()
            } else {
                self.root.join(&key)
            };
            if !path.is_dir() {
                fs::create_dir_all(&path)?;
                self.rescan();
            }
            return Ok(());
        }

        let root = self.root.clone();
        let name = unique_child_name(&root, default_name);
        let path = self.create_folder(&root, &name)?;
        self.set_folder_role(&path, Some(role))
    }

    /// The absolute path of the first folder holding `role`, if any — for an
    /// exclusive role (see [`FolderRole::is_exclusive`]) this is *the* folder
    /// holding it, since at most one can. For `Manuscript`, which isn't
    /// exclusive, prefer [`Project::folder_role_paths`] to see every folder
    /// holding it; this is just its first result.
    pub(crate) fn folder_role_path(&self, role: FolderRole) -> Option<PathBuf> {
        self.folder_role_paths(role).into_iter().next()
    }

    /// The absolute path of every folder holding `role`, sorted for stable
    /// display order (e.g. listing multiple Manuscript folders to pick from in
    /// "Export Manuscript…"). Empty for an exclusive role with nothing assigned,
    /// and never more than one entry for an exclusive role.
    pub(crate) fn folder_role_paths(&self, role: FolderRole) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self
            .meta
            .folder_roles
            .iter()
            .filter(|(_, r)| **r == role)
            .map(|(key, _)| {
                if key.is_empty() {
                    self.root.clone()
                } else {
                    self.root.join(key)
                }
            })
            .collect();
        paths.sort();
        paths
    }

    /// The absolute path of the project's designated Trash folder, if any. Visible
    /// within the crate (not just this module) so `project_template::save_from_project`
    /// can exclude Trash's contents when saving a project's structure as a template.
    pub(crate) fn trash_path(&self) -> Option<PathBuf> {
        self.folder_role_path(FolderRole::Trash)
    }

    /// Whether deleting `path` right now would route it into Trash rather than
    /// permanently removing it — exposed so callers can word a delete confirmation
    /// accurately ("Move to Trash?" vs "This cannot be undone.").
    pub fn deletes_to_trash(&self, path: &Path) -> bool {
        self.trash_path()
            .is_some_and(|trash| path != trash && !path.starts_with(&trash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_folder_role_assigns_and_enforces_single_holder_per_role() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let a = project.create_folder(dir.path(), "A").unwrap();
        let b = project.create_folder(dir.path(), "B").unwrap();

        project
            .set_folder_role(&a, Some(FolderRole::Trash))
            .unwrap();
        assert_eq!(project.folder_role(&a), Some(FolderRole::Trash));

        project
            .set_folder_role(&b, Some(FolderRole::Trash))
            .unwrap();
        assert_eq!(project.folder_role(&b), Some(FolderRole::Trash));
        assert_eq!(project.folder_role(&a), None);
    }

    #[test]
    fn set_folder_role_clears_with_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let a = project.create_folder(dir.path(), "A").unwrap();
        project
            .set_folder_role(&a, Some(FolderRole::Research))
            .unwrap();

        project.set_folder_role(&a, None).unwrap();

        assert_eq!(project.folder_role(&a), None);
    }

    #[test]
    fn folder_role_path_resolves_the_manuscript_folders_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let manuscript = project.create_folder(dir.path(), "Manuscript").unwrap();

        project
            .set_folder_role(&manuscript, Some(FolderRole::Manuscript))
            .unwrap();

        assert_eq!(
            project.folder_role_path(FolderRole::Manuscript),
            Some(manuscript)
        );
    }

    #[test]
    fn folder_role_path_returns_none_when_unassigned() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(project.folder_role_path(FolderRole::Manuscript), None);
    }

    #[test]
    fn set_folder_role_allows_multiple_simultaneous_manuscript_folders() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let a = project.create_folder(dir.path(), "Book One").unwrap();
        let b = project.create_folder(dir.path(), "Book Two").unwrap();

        project
            .set_folder_role(&a, Some(FolderRole::Manuscript))
            .unwrap();
        project
            .set_folder_role(&b, Some(FolderRole::Manuscript))
            .unwrap();

        assert_eq!(project.folder_role(&a), Some(FolderRole::Manuscript));
        assert_eq!(project.folder_role(&b), Some(FolderRole::Manuscript));
        assert_eq!(project.folder_role_paths(FolderRole::Manuscript), {
            let mut expected = vec![a, b];
            expected.sort();
            expected
        });
    }

    #[test]
    fn set_folder_role_errors_for_a_document_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();

        let result = project.set_folder_role(&doc, Some(FolderRole::Research));

        assert!(result.is_err());
    }

    #[test]
    fn ensure_role_folder_creates_and_assigns_when_no_folder_holds_the_role() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project
            .ensure_role_folder(FolderRole::Research, "Research")
            .unwrap();

        let path = dir.path().join("Research");
        assert!(path.is_dir());
        assert_eq!(project.folder_role(&path), Some(FolderRole::Research));
    }

    #[test]
    fn ensure_role_folder_uniquifies_the_default_name_if_already_taken() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.create_folder(dir.path(), "Research").unwrap();

        project
            .ensure_role_folder(FolderRole::Research, "Research")
            .unwrap();

        let path = dir.path().join("Research (2)");
        assert!(path.is_dir());
        assert_eq!(project.folder_role(&path), Some(FolderRole::Research));
    }

    #[test]
    fn ensure_role_folder_is_a_no_op_when_the_assigned_folder_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();

        project
            .ensure_role_folder(FolderRole::Trash, "Trash")
            .unwrap();

        assert!(
            !dir.path().join("Trash (2)").exists(),
            "should not have created a second Trash-like folder"
        );
        assert_eq!(project.folder_role(&trash), Some(FolderRole::Trash));
    }

    #[test]
    fn ensure_role_folder_recreates_a_missing_assigned_folder_at_its_original_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        fs::remove_dir(&trash).unwrap();

        project
            .ensure_role_folder(FolderRole::Trash, "Trash")
            .unwrap();

        assert!(trash.is_dir());
        assert_eq!(project.folder_role(&trash), Some(FolderRole::Trash));
    }

    #[test]
    fn ensure_role_folder_treats_research_and_trash_independently() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();

        project
            .ensure_role_folder(FolderRole::Research, "Research")
            .unwrap();

        assert!(dir.path().join("Research").is_dir());
        assert_eq!(project.folder_role(&trash), Some(FolderRole::Trash));
        assert_eq!(
            project.folder_role(&dir.path().join("Research")),
            Some(FolderRole::Research)
        );
    }

    #[test]
    fn folder_role_returns_none_when_unassigned() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(project.folder_role(dir.path()), None);
    }
}
