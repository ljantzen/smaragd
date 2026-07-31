use super::*;

impl Project {
    /// The documents (folders excluded) directly inside the project's designated
    /// Templates folder, if any — the candidate list for "New From Template". Not
    /// recursive: a template in a subfolder of the Templates folder isn't listed.
    pub fn template_documents(&self) -> Vec<&BinderNode> {
        let Some(path) = self.templates_path() else {
            return Vec::new();
        };
        let Some(folder) = self.tree.find_by_path(&path) else {
            return Vec::new();
        };
        folder
            .children()
            .iter()
            .filter(|child| matches!(child.kind, BinderNodeKind::Document))
            .collect()
    }

    /// The absolute path of the project's designated Templates folder, if any.
    fn templates_path(&self) -> Option<PathBuf> {
        self.meta
            .folder_roles
            .iter()
            .find(|(_, role)| **role == FolderRole::Templates)
            .map(|(key, _)| {
                if key.is_empty() {
                    self.root.clone()
                } else {
                    self.root.join(key)
                }
            })
    }

    /// The relative key (as persisted in `ProjectMeta::{type,pov,status}_picklist_folder`)
    /// of `field`'s currently assigned picklist folder, if any.
    fn picklist_folder_key(&self, field: PicklistField) -> Option<&str> {
        match field {
            PicklistField::Type => self.meta.type_picklist_folder.as_deref(),
            PicklistField::Pov => self.meta.pov_picklist_folder.as_deref(),
            PicklistField::Status => self.meta.status_picklist_folder.as_deref(),
        }
    }

    /// The documents (folders excluded) directly inside `field`'s assigned picklist
    /// folder, if any — the dropdown options for that metadata field in the Metadata
    /// panel (each document's title is one option). Not recursive: a document in a
    /// subfolder of the picklist folder isn't listed. Mirrors
    /// [`Project::template_documents`] exactly, but resolves its folder from
    /// `PicklistField`'s own dedicated slot rather than a shared [`FolderRole`] map.
    pub fn picklist_documents(&self, field: PicklistField) -> Vec<&BinderNode> {
        let Some(key) = self.picklist_folder_key(field) else {
            return Vec::new();
        };
        let path = if key.is_empty() {
            self.root.clone()
        } else {
            self.root.join(key)
        };
        let Some(folder) = self.tree.find_by_path(&path) else {
            return Vec::new();
        };
        folder
            .children()
            .iter()
            .filter(|child| matches!(child.kind, BinderNodeKind::Document))
            .collect()
    }

    /// Whether `path` is currently `field`'s assigned picklist folder — drives the
    /// binder's "Dropdown Source" checkboxes.
    pub fn is_picklist_folder(&self, field: PicklistField, path: &Path) -> bool {
        self.picklist_folder_key(field) == Some(relative_key(&self.root, path).as_str())
    }

    /// Assign `path` as `field`'s picklist folder (`None` clears it). Unlike
    /// [`Project::set_folder_role`], there's no cross-field exclusivity to enforce —
    /// each field is its own independent slot on `ProjectMeta`, so setting `Type`'s
    /// folder can never disturb `Pov`'s or `Status`'s. Errors if `path` isn't a
    /// directory.
    pub fn set_picklist_folder(
        &mut self,
        field: PicklistField,
        path: Option<&Path>,
    ) -> io::Result<()> {
        if let Some(path) = path
            && !path.is_dir()
        {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a folder"));
        }
        let key = path.map(|path| relative_key(&self.root, path));
        match field {
            PicklistField::Type => self.meta.type_picklist_folder = key,
            PicklistField::Pov => self.meta.pov_picklist_folder = key,
            PicklistField::Status => self.meta.status_picklist_folder = key,
        }
        self.save_metadata()
    }
}
