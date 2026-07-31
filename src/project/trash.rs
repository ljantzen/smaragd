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
        } else {
            self.meta.folder_roles.remove(&key);
            self.meta.trashed_origins.remove(&key);
        }

        self.save_metadata()?;
        self.rescan();
        Ok(())
    }

    /// Rewrite every `node_order`/`folder_roles`/`trashed_origins` key under
    /// `old_prefix` (the folder itself and all its descendants) to sit under
    /// `new_prefix` instead, following a folder rename or move — so a role (Research/
    /// Trash) assigned to the moved folder or something inside it, or a trashed
    /// item's current-location bookkeeping, keeps pointing at where the thing
    /// actually is instead of a now-nonexistent path. `trashed_origins`' *values*
    /// (each item's pre-trash location) are deliberately left untouched: they're
    /// history, not a reference to something that just moved.
    pub(super) fn rewrite_relative_key_prefix(&mut self, old_prefix: &str, new_prefix: &str) {
        rewrite_prefix_in(&mut self.meta.node_order, old_prefix, new_prefix);
        rewrite_prefix_in(&mut self.meta.folder_roles, old_prefix, new_prefix);
        rewrite_prefix_in(&mut self.meta.trashed_origins, old_prefix, new_prefix);
    }
}
