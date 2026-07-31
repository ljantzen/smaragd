use super::*;

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
