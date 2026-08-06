use super::*;
use crate::frontmatter::DocumentMeta;

impl Project {
    /// Folder-level metadata for the folder at `path` — the same shape as a
    /// document's frontmatter (see `ProjectMeta::folder_meta`'s doc comment
    /// for why only `status` currently does anything). Returns
    /// `DocumentMeta::default()` for a folder with nothing assigned yet,
    /// mirroring `document_meta`'s own default-on-absence behavior.
    pub fn folder_meta(&self, path: &Path) -> DocumentMeta {
        self.meta
            .folder_meta
            .get(&relative_key(&self.root, path))
            .cloned()
            .unwrap_or_default()
    }

    /// Assign `meta` as the folder at `path`'s metadata. `DocumentMeta::default()`
    /// removes the entry rather than storing an empty one, mirroring
    /// `set_folder_role`'s `None`-clears convention. Errors if `path` isn't a
    /// directory.
    pub fn set_folder_meta(&mut self, path: &Path, meta: DocumentMeta) -> io::Result<()> {
        if !path.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a folder"));
        }
        let key = relative_key(&self.root, path);
        if meta == DocumentMeta::default() {
            self.meta.folder_meta.remove(&key);
        } else {
            self.meta.folder_meta.insert(key, meta);
        }
        self.save_metadata()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_meta_returns_default_for_a_folder_with_nothing_assigned() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        assert_eq!(project.folder_meta(dir.path()), DocumentMeta::default());
    }

    #[test]
    fn set_folder_meta_persists_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();

        let meta = DocumentMeta {
            section_type: Some("Chapter".to_string()),
            status: Some("draft".to_string()),
            pov: Some("Alice".to_string()),
            word_count_target: Some(3000),
            tags: vec!["arc-1".to_string()],
        };
        project.set_folder_meta(&chapter, meta.clone()).unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(reloaded.folder_meta(&chapter), meta);
    }

    #[test]
    fn set_folder_meta_with_default_meta_removes_rather_than_stores_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();

        project
            .set_folder_meta(
                &chapter,
                DocumentMeta {
                    status: Some("draft".to_string()),
                    ..DocumentMeta::default()
                },
            )
            .unwrap();
        assert!(!project.meta.folder_meta.is_empty());

        project
            .set_folder_meta(&chapter, DocumentMeta::default())
            .unwrap();
        assert!(project.meta.folder_meta.is_empty());
    }

    #[test]
    fn set_folder_meta_errors_for_a_document_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc A").unwrap();

        assert!(
            project
                .set_folder_meta(&doc, DocumentMeta::default())
                .is_err()
        );
    }
}
