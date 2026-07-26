use std::path::{Path, PathBuf};

/// A project's binder: the nested tree of folders and markdown documents, mirroring
/// the project's directory structure on disk.
#[derive(Debug, Clone)]
pub struct BinderTree {
    pub root: BinderNode,
}

#[derive(Debug, Clone)]
pub struct BinderNode {
    /// The on-disk filename, extension included for documents (`scene.md`) — matched
    /// against `ProjectMeta::node_order` entries and real filenames elsewhere, so it's
    /// not the place to hide the `.md` extension; `ui::binder_panel::document_label`
    /// does that at render time instead.
    pub name: String,
    pub path: PathBuf,
    pub kind: BinderNodeKind,
}

#[derive(Debug, Clone)]
pub enum BinderNodeKind {
    Folder { children: Vec<BinderNode> },
    Document,
}

impl BinderNode {
    pub fn new_folder(
        name: impl Into<String>,
        path: impl Into<PathBuf>,
        children: Vec<BinderNode>,
    ) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            kind: BinderNodeKind::Folder { children },
        }
    }

    pub fn new_document(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            kind: BinderNodeKind::Document,
        }
    }

    pub fn children(&self) -> &[BinderNode] {
        match &self.kind {
            BinderNodeKind::Folder { children } => children,
            BinderNodeKind::Document => &[],
        }
    }

    /// Find a node by its absolute path, searching this node and its descendants.
    pub fn find_by_path(&self, path: &Path) -> Option<&BinderNode> {
        if self.path == path {
            return Some(self);
        }
        self.children()
            .iter()
            .find_map(|child| child.find_by_path(path))
    }

    /// Find a document whose filename (without extension) matches `stem`,
    /// case-insensitively — used to resolve `[[wikilink]]` targets to a file. Compares
    /// full Unicode case folding (`to_lowercase`), not just ASCII, so titles with
    /// accented or non-Latin characters (e.g. "Café", "Straße") match regardless of
    /// case.
    pub fn find_document_by_stem(&self, stem: &str) -> Option<&BinderNode> {
        if matches!(self.kind, BinderNodeKind::Document)
            && self
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.to_lowercase() == stem.to_lowercase())
        {
            return Some(self);
        }
        self.children()
            .iter()
            .find_map(|child| child.find_document_by_stem(stem))
    }

    /// Collect the filename (without extension) of every document in this subtree, in
    /// tree order — the candidate list for `[[wikilink]]` autocomplete.
    pub fn document_names(&self, out: &mut Vec<String>) {
        if matches!(self.kind, BinderNodeKind::Document)
            && let Some(stem) = self.path.file_stem().and_then(|s| s.to_str())
        {
            out.push(stem.to_string());
        }
        for child in self.children() {
            child.document_names(out);
        }
    }

    /// Collect the absolute path of every document in this subtree, in tree order.
    pub fn document_paths(&self, out: &mut Vec<PathBuf>) {
        if matches!(self.kind, BinderNodeKind::Document) {
            out.push(self.path.clone());
        }
        for child in self.children() {
            child.document_paths(out);
        }
    }

    /// Insert `node` as a child of the folder at `parent_path`. Returns `true` if a
    /// matching folder was found and the node was inserted.
    pub fn insert_under(&mut self, parent_path: &Path, node: BinderNode) -> bool {
        if self.path == parent_path {
            return match &mut self.kind {
                BinderNodeKind::Folder { children } => {
                    children.push(node);
                    true
                }
                BinderNodeKind::Document => false,
            };
        }
        if let BinderNodeKind::Folder { children } = &mut self.kind {
            for child in children.iter_mut() {
                if child.insert_under(parent_path, node.clone()) {
                    return true;
                }
            }
        }
        false
    }
}

impl BinderTree {
    pub fn find_by_path(&self, path: &Path) -> Option<&BinderNode> {
        self.root.find_by_path(path)
    }

    pub fn insert_under(&mut self, parent_path: &Path, node: BinderNode) -> bool {
        self.root.insert_under(parent_path, node)
    }

    pub fn find_document_by_stem(&self, stem: &str) -> Option<&BinderNode> {
        self.root.find_document_by_stem(stem)
    }

    /// The filename (without extension) of every document in the project, in tree
    /// order — the candidate list for `[[wikilink]]` autocomplete.
    pub fn document_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.root.document_names(&mut names);
        names
    }

    /// The absolute path of every document in the project, in tree order.
    pub fn document_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        self.root.document_paths(&mut paths);
        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(name: &str, path: &str) -> BinderNode {
        BinderNode::new_document(name, PathBuf::from(path))
    }

    fn folder(name: &str, path: &str, children: Vec<BinderNode>) -> BinderNode {
        BinderNode::new_folder(name, PathBuf::from(path), children)
    }

    #[test]
    fn find_by_path_locates_nested_document() {
        let tree = BinderTree {
            root: folder(
                "root",
                "/vault",
                vec![folder(
                    "Chapter 1",
                    "/vault/Chapter 1",
                    vec![doc("scene", "/vault/Chapter 1/scene.md")],
                )],
            ),
        };

        let found = tree.find_by_path(Path::new("/vault/Chapter 1/scene.md"));
        assert_eq!(found.map(|n| n.name.as_str()), Some("scene"));
    }

    #[test]
    fn find_by_path_returns_none_for_missing_path() {
        let tree = BinderTree {
            root: folder("root", "/vault", vec![doc("a", "/vault/a.md")]),
        };

        assert!(tree.find_by_path(Path::new("/vault/missing.md")).is_none());
    }

    #[test]
    fn insert_under_adds_child_to_matching_folder() {
        let mut tree = BinderTree {
            root: folder(
                "root",
                "/vault",
                vec![folder("Chapter 1", "/vault/Chapter 1", vec![])],
            ),
        };

        let inserted = tree.insert_under(
            Path::new("/vault/Chapter 1"),
            doc("new scene", "/vault/Chapter 1/new.md"),
        );

        assert!(inserted);
        let chapter = tree.find_by_path(Path::new("/vault/Chapter 1")).unwrap();
        assert_eq!(chapter.children().len(), 1);
        assert_eq!(chapter.children()[0].name, "new scene");
    }

    #[test]
    fn insert_under_returns_false_for_missing_parent() {
        let mut tree = BinderTree {
            root: folder("root", "/vault", vec![]),
        };

        let inserted =
            tree.insert_under(Path::new("/vault/missing"), doc("x", "/vault/missing/x.md"));
        assert!(!inserted);
    }

    #[test]
    fn insert_under_returns_false_when_target_is_a_document() {
        let mut tree = BinderTree {
            root: folder("root", "/vault", vec![doc("a", "/vault/a.md")]),
        };

        let inserted = tree.insert_under(Path::new("/vault/a.md"), doc("b", "/vault/b.md"));
        assert!(!inserted);
    }

    #[test]
    fn find_document_by_stem_matches_case_insensitively() {
        let tree = BinderTree {
            root: folder(
                "root",
                "/vault",
                vec![folder(
                    "Chapter 1",
                    "/vault/Chapter 1",
                    vec![doc("Opening Scene", "/vault/Chapter 1/Opening Scene.md")],
                )],
            ),
        };

        let found = tree.find_document_by_stem("opening scene");
        assert_eq!(found.map(|n| n.name.as_str()), Some("Opening Scene"));
    }

    #[test]
    fn find_document_by_stem_matches_non_ascii_titles_case_insensitively() {
        let tree = BinderTree {
            root: folder("root", "/vault", vec![doc("Café", "/vault/Café.md")]),
        };

        assert!(tree.find_document_by_stem("CAFÉ").is_some());
        assert!(tree.find_document_by_stem("café").is_some());
    }

    #[test]
    fn find_document_by_stem_returns_none_for_missing_or_folder_name() {
        let tree = BinderTree {
            root: folder(
                "root",
                "/vault",
                vec![folder("Chapter 1", "/vault/Chapter 1", vec![])],
            ),
        };

        assert!(tree.find_document_by_stem("Chapter 1").is_none());
        assert!(tree.find_document_by_stem("nonexistent").is_none());
    }

    #[test]
    fn document_names_collects_every_document_but_no_folders() {
        let tree = BinderTree {
            root: folder(
                "root",
                "/vault",
                vec![
                    doc("Intro", "/vault/Intro.md"),
                    folder(
                        "Chapter 1",
                        "/vault/Chapter 1",
                        vec![doc("Opening Scene", "/vault/Chapter 1/Opening Scene.md")],
                    ),
                ],
            ),
        };

        assert_eq!(tree.document_names(), vec!["Intro", "Opening Scene"]);
    }

    #[test]
    fn document_names_is_empty_for_a_folder_only_tree() {
        let tree = BinderTree {
            root: folder(
                "root",
                "/vault",
                vec![folder("empty", "/vault/empty", vec![])],
            ),
        };

        assert!(tree.document_names().is_empty());
    }

    #[test]
    fn document_paths_collects_absolute_paths_in_tree_order() {
        let tree = BinderTree {
            root: folder(
                "root",
                "/vault",
                vec![
                    doc("Intro", "/vault/Intro.md"),
                    folder(
                        "Chapter 1",
                        "/vault/Chapter 1",
                        vec![doc("Opening Scene", "/vault/Chapter 1/Opening Scene.md")],
                    ),
                ],
            ),
        };

        assert_eq!(
            tree.document_paths(),
            vec![
                PathBuf::from("/vault/Intro.md"),
                PathBuf::from("/vault/Chapter 1/Opening Scene.md"),
            ]
        );
    }
}
