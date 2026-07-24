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
                write!(f, "{} has not been set up as a tachylite project", path.display())
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

/// Manual ordering and (in future milestones) per-node metadata that the filesystem
/// itself can't express. Keyed by a `/`-separated path relative to the project root
/// ("" for the root folder itself) rather than `PathBuf`, so the file stays portable
/// across platforms and serializes to plain JSON without ambiguity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectMeta {
    pub version: u32,
    pub node_order: HashMap<String, Vec<String>>,
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

    /// Delete the file or folder at `path` (a folder is removed recursively), drop it
    /// (and, for a folder, its descendants) from manual ordering, and rescan.
    pub fn delete(&mut self, path: &Path) -> io::Result<()> {
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
        if is_dir {
            let prefix = relative_key(&self.root, path);
            self.meta
                .node_order
                .retain(|key, _| *key != prefix && !key.starts_with(&format!("{prefix}/")));
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
