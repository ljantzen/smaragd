pub mod model;
mod scan;

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use model::{BinderNode, BinderNodeKind, BinderTree};
use scan::scan_project;

const METADATA_DIR: &str = ".tachylite";
const METADATA_FILE: &str = "project.json";

/// Failure to load a project from a folder.
#[derive(Debug)]
pub enum LoadError {
    /// `root` doesn't exist or isn't a directory.
    NotADirectory(PathBuf),
    /// `root` is a plain folder tachylite has never opened before — no
    /// `.tachylite/project.json` marker is present. Distinguished from other IO
    /// failures so callers can offer to adopt/initialize the folder instead of just
    /// reporting failure.
    NotInitialized(PathBuf),
    /// Any other IO failure while creating the marker or scanning the project.
    Io(io::Error),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NotADirectory(path) => write!(f, "{} is not a directory", path.display()),
            LoadError::NotInitialized(path) => {
                write!(
                    f,
                    "{} has not been set up as a tachylite project",
                    path.display()
                )
            }
            LoadError::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<io::Error> for LoadError {
    fn from(err: io::Error) -> Self {
        LoadError::Io(err)
    }
}

/// Failure to restore an item from Trash.
#[derive(Debug)]
pub enum RestoreError {
    /// `path` has no recorded origin — it isn't a top-level trashed item.
    NotTrashed,
    /// The folder this item originally lived in no longer exists.
    OriginalFolderMissing(PathBuf),
    Io(io::Error),
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RestoreError::NotTrashed => write!(f, "not a trashed item"),
            RestoreError::OriginalFolderMissing(path) => {
                write!(
                    f,
                    "original location \"{}\" no longer exists",
                    path.display()
                )
            }
            RestoreError::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for RestoreError {}

impl From<io::Error> for RestoreError {
    fn from(err: io::Error) -> Self {
        RestoreError::Io(err)
    }
}

/// A Scrivener-Research/Trash-style role assigned to a folder, decoupled from its
/// position in the tree. At most one folder project-wide holds a given role at a
/// time. `Research` is currently just a marker — a forward-looking extension point
/// for features (Compile, word-count rollups) that don't exist yet. `Trash` has a
/// real behavior change: see [`Project::delete`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FolderRole {
    Research,
    Trash,
}

/// Manual ordering and per-node metadata that the filesystem itself can't express.
/// Keyed by a `/`-separated path relative to the project root ("" for the root folder
/// itself) rather than `PathBuf`, so the file stays portable across platforms and
/// serializes to plain JSON without ambiguity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectMeta {
    pub version: u32,
    pub node_order: HashMap<String, Vec<String>>,
    /// `#[serde(default)]` is required, not cosmetic: project.json files written
    /// before this field existed have no `folder_roles`/`trashed_origins` keys at
    /// all — without a default, deserializing them would fail outright and silently
    /// discard their real, already-persisted `node_order` data.
    #[serde(default)]
    pub folder_roles: HashMap<String, FolderRole>,
    /// A trashed item's *current* relative key (its path inside the Trash folder,
    /// post-move) → its *original* relative key (where it lived pre-delete).
    /// Disambiguates same-named items trashed from different folders and is what a
    /// future "restore from trash" action needs to put something back where it came
    /// from — the on-disk name alone (deduplicated with a " (2)" suffix on collision)
    /// doesn't carry that.
    #[serde(default)]
    pub trashed_origins: HashMap<String, String>,
}

pub struct Project {
    pub root: PathBuf,
    pub tree: BinderTree,
    pub meta: ProjectMeta,
}

impl Project {
    /// Load a project from `root`. `root` must already be a tachylite project (i.e.
    /// have a `.tachylite/project.json` marker) — use [`Project::initialize`] to
    /// create one first. A *corrupt* (as opposed to absent) marker file is still not
    /// an error, falling back to default metadata.
    pub fn load_from_folder(root: &Path) -> Result<Project, LoadError> {
        if !root.is_dir() {
            return Err(LoadError::NotADirectory(root.to_path_buf()));
        }
        if !metadata_path(root).is_file() {
            return Err(LoadError::NotInitialized(root.to_path_buf()));
        }

        let meta = load_metadata(root).unwrap_or_default();
        let mut tree = scan_project(root);
        apply_order(&mut tree.root, root, &meta.node_order);

        Ok(Project {
            root: root.to_path_buf(),
            tree,
            meta,
        })
    }

    /// Ensure `root` exists and is marked as a tachylite project — creating it and/or
    /// writing a fresh `.tachylite/project.json` with default metadata if one isn't
    /// already there — then load it. Never overwrites existing metadata, so calling
    /// this on an already-initialized project is a no-op beyond a normal
    /// `load_from_folder`. Backs both "New Project" (a path that doesn't exist yet)
    /// and adopting an existing folder of markdown files as a project.
    pub fn initialize(root: &Path) -> Result<Project, LoadError> {
        fs::create_dir_all(root)?;
        if !metadata_path(root).is_file() {
            save_metadata(root, &ProjectMeta::default())?;
        }
        Self::load_from_folder(root)
    }

    /// Read and parse the YAML frontmatter of the document at `path`. On-demand
    /// only — nothing in `BinderNode`/`ProjectMeta` carries per-document metadata, so
    /// call this only when a specific document's metadata is actually needed.
    pub fn document_meta(&self, path: &Path) -> io::Result<crate::frontmatter::DocumentMeta> {
        let contents = fs::read_to_string(path)?;
        Ok(crate::frontmatter::parse(&contents))
    }

    /// The role assigned to the folder at `path`, if any.
    pub fn folder_role(&self, path: &Path) -> Option<FolderRole> {
        self.meta
            .folder_roles
            .get(&relative_key(&self.root, path))
            .copied()
    }

    /// Assign `role` to the folder at `path` (`None` clears it). At most one folder
    /// project-wide holds a given role — assigning it here clears it from wherever it
    /// was previously assigned, mirroring Scrivener's singular Draft/Trash. Errors if
    /// `path` isn't a directory.
    pub fn set_folder_role(&mut self, path: &Path, role: Option<FolderRole>) -> io::Result<()> {
        if !path.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a folder"));
        }
        let key = relative_key(&self.root, path);
        match role {
            Some(role) => {
                self.meta.folder_roles.retain(|_, r| *r != role);
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

    /// The absolute path of the project's designated Trash folder, if any.
    fn trash_path(&self) -> Option<PathBuf> {
        self.meta
            .folder_roles
            .iter()
            .find(|(_, role)| **role == FolderRole::Trash)
            .map(|(key, _)| {
                if key.is_empty() {
                    self.root.clone()
                } else {
                    self.root.join(key)
                }
            })
    }

    /// Whether deleting `path` right now would route it into Trash rather than
    /// permanently removing it — exposed so callers can word a delete confirmation
    /// accurately ("Move to Trash?" vs "This cannot be undone.").
    pub fn deletes_to_trash(&self, path: &Path) -> bool {
        self.trash_path()
            .is_some_and(|trash| path != trash && !path.starts_with(&trash))
    }

    pub fn rescan(&mut self) {
        let mut tree = scan_project(&self.root);
        apply_order(&mut tree.root, &self.root, &self.meta.node_order);
        self.tree = tree;
    }

    pub fn save_metadata(&self) -> io::Result<()> {
        save_metadata(&self.root, &self.meta)
    }

    /// Create a new empty markdown document under `parent` (a folder within this
    /// project), record it at the end of that folder's manual order, and rescan.
    pub fn create_document(&mut self, parent: &Path, filename: &str) -> io::Result<PathBuf> {
        let filename = ensure_md_extension(filename);
        let path = parent.join(&filename);
        ensure_does_not_exist(&path)?;
        fs::write(&path, "")?;
        self.record_new_child(parent, &filename)?;
        self.rescan();
        Ok(path)
    }

    /// Create a new empty folder under `parent`, record it, and rescan. Refuses to
    /// overwrite an existing file or folder at the destination.
    pub fn create_folder(&mut self, parent: &Path, name: &str) -> io::Result<PathBuf> {
        let path = parent.join(name);
        ensure_does_not_exist(&path)?;
        fs::create_dir_all(&path)?;
        self.record_new_child(parent, name)?;
        self.rescan();
        Ok(path)
    }

    fn record_new_child(&mut self, parent: &Path, name: &str) -> io::Result<()> {
        let key = relative_key(&self.root, parent);
        self.meta
            .node_order
            .entry(key)
            .or_default()
            .push(name.to_string());
        self.save_metadata()
    }

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
            self.rewrite_order_key_prefix(
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

    /// Move `path` (already inside this project) to be a child of `new_parent`,
    /// keeping its own name unless that collides (resolved via `unique_child_name`
    /// since two different folders can easily contain same-named files), and fixing
    /// up `node_order` — and, for a folder, nested order keys via
    /// `rewrite_order_key_prefix` — to follow. Does not touch `folder_roles` /
    /// `trashed_origins` or persist/rescan; callers own that, since what else needs
    /// updating differs between trashing and restoring.
    fn move_node(&mut self, path: &Path, new_parent: &Path) -> io::Result<PathBuf> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
        let dest_name = unique_child_name(new_parent, name);
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
            self.rewrite_order_key_prefix(
                &relative_key(&self.root, path),
                &relative_key(&self.root, &dest),
            );
        }
        let new_parent_key = relative_key(&self.root, new_parent);
        self.meta
            .node_order
            .entry(new_parent_key)
            .or_default()
            .push(dest_name);
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
        let dest = self.move_node(path, &original_parent)?;
        self.meta.trashed_origins.remove(&key);
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
    fn permanently_delete(&mut self, path: &Path) -> io::Result<()> {
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

    /// Rewrite every `node_order` key under `old_prefix` (the folder itself and all
    /// its descendants) to sit under `new_prefix` instead, following a folder rename.
    fn rewrite_order_key_prefix(&mut self, old_prefix: &str, new_prefix: &str) {
        let affected: Vec<String> = self
            .meta
            .node_order
            .keys()
            .filter(|key| *key == old_prefix || key.starts_with(&format!("{old_prefix}/")))
            .cloned()
            .collect();
        for key in affected {
            if let Some(order) = self.meta.node_order.remove(&key) {
                let new_key = format!("{new_prefix}{}", &key[old_prefix.len()..]);
                self.meta.node_order.insert(new_key, order);
            }
        }
    }
}

/// Guard against silently clobbering an existing file or folder — used before any
/// operation (create, rename) that's about to write to a new destination path.
fn ensure_does_not_exist(path: &Path) -> io::Result<()> {
    if path.exists() {
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", path.display()),
        ))
    } else {
        Ok(())
    }
}

fn ensure_md_extension(filename: &str) -> String {
    if filename.ends_with(".md") {
        filename.to_string()
    } else {
        format!("{filename}.md")
    }
}

/// Resolve a name collision under `parent` by appending " (2)", " (3)", ... before the
/// extension (or at the end, for an extension-less name/a folder) until it no longer
/// exists on disk. Returns `desired` unchanged if it's already free.
fn unique_child_name(parent: &Path, desired: &str) -> String {
    if !parent.join(desired).exists() {
        return desired.to_string();
    }
    let (stem, ext) = match desired.rsplit_once('.') {
        Some((stem, ext)) => (stem, format!(".{ext}")),
        None => (desired, String::new()),
    };
    for n in 2.. {
        let candidate = format!("{stem} ({n}){ext}");
        if !parent.join(&candidate).exists() {
            return candidate;
        }
    }
    unreachable!()
}

/// Convert an absolute path within the project into the `/`-separated relative key
/// used by `ProjectMeta::node_order`. The project root itself maps to `""`.
fn relative_key(root: &Path, path: &Path) -> String {
    if path == root {
        return String::new();
    }
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Reorder each folder's children to match any recorded order for that folder,
/// appending children with no recorded position (e.g. newly discovered files never
/// created through tachylite) at the end in their existing (alphabetical) order.
fn apply_order(node: &mut BinderNode, root: &Path, order: &HashMap<String, Vec<String>>) {
    if let BinderNodeKind::Folder { children } = &mut node.kind {
        if let Some(order_list) = order.get(&relative_key(root, &node.path)) {
            children.sort_by_key(|child| {
                order_list
                    .iter()
                    .position(|name| name == &child.name)
                    .unwrap_or(usize::MAX)
            });
        }
        for child in children.iter_mut() {
            apply_order(child, root, order);
        }
    }
}

fn metadata_path(root: &Path) -> PathBuf {
    root.join(METADATA_DIR).join(METADATA_FILE)
}

fn load_metadata(root: &Path) -> Option<ProjectMeta> {
    let contents = fs::read_to_string(metadata_path(root)).ok()?;
    serde_json::from_str(&contents).ok()
}

fn save_metadata(root: &Path, meta: &ProjectMeta) -> io::Result<()> {
    let dir = root.join(METADATA_DIR);
    fs::create_dir_all(&dir)?;
    let contents = serde_json::to_string_pretty(meta)?;
    fs::write(metadata_path(root), contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut node_order = HashMap::new();
        node_order.insert(
            "Chapter 1".to_string(),
            vec!["01-opening.md".to_string(), "02-arrival.md".to_string()],
        );
        let meta = ProjectMeta {
            version: 1,
            node_order,
            ..Default::default()
        };

        save_metadata(dir.path(), &meta).unwrap();
        let loaded = load_metadata(dir.path()).unwrap();

        assert_eq!(loaded, meta);
    }

    #[test]
    fn missing_metadata_file_falls_back_to_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_metadata(dir.path()).is_none());
    }

    #[test]
    fn corrupt_metadata_file_falls_back_to_none_instead_of_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let meta_dir = dir.path().join(METADATA_DIR);
        fs::create_dir_all(&meta_dir).unwrap();
        fs::write(meta_dir.join(METADATA_FILE), "{ this is not valid json").unwrap();

        assert!(load_metadata(dir.path()).is_none());
    }

    #[test]
    fn load_from_folder_errors_when_project_has_no_marker() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.md"), "hello").unwrap();

        let result = Project::load_from_folder(dir.path());

        assert!(matches!(result, Err(LoadError::NotInitialized(_))));
    }

    #[test]
    fn load_from_folder_errors_on_nonexistent_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");

        assert!(matches!(
            Project::load_from_folder(&missing),
            Err(LoadError::NotADirectory(_))
        ));
    }

    #[test]
    fn initialize_lets_a_pre_existing_folder_of_markdown_files_load_with_default_metadata() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.md"), "hello").unwrap();

        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(project.meta, ProjectMeta::default());
        assert!(
            project
                .tree
                .find_by_path(&dir.path().join("notes.md"))
                .is_some()
        );
    }

    #[test]
    fn initialize_creates_the_folder_and_marker_for_a_brand_new_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("New Project");

        let project = Project::initialize(&root).unwrap();

        assert!(root.is_dir());
        assert!(root.join(METADATA_DIR).join(METADATA_FILE).is_file());
        assert!(project.tree.root.children().is_empty());
    }

    #[test]
    fn initialize_does_not_clobber_existing_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let mut node_order = HashMap::new();
        node_order.insert("".to_string(), vec!["custom.md".to_string()]);
        let meta = ProjectMeta {
            version: 7,
            node_order,
            ..Default::default()
        };
        save_metadata(dir.path(), &meta).unwrap();

        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(project.meta, meta);
    }

    #[test]
    fn document_meta_reads_frontmatter_from_a_project_document() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Scene 1").unwrap();
        fs::write(&path, "---\ntype: Scene\npov: Alice\n---\nBody.\n").unwrap();

        let meta = project.document_meta(&path).unwrap();

        assert_eq!(meta.section_type.as_deref(), Some("Scene"));
        assert_eq!(meta.pov.as_deref(), Some("Alice"));
    }

    #[test]
    fn document_meta_returns_default_for_a_document_with_no_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Plain").unwrap();

        let meta = project.document_meta(&path).unwrap();

        assert_eq!(meta, crate::frontmatter::DocumentMeta::default());
    }

    #[test]
    fn document_meta_errors_if_the_path_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        let result = project.document_meta(&dir.path().join("missing.md"));

        assert!(result.is_err());
    }

    #[test]
    fn load_metadata_defaults_folder_roles_and_trashed_origins_when_absent_from_older_project_json()
    {
        let dir = tempfile::tempdir().unwrap();
        let meta_dir = dir.path().join(METADATA_DIR);
        fs::create_dir_all(&meta_dir).unwrap();
        fs::write(
            meta_dir.join(METADATA_FILE),
            r#"{"version":1,"node_order":{"":["a.md"]}}"#,
        )
        .unwrap();

        let meta = load_metadata(dir.path()).unwrap();

        assert_eq!(meta.node_order.get(""), Some(&vec!["a.md".to_string()]));
        assert!(meta.folder_roles.is_empty());
        assert!(meta.trashed_origins.is_empty());
    }

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

    #[test]
    fn create_document_writes_file_appends_order_and_appears_in_tree() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        let path = project.create_document(dir.path(), "New Scene").unwrap();

        assert_eq!(path, dir.path().join("New Scene.md"));
        assert!(path.exists());
        assert!(project.tree.find_by_path(&path).is_some());
        assert_eq!(
            project.meta.node_order.get(""),
            Some(&vec!["New Scene.md".to_string()])
        );

        // Order persisted to disk, not just in memory.
        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(
            reloaded.meta.node_order.get(""),
            Some(&vec!["New Scene.md".to_string()])
        );
    }

    #[test]
    fn create_folder_creates_directory_and_appears_in_tree() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        let path = project.create_folder(dir.path(), "Chapter 3").unwrap();

        assert!(path.is_dir());
        let node = project.tree.find_by_path(&path).expect("folder present");
        assert!(matches!(node.kind, BinderNodeKind::Folder { .. }));
    }

    #[test]
    fn recorded_order_is_applied_ahead_of_unlisted_siblings() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.md"), "").unwrap();
        fs::write(dir.path().join("a.md"), "").unwrap();

        let mut node_order = HashMap::new();
        node_order.insert("".to_string(), vec!["b.md".to_string()]);
        save_metadata(
            dir.path(),
            &ProjectMeta {
                version: 1,
                node_order,
                ..Default::default()
            },
        )
        .unwrap();

        let project = Project::load_from_folder(dir.path()).unwrap();
        let names: Vec<&str> = project
            .tree
            .root
            .children()
            .iter()
            .map(|c| c.name.as_str())
            .collect();

        assert_eq!(names, vec!["b.md", "a.md"]);
    }

    #[test]
    fn rename_document_renames_file_and_updates_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Old Name").unwrap();

        let new_path = project.rename(&path, "New Name").unwrap();

        assert_eq!(new_path, dir.path().join("New Name.md"));
        assert!(!path.exists());
        assert!(new_path.exists());
        assert!(project.tree.find_by_path(&new_path).is_some());
        assert_eq!(
            project.meta.node_order.get(""),
            Some(&vec!["New Name.md".to_string()])
        );
    }

    #[test]
    fn rename_document_updates_wikilinks_pointing_at_it_in_other_documents() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Old Name").unwrap();
        let referrer = project.create_document(dir.path(), "Referrer").unwrap();
        fs::write(
            &referrer,
            "See [[Old Name]] and [[Old Name|the other note]].",
        )
        .unwrap();

        project.rename(&target, "New Name").unwrap();

        let updated = fs::read_to_string(&referrer).unwrap();
        assert_eq!(updated, "See [[New Name]] and [[New Name|the other note]].");
    }

    #[test]
    fn rename_document_leaves_unrelated_wikilinks_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Old Name").unwrap();
        let referrer = project.create_document(dir.path(), "Referrer").unwrap();
        fs::write(&referrer, "See [[Something Else]].").unwrap();

        project.rename(&target, "New Name").unwrap();

        assert_eq!(
            fs::read_to_string(&referrer).unwrap(),
            "See [[Something Else]]."
        );
    }

    #[test]
    fn rename_folder_updates_nested_order_keys() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Old Chapter").unwrap();
        project.create_document(&folder, "Scene 1").unwrap();

        let new_folder = project.rename(&folder, "New Chapter").unwrap();

        assert!(new_folder.is_dir());
        assert!(new_folder.join("Scene 1.md").exists());
        assert_eq!(
            project.meta.node_order.get("New Chapter"),
            Some(&vec!["Scene 1.md".to_string()])
        );
        assert!(!project.meta.node_order.contains_key("Old Chapter"));
    }

    #[test]
    fn rename_refuses_to_overwrite_an_existing_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "A").unwrap();
        project.create_document(dir.path(), "B").unwrap();

        let result = project.rename(&path, "B");

        assert!(result.is_err());
        assert!(path.exists());
    }

    #[test]
    fn create_document_refuses_to_overwrite_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Existing").unwrap();
        fs::write(&path, "original content").unwrap();

        let result = project.create_document(dir.path(), "Existing");

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "original content");
    }

    #[test]
    fn create_folder_refuses_to_overwrite_an_existing_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.create_folder(dir.path(), "Existing").unwrap();

        let result = project.create_folder(dir.path(), "Existing");

        assert!(result.is_err());
    }

    #[test]
    fn delete_document_removes_file_and_order_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Doomed").unwrap();

        project.delete(&path).unwrap();

        assert!(!path.exists());
        assert!(project.tree.find_by_path(&path).is_none());
        assert_eq!(project.meta.node_order.get(""), Some(&vec![]));
    }

    #[test]
    fn delete_folder_removes_directory_and_nested_order_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();
        project.create_document(&folder, "Scene 1").unwrap();

        project.delete(&folder).unwrap();

        assert!(!folder.exists());
        assert!(project.tree.find_by_path(&folder).is_none());
        assert!(!project.meta.node_order.contains_key("Chapter 1"));
    }
}
