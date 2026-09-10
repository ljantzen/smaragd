use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::model::{BinderNode, BinderTree};
use super::store::{ProjectStore, TreeEntryKind};

/// Scan `root` into a `BinderTree`. Only directories and `.md` files are included.
/// Hidden entries (dotfiles, including our own `.smaragd/` metadata dir) and anything
/// matched by a `.gitignore`/`.ignore` file are skipped — see `store`'s
/// `ProjectStore::list_tree` (which does the actual walking) for how, and what a
/// non-native implementation would need to reproduce.
pub fn scan_project(store: &dyn ProjectStore, root: &Path) -> BinderTree {
    let root = root.to_path_buf();

    let mut kinds: HashMap<PathBuf, TreeEntryKind> = HashMap::new();
    let mut children_of: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    kinds.insert(root.clone(), TreeEntryKind::Dir);

    for (path, kind) in store.list_tree(&root) {
        kinds.insert(path.clone(), kind);
        if let Some(parent) = path.parent() {
            children_of
                .entry(parent.to_path_buf())
                .or_default()
                .push(path);
        }
    }

    BinderTree {
        root: build_node(&root, &root, &kinds, &children_of),
    }
}

fn build_node(
    root: &Path,
    path: &Path,
    kinds: &HashMap<PathBuf, TreeEntryKind>,
    children_of: &HashMap<PathBuf, Vec<PathBuf>>,
) -> BinderNode {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(if path == root { "project" } else { "" })
        .to_string();

    match kinds.get(path) {
        Some(TreeEntryKind::Doc) => BinderNode::new_document(name, path.to_path_buf()),
        _ => {
            let mut child_paths = children_of.get(path).cloned().unwrap_or_default();
            child_paths.sort();
            let children = child_paths
                .iter()
                .map(|child_path| build_node(root, child_path, kinds, children_of))
                .collect();
            BinderNode::new_folder(name, path.to_path_buf(), children)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::model::BinderNodeKind;
    use std::fs;

    fn names(node: &BinderNode) -> Vec<String> {
        let mut names: Vec<String> = node.children().iter().map(|c| c.name.clone()).collect();
        names.sort();
        names
    }

    #[test]
    fn scans_nested_folders_and_documents() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Chapter 1")).unwrap();
        fs::write(dir.path().join("Chapter 1/01-opening.md"), "").unwrap();
        fs::write(dir.path().join("Chapter 1/02-arrival.md"), "").unwrap();
        fs::create_dir_all(dir.path().join("Chapter 2")).unwrap();
        fs::write(dir.path().join("Chapter 2/03-conflict.md"), "").unwrap();
        fs::write(dir.path().join("notes.md"), "").unwrap();

        let tree = scan_project(&crate::project::store::NativeStore, dir.path());

        assert_eq!(
            names(&tree.root),
            vec!["Chapter 1", "Chapter 2", "notes.md"]
        );

        let chapter1 = tree
            .find_by_path(&dir.path().join("Chapter 1"))
            .expect("Chapter 1 present");
        assert_eq!(names(chapter1), vec!["01-opening.md", "02-arrival.md"]);
    }

    #[test]
    fn excludes_gitignored_files_including_nested_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "draft-notes.md\n").unwrap();
        fs::write(dir.path().join("draft-notes.md"), "").unwrap();
        fs::write(dir.path().join("keep.md"), "").unwrap();

        fs::create_dir_all(dir.path().join("Chapter 1")).unwrap();
        fs::write(dir.path().join("Chapter 1/.gitignore"), "scratch.md\n").unwrap();
        fs::write(dir.path().join("Chapter 1/scratch.md"), "").unwrap();
        fs::write(dir.path().join("Chapter 1/scene.md"), "").unwrap();

        let tree = scan_project(&crate::project::store::NativeStore, dir.path());

        assert_eq!(names(&tree.root), vec!["Chapter 1", "keep.md"]);
        let chapter1 = tree
            .find_by_path(&dir.path().join("Chapter 1"))
            .expect("Chapter 1 present");
        assert_eq!(names(chapter1), vec!["scene.md"]);
    }

    #[test]
    fn excludes_non_markdown_files_without_crashing() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("cover.png"), b"not markdown").unwrap();
        fs::write(dir.path().join("todo.txt"), "not markdown either").unwrap();
        fs::write(dir.path().join("chapter.md"), "").unwrap();

        let tree = scan_project(&crate::project::store::NativeStore, dir.path());

        assert_eq!(names(&tree.root), vec!["chapter.md"]);
    }

    #[test]
    fn empty_folder_scans_to_empty_tree() {
        let dir = tempfile::tempdir().unwrap();

        let tree = scan_project(&crate::project::store::NativeStore, dir.path());

        assert!(
            matches!(tree.root.kind, BinderNodeKind::Folder { ref children } if children.is_empty())
        );
    }

    #[test]
    fn folder_with_only_ignored_content_scans_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "*.md\n").unwrap();
        fs::write(dir.path().join("secret-draft.md"), "").unwrap();

        let tree = scan_project(&crate::project::store::NativeStore, dir.path());

        assert!(
            matches!(tree.root.kind, BinderNodeKind::Folder { ref children } if children.is_empty())
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_symlink_to_a_file_outside_the_project_is_excluded_even_with_an_md_extension() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.txt");
        fs::write(&secret, "not part of this project").unwrap();
        std::os::unix::fs::symlink(&secret, dir.path().join("Notes.md")).unwrap();
        fs::write(dir.path().join("real.md"), "").unwrap();

        let tree = scan_project(&crate::project::store::NativeStore, dir.path());

        assert_eq!(names(&tree.root), vec!["real.md"]);
    }

    #[test]
    fn hidden_metadata_directory_is_excluded() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".smaragd")).unwrap();
        fs::write(dir.path().join(".smaragd/project.json"), "{}").unwrap();
        fs::write(dir.path().join("chapter.md"), "").unwrap();

        let tree = scan_project(&crate::project::store::NativeStore, dir.path());

        assert_eq!(names(&tree.root), vec!["chapter.md"]);
    }
}
