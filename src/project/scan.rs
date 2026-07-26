use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use super::model::{BinderNode, BinderTree};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Dir,
    Doc,
}

/// Scan `root` into a `BinderTree`. Only directories and `.md` files are included.
/// Hidden entries (dotfiles, including our own `.tachylite/` metadata dir) and anything
/// matched by a `.gitignore`/`.ignore` file are skipped.
///
/// `require_git(false)` is set deliberately: the `ignore` crate only honors `.gitignore`
/// files by default when the scanned folder is inside an actual `.git` repository. A
/// project folder won't always be one (and isn't required to be), so gitignore rules
/// must apply regardless.
pub fn scan_project(root: &Path) -> BinderTree {
    let root = root.to_path_buf();

    let mut kinds: HashMap<PathBuf, EntryKind> = HashMap::new();
    let mut children_of: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    kinds.insert(root.clone(), EntryKind::Dir);

    let walker = WalkBuilder::new(&root).require_git(false).build();
    for entry in walker {
        let Ok(entry) = entry else { continue };
        let path = entry.path().to_path_buf();
        if path == root {
            continue;
        }

        // `entry.file_type()` reports the entry's own type (a symlink is neither a
        // dir nor a file here, since the walker isn't following links — see
        // `require_git`'s doc comment above for why `follow_links` is left at its
        // default `false`). Requiring `is_file()`, not just a `.md` extension, keeps
        // a symlink named e.g. `Notes.md` out of the binder entirely: `EditorState`'s
        // plain `fs::read_to_string`/`fs::write` would otherwise transparently follow
        // it, silently reading or overwriting whatever it points at (e.g. a synced
        // project pulled from a collaborator's git remote could plant a symlink to a
        // file outside the project).
        let file_type = entry.file_type();
        let is_dir = file_type.is_some_and(|ft| ft.is_dir());
        if is_dir {
            kinds.insert(path.clone(), EntryKind::Dir);
        } else if file_type.is_some_and(|ft| ft.is_file())
            && path.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            kinds.insert(path.clone(), EntryKind::Doc);
        } else {
            continue;
        }

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
    kinds: &HashMap<PathBuf, EntryKind>,
    children_of: &HashMap<PathBuf, Vec<PathBuf>>,
) -> BinderNode {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(if path == root { "project" } else { "" })
        .to_string();

    match kinds.get(path) {
        Some(EntryKind::Doc) => BinderNode::new_document(name, path.to_path_buf()),
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

        let tree = scan_project(dir.path());

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

        let tree = scan_project(dir.path());

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

        let tree = scan_project(dir.path());

        assert_eq!(names(&tree.root), vec!["chapter.md"]);
    }

    #[test]
    fn empty_folder_scans_to_empty_tree() {
        let dir = tempfile::tempdir().unwrap();

        let tree = scan_project(dir.path());

        assert!(
            matches!(tree.root.kind, BinderNodeKind::Folder { ref children } if children.is_empty())
        );
    }

    #[test]
    fn folder_with_only_ignored_content_scans_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "*.md\n").unwrap();
        fs::write(dir.path().join("secret-draft.md"), "").unwrap();

        let tree = scan_project(dir.path());

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

        let tree = scan_project(dir.path());

        assert_eq!(names(&tree.root), vec!["real.md"]);
    }

    #[test]
    fn hidden_metadata_directory_is_excluded() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".tachylite")).unwrap();
        fs::write(dir.path().join(".tachylite/project.json"), "{}").unwrap();
        fs::write(dir.path().join("chapter.md"), "").unwrap();

        let tree = scan_project(dir.path());

        assert_eq!(names(&tree.root), vec!["chapter.md"]);
    }
}
