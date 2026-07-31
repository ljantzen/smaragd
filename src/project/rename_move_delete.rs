use super::*;

/// How `move_node_with` should resolve a name collision in the destination folder —
/// see `Project::move_node`/`move_item`, its two callers, for when each applies.
enum NameCollision {
    Uniquify,
    Refuse,
}

impl Project {
    /// Rename the file or folder at `path` to `new_name` (a document keeps its `.md`
    /// extension; a folder is renamed as-is), updating manual ordering and rescanning.
    /// Refuses to overwrite an existing file or folder at the destination.
    pub fn rename(&mut self, path: &Path, new_name: &str) -> io::Result<PathBuf> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
        let is_dir = path.is_dir();
        let new_name = if is_dir {
            new_name.to_string()
        } else {
            ensure_md_extension(new_name)
        };
        ensure_simple_child_name(&new_name)?;
        let new_path = parent.join(&new_name);
        ensure_does_not_exist(&new_path)?;

        fs::rename(path, &new_path)?;

        if let Some(old_name) = path.file_name().and_then(|n| n.to_str()) {
            let parent_key = relative_key(&self.root, parent);
            if let Some(order) = self.meta.node_order.get_mut(&parent_key) {
                for entry in order.iter_mut() {
                    if entry == old_name {
                        *entry = new_name.clone();
                    }
                }
            }
        }
        if is_dir {
            self.rewrite_relative_key_prefix(
                &relative_key(&self.root, path),
                &relative_key(&self.root, &new_path),
            );
        }

        self.save_metadata()?;
        self.rescan();

        // A folder rename can't affect wikilinks — they resolve by document filename,
        // not folder name — but a document rename might be the target of links
        // elsewhere in the project, so those need to follow it to the new name.
        if !is_dir
            && let (Some(old_stem), Some(new_stem)) = (
                path.file_stem().and_then(|s| s.to_str()),
                new_path.file_stem().and_then(|s| s.to_str()),
            )
        {
            self.rename_wikilinks_everywhere(old_stem, new_stem)?;
            self.relink_story_cards(old_stem, new_stem)?;
        }

        Ok(new_path)
    }

    /// Rewrite `[[old_target]]` / `[[old_target|Alias]]` wikilinks to `new_target` in
    /// every document in the project (including, if applicable, the renamed document
    /// itself, in case it links to itself).
    fn rename_wikilinks_everywhere(&self, old_target: &str, new_target: &str) -> io::Result<()> {
        for doc_path in self.tree.document_paths() {
            let contents = fs::read_to_string(&doc_path)?;
            if let Some(updated) =
                crate::markdown::rename_wikilink_target(&contents, old_target, new_target)
            {
                fs::write(&doc_path, updated)?;
            }
        }
        Ok(())
    }

    /// Follow a document rename in any story card linked to it by its old stem — the
    /// same "keep soft references working" job `rename_wikilinks_everywhere` does for
    /// `[[wikilinks]]`, just for `StoryCard::linked_document_stem`. A no-op (and no
    /// extra write) if no card was linked to `old_stem`.
    fn relink_story_cards(&mut self, old_stem: &str, new_stem: &str) -> io::Result<()> {
        let mut changed = false;
        for card in self.meta.story_cards.iter_mut() {
            if card.linked_document_stem.as_deref() == Some(old_stem) {
                card.linked_document_stem = Some(new_stem.to_string());
                changed = true;
            }
        }
        if changed {
            self.save_metadata()
        } else {
            Ok(())
        }
    }

    /// Delete `path`. If a Trash folder is designated and `path` isn't already inside
    /// it (and isn't the Trash folder itself), moves it into Trash instead of
    /// removing it from disk — see [`Project::move_to_trash`]. Otherwise permanently
    /// removes it, as if there were no Trash configured.
    pub fn delete(&mut self, path: &Path) -> io::Result<()> {
        if self.deletes_to_trash(path) {
            let trash = self.trash_path().expect("checked by deletes_to_trash");
            return self.move_to_trash(path, &trash);
        }
        self.permanently_delete(path)
    }

    /// Move the file or folder at `path` into `new_parent` — e.g. dragging and
    /// dropping it onto a different folder in the binder. Unlike `move_node` (used
    /// for trashing/restoring, where silently disambiguating a same-named collision
    /// is the right call), this *refuses* the move if `new_parent` already has an
    /// entry with that name: a drag-and-drop is a deliberate placement, so quietly
    /// renaming around a collision would be surprising. Also refuses moving a folder
    /// into itself or one of its own subfolders, which `fs::rename` would otherwise
    /// reject with a much less legible OS error.
    ///
    /// Dropping an item onto its *own* parent folder's header (as opposed to a
    /// sibling document row — see `move_item_before` for that) is handled as a
    /// special case: there's no actual filesystem move to make, just a
    /// reposition to the end of that folder's order, since `move_node_with`'s
    /// `NameCollision::Refuse` would otherwise reject this as a collision with
    /// the item's own still-existing path.
    pub fn move_item(&mut self, path: &Path, new_parent: &Path) -> io::Result<PathBuf> {
        if new_parent.starts_with(path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "can't move a folder into itself or one of its own subfolders",
            ));
        }
        let dest = if path.parent() == Some(new_parent) {
            let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "path has no file name")
            })?;
            self.reposition_in_order(new_parent, name, None);
            path.to_path_buf()
        } else {
            self.move_node_with(path, new_parent, NameCollision::Refuse)?
        };
        self.save_metadata()?;
        self.rescan();
        Ok(dest)
    }

    /// Move `path` (a file or folder) to sit immediately before `before` among
    /// `before`'s parent's children — used when something is dragged and dropped
    /// directly onto another document row in the binder (as opposed to a folder
    /// header, which always appends to the end — see `move_item`). Works whether
    /// `before` is currently a sibling of `path` (a pure reorder) or in a
    /// different folder (a move, positioned rather than appended). Refuses
    /// moving an item before itself, or a folder into itself/one of its own
    /// subfolders, same reasoning as `move_item`.
    pub fn move_item_before(&mut self, path: &Path, before: &Path) -> io::Result<PathBuf> {
        if path == before {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "can't move an item before itself",
            ));
        }
        let new_parent = before
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no parent"))?;
        if new_parent.starts_with(path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "can't move a folder into itself or one of its own subfolders",
            ));
        }

        let dest = if path.parent() == Some(new_parent) {
            path.to_path_buf()
        } else {
            self.move_node_with(path, new_parent, NameCollision::Refuse)?
        };

        let name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
        let before_name = before.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "target has no file name")
        })?;
        self.reposition_in_order(new_parent, name, Some(before_name));

        self.save_metadata()?;
        self.rescan();
        Ok(dest)
    }

    /// Remove `name` from `parent`'s child order and reinsert it immediately
    /// before `before` — or at the end, if `before` is `None` or not found.
    /// `before` is looked up *after* `name` is removed, so a `before` that
    /// originally sat right after `name` in the list doesn't shift by one and
    /// end up landing after it.
    ///
    /// Starts from `self.tree`'s current children for `parent` (still the
    /// *pre-move* tree at this point — callers run this before `rescan()`),
    /// not directly from `self.meta.node_order`'s entry: `node_order` is a
    /// sparse override list — anything never explicitly reordered has no entry
    /// at all, filled in by `apply_order`'s fallback (existing/alphabetical
    /// order, sorted last) rather than being absent from the *displayed* list.
    /// Computing the target index against that raw, possibly-incomplete list
    /// previously let an untracked sibling's true position throw the result
    /// off — e.g. dragging the last of 5 never-explicitly-reordered scenes
    /// onto its neighbor landed it at the very top of the chapter instead of
    /// where it was dropped, because the stored order only had one or two
    /// tracked entries and the target's position among *those* wasn't its
    /// real, currently-displayed position. `self.tree`'s children are always
    /// the complete, already-`apply_order`-resolved list, so this can't happen
    /// — and writing the full resulting list back (instead of just patching
    /// the old sparse one) also fixes `parent`'s entry going forward.
    fn reposition_in_order(&mut self, parent: &Path, name: &str, before: Option<&str>) {
        let mut order: Vec<String> = self
            .tree
            .find_by_path(parent)
            .map(|node| {
                node.children()
                    .iter()
                    .map(|child| child.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        order.retain(|entry| entry != name);
        let index = before
            .and_then(|before_name| order.iter().position(|entry| entry == before_name))
            .unwrap_or(order.len());
        order.insert(index, name.to_string());
        let parent_key = relative_key(&self.root, parent);
        self.meta.node_order.insert(parent_key, order);
    }

    /// Move `path` (already inside this project) to be a child of `new_parent`,
    /// keeping its own name unless that collides — resolved via `unique_child_name`,
    /// since two different folders can easily contain same-named files — and fixing
    /// up `node_order` — and, for a folder, nested `node_order`/`folder_roles`/
    /// `trashed_origins` keys via `rewrite_relative_key_prefix` — to follow. Does not
    /// persist/rescan; callers own that, since what else needs updating differs
    /// between trashing and restoring. `restore_from_trash` removes its own
    /// `trashed_origins` entry *before* calling this, precisely because this now
    /// touches that map too — see its comment for why the order matters.
    pub(super) fn move_node(&mut self, path: &Path, new_parent: &Path) -> io::Result<PathBuf> {
        self.move_node_with(path, new_parent, NameCollision::Uniquify)
    }

    /// Shared core of `move_node`/`move_item`: rename `path` into `new_parent` and
    /// fix up `node_order` (and, for a folder, nested order keys) to follow — the
    /// only thing that differs between the two public entry points is what happens
    /// when `new_parent` already has an entry with `path`'s name, which
    /// `on_collision` controls.
    fn move_node_with(
        &mut self,
        path: &Path,
        new_parent: &Path,
        on_collision: NameCollision,
    ) -> io::Result<PathBuf> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
        let dest_name = match on_collision {
            NameCollision::Uniquify => unique_child_name(new_parent, name),
            NameCollision::Refuse => {
                ensure_does_not_exist(&new_parent.join(name))?;
                name.to_string()
            }
        };
        let dest = new_parent.join(&dest_name);
        let is_dir = path.is_dir();

        fs::rename(path, &dest)?;

        if let Some(parent) = path.parent() {
            let parent_key = relative_key(&self.root, parent);
            if let Some(order) = self.meta.node_order.get_mut(&parent_key) {
                order.retain(|entry| entry != name);
            }
        }
        if is_dir {
            self.rewrite_relative_key_prefix(
                &relative_key(&self.root, path),
                &relative_key(&self.root, &dest),
            );
        }
        // Appends `dest_name` after every one of `new_parent`'s *current*
        // children — tracked or not — rather than just whatever was already in
        // its (possibly sparse) `node_order` entry; see `reposition_in_order`'s
        // doc comment for why that distinction matters.
        self.reposition_in_order(new_parent, &dest_name, None);
        Ok(dest)
    }

    /// Move `path` into the designated `trash` folder. Records the item's original
    /// relative path in `meta.trashed_origins`, keyed by its new (post-move) relative
    /// path — this is what disambiguates same-named trashed items and is what
    /// `restore_from_trash` needs, since the on-disk name alone doesn't carry it.
    fn move_to_trash(&mut self, path: &Path, trash: &Path) -> io::Result<()> {
        let original_key = relative_key(&self.root, path);
        let dest = self.move_node(path, trash)?;
        self.meta
            .trashed_origins
            .insert(relative_key(&self.root, &dest), original_key);

        self.save_metadata()?;
        self.rescan();
        Ok(())
    }
}
