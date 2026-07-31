use super::*;

/// Which metadata field a folder's direct child documents' titles populate as
/// dropdown options in the Metadata panel — see [`Project::picklist_documents`].
/// Deliberately *not* a [`FolderRole`]: a folder assigned here gets no other special
/// behavior (no export exclusion, no Trash-style semantics, no project-wide
/// exclusivity across fields) — it's purely a pointer used to build a dropdown, so an
/// existing folder that already serves another purpose (e.g. a Research subfolder of
/// character bios) can double as a picklist source without anything else about it
/// changing. Each field is its own independent slot on [`ProjectMeta`] (see
/// `type_picklist_folder`/`pov_picklist_folder`/`status_picklist_folder`), so —
/// unlike `FolderRole`'s single shared map — assigning one field's folder never
/// affects another field's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PicklistField {
    Type,
    Pov,
    Status,
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_documents_is_empty_when_no_folder_holds_the_templates_role() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let templates = project.create_folder(dir.path(), "Templates").unwrap();
        project.create_document(&templates, "Character").unwrap();

        assert!(project.template_documents().is_empty());
    }

    #[test]
    fn template_documents_lists_only_direct_child_documents_of_the_templates_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let templates = project.create_folder(dir.path(), "Templates").unwrap();
        project
            .set_folder_role(&templates, Some(FolderRole::Templates))
            .unwrap();
        project
            .create_document(&templates, "Character Sheet")
            .unwrap();
        let nested = project.create_folder(&templates, "Nested").unwrap();
        project.create_document(&nested, "Too Deep").unwrap();

        let names: Vec<&str> = project
            .template_documents()
            .iter()
            .map(|doc| doc.name.as_str())
            .collect();

        assert_eq!(names, vec!["Character Sheet.md"]);
    }

    #[test]
    fn picklist_documents_is_empty_when_the_field_has_no_folder_assigned() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let people = project.create_folder(dir.path(), "People").unwrap();
        project.create_document(&people, "Alice").unwrap();

        assert!(project.picklist_documents(PicklistField::Pov).is_empty());
    }

    #[test]
    fn picklist_documents_lists_only_direct_child_documents_of_the_assigned_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let people = project.create_folder(dir.path(), "People").unwrap();
        project
            .set_picklist_folder(PicklistField::Pov, Some(&people))
            .unwrap();
        project.create_document(&people, "Alice").unwrap();
        let nested = project.create_folder(&people, "Nested").unwrap();
        project.create_document(&nested, "Too Deep").unwrap();

        let names: Vec<&str> = project
            .picklist_documents(PicklistField::Pov)
            .iter()
            .map(|doc| doc.name.as_str())
            .collect();

        assert_eq!(names, vec!["Alice.md"]);
    }

    #[test]
    fn set_picklist_folder_is_independent_per_field() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let people = project.create_folder(dir.path(), "People").unwrap();
        let types = project.create_folder(dir.path(), "Types").unwrap();
        project
            .set_picklist_folder(PicklistField::Pov, Some(&people))
            .unwrap();
        project
            .set_picklist_folder(PicklistField::Type, Some(&types))
            .unwrap();

        assert!(project.is_picklist_folder(PicklistField::Pov, &people));
        assert!(!project.is_picklist_folder(PicklistField::Type, &people));
        assert!(project.is_picklist_folder(PicklistField::Type, &types));
        assert!(!project.is_picklist_folder(PicklistField::Status, &types));
    }

    #[test]
    fn set_picklist_folder_reassignment_supersedes_the_previous_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let old = project.create_folder(dir.path(), "Old").unwrap();
        let new = project.create_folder(dir.path(), "New").unwrap();
        project
            .set_picklist_folder(PicklistField::Status, Some(&old))
            .unwrap();
        project
            .set_picklist_folder(PicklistField::Status, Some(&new))
            .unwrap();

        assert!(!project.is_picklist_folder(PicklistField::Status, &old));
        assert!(project.is_picklist_folder(PicklistField::Status, &new));
    }

    #[test]
    fn set_picklist_folder_none_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let people = project.create_folder(dir.path(), "People").unwrap();
        project
            .set_picklist_folder(PicklistField::Pov, Some(&people))
            .unwrap();
        project
            .set_picklist_folder(PicklistField::Pov, None)
            .unwrap();

        assert!(!project.is_picklist_folder(PicklistField::Pov, &people));
        assert!(project.picklist_documents(PicklistField::Pov).is_empty());
    }

    #[test]
    fn set_picklist_folder_errors_for_a_document_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Note").unwrap();

        assert!(
            project
                .set_picklist_folder(PicklistField::Type, Some(&doc))
                .is_err()
        );
    }
}
