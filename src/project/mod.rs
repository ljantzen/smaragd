pub mod model;
mod scan;

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use model::{BinderNode, BinderNodeKind, BinderTree};
use scan::scan_project;

const METADATA_DIR: &str = ".tachylite";
const METADATA_FILE: &str = "project.json";

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
    /// Load a project from `root`. Missing or corrupt `.tachylite/project.json` is not
    /// an error — a plain folder of markdown files that's never been opened by
    /// tachylite before must always be openable, falling back to alphabetical order.
    pub fn load_from_folder(root: &Path) -> io::Result<Project> {
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} is not a directory", root.display()),
            ));
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
        fs::write(&path, "")?;
        self.record_new_child(parent, &filename)?;
        self.rescan();
        Ok(path)
    }

    /// Create a new empty folder under `parent`, record it, and rescan.
    pub fn create_folder(&mut self, parent: &Path, name: &str) -> io::Result<PathBuf> {
        let path = parent.join(name);
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
    fn load_from_folder_falls_back_gracefully_without_metadata() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.md"), "hello").unwrap();

        let project = Project::load_from_folder(dir.path()).unwrap();

        assert_eq!(project.meta, ProjectMeta::default());
        assert!(
            project
                .tree
                .find_by_path(&dir.path().join("notes.md"))
                .is_some()
        );
    }

    #[test]
    fn load_from_folder_errors_on_nonexistent_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");

        assert!(Project::load_from_folder(&missing).is_err());
    }

    #[test]
    fn create_document_writes_file_appends_order_and_appears_in_tree() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::load_from_folder(dir.path()).unwrap();

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
        let mut project = Project::load_from_folder(dir.path()).unwrap();

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
}
