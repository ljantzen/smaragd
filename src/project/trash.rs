use super::*;

impl Project {
    /// The absolute path this trashed item originally lived at, if `path` is
    /// currently a top-level item inside the designated Trash folder that was moved
    /// there via `delete` (as opposed to something nested inside a trashed folder, or
    /// created directly inside Trash — those were never individually recorded and
    /// aren't restorable on their own). Doubles as the "can this be restored?" check.
    pub fn trashed_origin(&self, path: &Path) -> Option<PathBuf> {
        self.meta
            .trashed_origins
            .get(&relative_key(&self.root, path))
            .map(|key| self.root.join(key))
    }

    /// Move a trashed item (see [`Project::trashed_origin`]) back to the folder it
    /// was deleted from, resolving a name collision the same way `move_to_trash`
    /// does. If that folder no longer exists: errors with
    /// `RestoreError::OriginalFolderMissing` when `recreate_missing_folder` is
    /// `false`, or recreates it and proceeds when `true` — callers offer that choice
    /// interactively rather than picking one silently.
    pub fn restore_from_trash(
        &mut self,
        path: &Path,
        recreate_missing_folder: bool,
    ) -> Result<PathBuf, RestoreError> {
        let key = relative_key(&self.root, path);
        let Some(original) = self
            .meta
            .trashed_origins
            .get(&key)
            .cloned()
            .map(|k| self.root.join(k))
        else {
            return Err(RestoreError::NotTrashed);
        };
        let original_parent = original.parent().unwrap_or(&self.root).to_path_buf();
        if !original_parent.is_dir() {
            if recreate_missing_folder {
                fs::create_dir_all(&original_parent)?;
            } else {
                return Err(RestoreError::OriginalFolderMissing(original_parent));
            }
        }
        // Remove this item's own trashed_origins entry *before* move_node runs, not
        // after: move_node (via rewrite_relative_key_prefix, for a folder) now also
        // follows folder_roles/trashed_origins keys under the moved prefix, which for
        // a restored folder would otherwise race with this very removal — the rewrite
        // would relocate the entry to the new key first, leaving nothing for this
        // line to find, and stranding a stale, self-referential entry behind instead
        // of clearing it.
        self.meta.trashed_origins.remove(&key);
        let dest = self.move_node(path, &original_parent)?;
        self.save_metadata()?;
        self.rescan();
        Ok(dest)
    }

    /// Permanently remove everything currently inside the designated Trash folder. A
    /// no-op if no Trash folder is designated or it's already empty.
    pub fn empty_trash(&mut self) -> io::Result<()> {
        let Some(trash) = self.trash_path() else {
            return Ok(());
        };
        let Some(node) = self.tree.find_by_path(&trash) else {
            return Ok(());
        };
        let children: Vec<PathBuf> = node.children().iter().map(|c| c.path.clone()).collect();
        for child in children {
            self.permanently_delete(&child)?;
        }
        Ok(())
    }

    /// Delete the file or folder at `path` (a folder is removed recursively), drop it
    /// (and, for a folder, its descendants) from manual ordering and any assigned
    /// folder role / trashed-origin record, and rescan. Bypasses Trash entirely —
    /// used both when no Trash is configured and to actually clear something out of
    /// Trash (via [`Project::delete`]'s routing, or [`Project::empty_trash`]).
    pub(super) fn permanently_delete(&mut self, path: &Path) -> io::Result<()> {
        let is_dir = path.is_dir();
        if is_dir {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }

        if let Some(parent) = path.parent()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
        {
            let parent_key = relative_key(&self.root, parent);
            if let Some(order) = self.meta.node_order.get_mut(&parent_key) {
                order.retain(|entry| entry != name);
            }
        }
        let key = relative_key(&self.root, path);
        if is_dir {
            let prefix = key;
            let under_prefix = |k: &String| *k == prefix || k.starts_with(&format!("{prefix}/"));
            self.meta.node_order.retain(|k, _| !under_prefix(k));
            self.meta.folder_roles.retain(|k, _| !under_prefix(k));
            self.meta.trashed_origins.retain(|k, _| !under_prefix(k));
            self.meta.folder_meta.retain(|k, _| !under_prefix(k));
        } else {
            self.meta.folder_roles.remove(&key);
            self.meta.trashed_origins.remove(&key);
        }

        self.save_metadata()?;
        self.rescan();
        Ok(())
    }

    /// Rewrite every `node_order`/`folder_roles`/`trashed_origins`/`folder_meta`
    /// key under `old_prefix` (the folder itself and all its descendants) to sit
    /// under `new_prefix` instead, following a folder rename or move — so a role
    /// (Research/Trash) or folder metadata (status/type/...) assigned to the
    /// moved folder or something inside it, or a trashed item's current-location
    /// bookkeeping, keeps pointing at where the thing actually is instead of a
    /// now-nonexistent path. `trashed_origins`' *values* (each item's pre-trash
    /// location) are deliberately left untouched: they're history, not a
    /// reference to something that just moved. `status_colors` needs no
    /// rewriting here — it's keyed by status text, not by path.
    pub(super) fn rewrite_relative_key_prefix(&mut self, old_prefix: &str, new_prefix: &str) {
        rewrite_prefix_in(&mut self.meta.node_order, old_prefix, new_prefix);
        rewrite_prefix_in(&mut self.meta.folder_roles, old_prefix, new_prefix);
        rewrite_prefix_in(&mut self.meta.trashed_origins, old_prefix, new_prefix);
        rewrite_prefix_in(&mut self.meta.folder_meta, old_prefix, new_prefix);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletes_to_trash_reports_correctly_for_configured_vs_unconfigured_trash() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();

        assert!(!project.deletes_to_trash(&doc));

        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();

        assert!(project.deletes_to_trash(&doc));
        assert!(!project.deletes_to_trash(&trash));
    }

    #[test]
    fn delete_moves_into_designated_trash_folder_instead_of_removing_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let doc = project.create_document(dir.path(), "Doomed").unwrap();

        project.delete(&doc).unwrap();

        assert!(!doc.exists());
        let expected = trash.join("Doomed.md");
        assert!(expected.exists());
        assert!(project.tree.find_by_path(&expected).is_some());
    }

    #[test]
    fn delete_of_the_trash_folder_itself_permanently_removes_it_and_clears_the_role() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();

        project.delete(&trash).unwrap();

        assert!(!trash.exists());
        assert_eq!(project.folder_role(&trash), None);
    }

    #[test]
    fn delete_of_an_item_already_inside_trash_permanently_removes_it_instead_of_re_trashing() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let doc = project.create_document(&trash, "Already Trashed").unwrap();

        project.delete(&doc).unwrap();

        assert!(!doc.exists());
        assert!(project.tree.find_by_path(&doc).is_none());
    }

    #[test]
    fn move_to_trash_uniquifies_a_colliding_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        fs::write(trash.join("Doomed.md"), "").unwrap();
        let doc = project.create_document(dir.path(), "Doomed").unwrap();

        project.delete(&doc).unwrap();

        assert!(trash.join("Doomed (2).md").exists());
    }

    #[test]
    fn move_to_trash_preserves_nested_order_keys_for_a_trashed_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let chapter = project.create_folder(dir.path(), "Old Chapter").unwrap();
        project.create_document(&chapter, "Scene 1").unwrap();

        project.delete(&chapter).unwrap();

        let moved = trash.join("Old Chapter");
        assert!(moved.join("Scene 1.md").exists());
        assert_eq!(
            project.meta.node_order.get("Trash/Old Chapter"),
            Some(&vec!["Scene 1.md".to_string()])
        );
        assert!(!project.meta.node_order.contains_key("Old Chapter"));
    }

    #[test]
    fn move_to_trash_records_the_original_relative_path_in_trashed_origins() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let doc = project.create_document(&chapter, "Notes").unwrap();

        project.delete(&doc).unwrap();

        assert_eq!(
            project.meta.trashed_origins.get("Trash/Notes.md"),
            Some(&"Chapter 1/Notes.md".to_string())
        );
    }

    #[test]
    fn move_to_trash_disambiguates_two_same_named_items_from_different_folders() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let chapter1 = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let chapter2 = project.create_folder(dir.path(), "Chapter 2").unwrap();
        let doc1 = project.create_document(&chapter1, "notes").unwrap();
        let doc2 = project.create_document(&chapter2, "notes").unwrap();

        project.delete(&doc1).unwrap();
        project.delete(&doc2).unwrap();

        assert!(trash.join("notes.md").exists());
        assert!(trash.join("notes (2).md").exists());
        assert_eq!(
            project.meta.trashed_origins.get("Trash/notes.md"),
            Some(&"Chapter 1/notes.md".to_string())
        );
        assert_eq!(
            project.meta.trashed_origins.get("Trash/notes (2).md"),
            Some(&"Chapter 2/notes.md".to_string())
        );
    }

    #[test]
    fn empty_trash_permanently_removes_all_trashed_items_and_their_origin_records() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let doc = project.create_document(dir.path(), "Doomed").unwrap();
        project.delete(&doc).unwrap();
        assert!(!project.meta.trashed_origins.is_empty());

        project.empty_trash().unwrap();

        assert!(!trash.join("Doomed.md").exists());
        assert!(project.meta.trashed_origins.is_empty());
        assert_eq!(project.folder_role(&trash), Some(FolderRole::Trash));
    }

    #[test]
    fn empty_trash_is_a_no_op_when_no_trash_folder_is_designated() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        assert!(project.empty_trash().is_ok());
    }

    fn project_with_trashed_doc(dir: &Path) -> (Project, PathBuf, PathBuf, PathBuf) {
        let mut project = Project::initialize(dir).unwrap();
        let trash = project.create_folder(dir, "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let chapter = project.create_folder(dir, "Chapter 1").unwrap();
        let doc = project.create_document(&chapter, "Notes").unwrap();
        project.delete(&doc).unwrap();
        let trashed = trash.join("Notes.md");
        (project, trash, chapter, trashed)
    }

    #[test]
    fn trashed_origin_returns_the_original_absolute_path_for_a_trashed_item() {
        let dir = tempfile::tempdir().unwrap();
        let (project, _trash, chapter, trashed) = project_with_trashed_doc(dir.path());

        assert_eq!(
            project.trashed_origin(&trashed),
            Some(chapter.join("Notes.md"))
        );
    }

    #[test]
    fn trashed_origin_returns_none_for_a_non_trashed_path() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(project.trashed_origin(dir.path()), None);
    }

    #[test]
    fn restore_from_trash_moves_item_back_to_its_original_folder_and_clears_the_origin_record() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, _trash, chapter, trashed) = project_with_trashed_doc(dir.path());

        let restored = project.restore_from_trash(&trashed, false).unwrap();

        assert_eq!(restored, chapter.join("Notes.md"));
        assert!(restored.exists());
        assert!(!trashed.exists());
        assert!(project.trashed_origin(&restored).is_none());
        assert!(project.tree.find_by_path(&restored).is_some());
    }

    #[test]
    fn restore_from_trash_errors_when_path_is_not_a_recorded_trashed_item() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();

        let result = project.restore_from_trash(&doc, false);

        assert!(matches!(result, Err(RestoreError::NotTrashed)));
    }

    #[test]
    fn restore_from_trash_errors_when_original_folder_is_missing_and_recreate_is_false() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, _trash, chapter, trashed) = project_with_trashed_doc(dir.path());
        project.delete(&chapter).unwrap(); // moves "Chapter 1" itself into Trash

        let result = project.restore_from_trash(&trashed, false);

        assert!(matches!(
            result,
            Err(RestoreError::OriginalFolderMissing(_))
        ));
        assert!(trashed.exists()); // left in place, not moved
    }

    #[test]
    fn restore_from_trash_recreates_the_original_folder_when_recreate_is_true() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, _trash, chapter, trashed) = project_with_trashed_doc(dir.path());
        fs::remove_dir_all(&chapter).unwrap(); // "Chapter 1" is gone, but not via project.delete

        let restored = project.restore_from_trash(&trashed, true).unwrap();

        assert!(chapter.is_dir());
        assert_eq!(restored, chapter.join("Notes.md"));
        assert!(restored.exists());
    }

    #[test]
    fn restore_from_trash_uniquifies_when_something_now_occupies_the_original_name() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, _trash, chapter, trashed) = project_with_trashed_doc(dir.path());
        fs::write(chapter.join("Notes.md"), "new content").unwrap();

        let restored = project.restore_from_trash(&trashed, false).unwrap();

        assert_eq!(restored, chapter.join("Notes (2).md"));
        assert_eq!(
            fs::read_to_string(chapter.join("Notes.md")).unwrap(),
            "new content"
        );
    }

    #[test]
    fn restore_from_trash_of_a_folder_preserves_nested_order_keys_and_removes_trashs_order_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let old_chapter = project.create_folder(dir.path(), "Old Chapter").unwrap();
        project.create_document(&old_chapter, "Scene 1").unwrap();
        project.delete(&old_chapter).unwrap();
        let trashed = trash.join("Old Chapter");

        let restored = project.restore_from_trash(&trashed, false).unwrap();

        assert_eq!(restored, dir.path().join("Old Chapter"));
        assert!(restored.join("Scene 1.md").exists());
        assert_eq!(
            project.meta.node_order.get("Old Chapter"),
            Some(&vec!["Scene 1.md".to_string()])
        );
        assert!(!project.meta.node_order.contains_key("Trash/Old Chapter"));
        assert_eq!(project.meta.node_order.get("Trash"), Some(&vec![]));
    }
}
