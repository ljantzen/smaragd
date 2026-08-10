//! Import an existing manuscript — DOCX, EPUB, a Scrivener project, or PDF —
//! into a smaragd project as binder documents/folders.
//!
//! Each format's parser (`import::docx`, and siblings added alongside it)
//! only has to produce a [`Vec<ImportedNode>`] — a plain markdown string per
//! document, optionally grouped into folders — which [`write_imported_tree`]
//! then writes into a real [`Project`] via the same
//! `create_document_with_content`/`create_folder`/`ensure_role_folder` APIs
//! `project_template::ProjectTemplate::apply` already uses to stamp out a New
//! Project template's starter content. No format-specific code ever touches
//! `BinderTree` directly.

pub mod docx;
pub mod epub;
pub mod pdf;

use std::fmt;
use std::io;
use std::path::Path;

use crate::project::{FolderRole, Project};

/// One document or folder to write into the project, and (for a folder) its
/// children — the shared target shape every format parser produces.
pub struct ImportedNode {
    pub name: String,
    pub kind: ImportedKind,
}

pub enum ImportedKind {
    /// `role`, if set, is assigned via `Project::ensure_role_folder` — see
    /// `write_imported_tree`'s doc comment for why that makes this folder's
    /// own `name` and position in its parent's children moot.
    Folder {
        role: Option<FolderRole>,
        children: Vec<ImportedNode>,
    },
    Document {
        markdown: String,
    },
}

/// How many documents/folders `write_imported_tree` actually created — shown
/// to the user afterward (e.g. "Imported 12 documents, 3 folders") since a
/// multi-document import has no other feedback a single Export's one output
/// file already gets for free.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub documents: usize,
    pub folders: usize,
}

impl ImportSummary {
    fn merge(&mut self, other: ImportSummary) {
        self.documents += other.documents;
        self.folders += other.folders;
    }
}

/// Mirrors `export::ExportError`'s shape: one variant per format-specific
/// failure mode, plus a shared `Io` for anything from the write side
/// (`write_imported_tree`, always `io::Error` since `Project::create_*` is).
#[derive(Debug)]
pub enum ImportError {
    Io(io::Error),
    Docx(docx_rs::ReaderError),
    Epub(epub::EpubImportError),
    Pdf(pdf_extract::OutputError),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Io(err) => write!(f, "{err}"),
            ImportError::Docx(err) => write!(f, "{err}"),
            ImportError::Epub(err) => write!(f, "{err}"),
            ImportError::Pdf(err) => write!(f, "{err}"),
        }
    }
}

impl From<io::Error> for ImportError {
    fn from(err: io::Error) -> Self {
        ImportError::Io(err)
    }
}

impl From<docx_rs::ReaderError> for ImportError {
    fn from(err: docx_rs::ReaderError) -> Self {
        ImportError::Docx(err)
    }
}

impl From<epub::EpubImportError> for ImportError {
    fn from(err: epub::EpubImportError) -> Self {
        ImportError::Epub(err)
    }
}

impl From<pdf_extract::OutputError> for ImportError {
    fn from(err: pdf_extract::OutputError) -> Self {
        ImportError::Pdf(err)
    }
}

/// Writes `nodes` into `project` under `parent`, recursively — the single
/// shared "commit a parsed import to disk" path every format parser feeds
/// into. A name collision is resolved the same way `ensure_role_folder`
/// already resolves one (`project::unique_child_name`'s " (2)", " (3)", ...
/// suffixing) rather than failing the whole import over one clashing title.
///
/// A `Folder` node carrying a `role` is written via `Project::ensure_role_folder`
/// instead of `create_folder` — which, notably, always places that folder at
/// the project *root*, ignoring `parent`. This only matters for Scrivener
/// imports (the only parser that ever sets a `role`, mapping its Draft/
/// Research folders onto smaragd's own `FolderRole::Manuscript`/`Research`):
/// a project-wide-unique role folder living somewhere other than the
/// project's own root would violate the very invariant `FolderRole` exists to
/// enforce (see `project::roles`'s doc comment), so this is the correct
/// behavior, not a limitation to fix.
pub fn write_imported_tree(
    project: &mut Project,
    parent: &Path,
    nodes: &[ImportedNode],
) -> io::Result<ImportSummary> {
    let mut summary = ImportSummary::default();
    for node in nodes {
        match &node.kind {
            ImportedKind::Folder {
                role: Some(role),
                children,
            } => {
                project.ensure_role_folder(*role, &node.name)?;
                let path = project
                    .folder_role_path(*role)
                    .expect("ensure_role_folder just assigned this role to a folder");
                summary.folders += 1;
                summary.merge(write_imported_tree(project, &path, children)?);
            }
            ImportedKind::Folder {
                role: None,
                children,
            } => {
                let name = crate::project::unique_child_name(parent, &node.name);
                let path = project.create_folder(parent, &name)?;
                summary.folders += 1;
                summary.merge(write_imported_tree(project, &path, children)?);
            }
            ImportedKind::Document { markdown } => {
                let filename =
                    crate::project::unique_child_name(parent, &format!("{}.md", node.name));
                project.create_document_with_content(parent, &filename, markdown)?;
                summary.documents += 1;
            }
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_imported_tree_creates_documents_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let nodes = vec![
            ImportedNode {
                name: "Chapter One".to_string(),
                kind: ImportedKind::Document {
                    markdown: "It was a dark and stormy night.".to_string(),
                },
            },
            ImportedNode {
                name: "Notes".to_string(),
                kind: ImportedKind::Folder {
                    role: None,
                    children: vec![ImportedNode {
                        name: "Idea".to_string(),
                        kind: ImportedKind::Document {
                            markdown: "A loose idea.".to_string(),
                        },
                    }],
                },
            },
        ];

        let summary = write_imported_tree(&mut project, dir.path(), &nodes).unwrap();

        assert_eq!(
            summary,
            ImportSummary {
                documents: 2,
                folders: 1
            }
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("Chapter One.md")).unwrap(),
            "It was a dark and stormy night."
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("Notes").join("Idea.md")).unwrap(),
            "A loose idea."
        );
    }

    #[test]
    fn write_imported_tree_deduplicates_a_colliding_document_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project
            .create_document_with_content(dir.path(), "Chapter One.md", "existing")
            .unwrap();
        let nodes = vec![ImportedNode {
            name: "Chapter One".to_string(),
            kind: ImportedKind::Document {
                markdown: "imported".to_string(),
            },
        }];

        write_imported_tree(&mut project, dir.path(), &nodes).unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.path().join("Chapter One (2).md")).unwrap(),
            "imported"
        );
    }

    #[test]
    fn write_imported_tree_assigns_a_folder_role_and_ignores_its_requested_parent() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let subfolder = project.create_folder(dir.path(), "Destination").unwrap();
        let nodes = vec![ImportedNode {
            name: "Research".to_string(),
            kind: ImportedKind::Folder {
                role: Some(FolderRole::Research),
                children: vec![],
            },
        }];

        write_imported_tree(&mut project, &subfolder, &nodes).unwrap();

        assert_eq!(
            project.folder_role_path(FolderRole::Research),
            Some(dir.path().join("Research"))
        );
    }
}
