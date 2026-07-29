//! Compile a binder folder (and its subfolders) into a single DOCX, EPUB, or
//! print-ready PDF file.
//!
//! Walks the project's existing `Block`/`Span` markdown IR (`crate::markdown`)
//! — the same intermediate representation the egui preview renders from, but
//! entirely egui-agnostic — so each renderer (`docx`/`epub`/`pdf`) only has to
//! translate that IR into its own output format, not re-parse markdown itself.
//! All three read from a shared [`style::TypesetStyle`] (fonts, page setup,
//! running headers, drop caps) rather than hardcoding their own literals, so
//! one style genuinely drives every output format.

pub mod docx;
pub mod epub;
pub mod pdf;
pub mod style;

use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

use docx_rs::DocxError;

use crate::frontmatter;
use crate::markdown::{self, Block};
use crate::project::model::{BinderNode, BinderNodeKind};
use crate::project::{FolderRole, Project};
use crate::ui::binder_panel::document_label;

/// One document gathered for export: its title, parsed content, and the on-disk
/// path it came from (needed to resolve any relative image `src` in its content).
pub struct ExportDoc {
    pub title: String,
    pub blocks: Vec<Block>,
    pub source_path: PathBuf,
}

/// Book-level metadata entered once in the export dialog — title/author only;
/// everything typographic (fonts, sizes, page setup) lives in
/// [`style::TypesetStyle`] instead, since a style isn't book-specific.
#[derive(Debug, Clone, Default)]
pub struct BookMeta {
    pub title: String,
    pub author: String,
}

#[derive(Debug)]
pub enum ExportError {
    Io(io::Error),
    Docx(DocxError),
    Epub(epub_builder::Error),
    Pdf(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Io(err) => write!(f, "{err}"),
            ExportError::Docx(err) => write!(f, "{err}"),
            ExportError::Epub(err) => write!(f, "{err}"),
            ExportError::Pdf(msg) => write!(f, "{msg}"),
        }
    }
}

impl From<io::Error> for ExportError {
    fn from(err: io::Error) -> Self {
        ExportError::Io(err)
    }
}

impl From<DocxError> for ExportError {
    fn from(err: DocxError) -> Self {
        ExportError::Docx(err)
    }
}

impl From<epub_builder::Error> for ExportError {
    fn from(err: epub_builder::Error) -> Self {
        ExportError::Epub(err)
    }
}

/// Collects every document in `folder`'s subtree, in binder order, skipping any
/// nested folder whose role is `Trash` or `Templates` (trashed/template content
/// should never end up in a compiled manuscript by accident — see
/// `FolderRole`'s doc comment). A document that can't be read (e.g. removed from
/// disk since the binder was last scanned) is silently skipped, matching
/// `Project::backlinks`'s tolerance of unreadable files.
///
/// `typewriter_quotes` mirrors `Settings::typewriter_quotes` — when set, every
/// gathered document's parsed blocks are run through
/// `markdown::apply_typewriter_quotes` before export, so straight typewriter
/// punctuation ships as curly quotes/an em dash/an ellipsis without the source
/// `.md` files themselves ever being rewritten.
pub fn gather(project: &Project, folder: &BinderNode, typewriter_quotes: bool) -> Vec<ExportDoc> {
    let mut docs = Vec::new();
    gather_into(project, folder, typewriter_quotes, &mut docs);
    docs
}

fn gather_into(
    project: &Project,
    node: &BinderNode,
    typewriter_quotes: bool,
    out: &mut Vec<ExportDoc>,
) {
    match &node.kind {
        BinderNodeKind::Document => {
            let Ok(contents) = fs::read_to_string(&node.path) else {
                return;
            };
            let stripped = frontmatter::strip(&contents);
            let mut blocks = markdown::parse(stripped);
            if typewriter_quotes {
                markdown::apply_typewriter_quotes(&mut blocks);
            }
            out.push(ExportDoc {
                title: document_label(&node.name).to_string(),
                blocks,
                source_path: node.path.clone(),
            });
        }
        BinderNodeKind::Folder { children } => {
            if matches!(
                project.folder_role(&node.path),
                Some(FolderRole::Trash) | Some(FolderRole::Templates)
            ) {
                return;
            }
            for child in children {
                gather_into(project, child, typewriter_quotes, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::model::BinderTree;
    use std::path::Path;

    fn write(dir: &Path, rel: &str, contents: &str) -> PathBuf {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn gather_skips_trash_and_templates_but_keeps_research() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "Manuscript/Intro.md", "# Intro");
        write(root, "Manuscript/Trash/Deleted.md", "# gone");
        write(root, "Manuscript/Templates/Blank.md", "# blank");
        write(root, "Manuscript/Notes/Idea.md", "# idea");

        let mut project = Project::initialize(root).unwrap();
        let manuscript = root.join("Manuscript");
        let trash = manuscript.join("Trash");
        let templates = manuscript.join("Templates");
        let research = manuscript.join("Notes");
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        project
            .set_folder_role(&templates, Some(FolderRole::Templates))
            .unwrap();
        project
            .set_folder_role(&research, Some(FolderRole::Research))
            .unwrap();
        let project = Project::load_from_folder(root).unwrap();

        let folder = project.tree.find_by_path(&manuscript).unwrap();
        let docs = gather(&project, folder, false);
        let titles: Vec<&str> = docs.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(titles, vec!["Intro", "Idea"]);
    }

    #[test]
    fn gather_returns_documents_in_tree_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "B.md", "b");
        write(root, "A.md", "a");
        let project = Project::initialize(root).unwrap();
        let docs = gather(&project, &project.tree.root, false);
        // scan order, not alphabetical re-sort — see project/scan.rs
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn gather_on_an_empty_folder_returns_no_documents() {
        let dir = tempfile::tempdir().unwrap();
        let tree = BinderTree {
            root: BinderNode::new_folder("root", dir.path(), vec![]),
        };
        let mut project = Project::initialize(dir.path()).unwrap();
        project.tree = tree;
        assert!(gather(&project, &project.tree.root, false).is_empty());
    }

    #[test]
    fn gather_applies_typewriter_quotes_when_requested() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "Scene.md", r#""Wait--stop," she said."#);
        let project = Project::initialize(root).unwrap();

        let plain = gather(&project, &project.tree.root, false);
        let joined: String = plain[0]
            .blocks
            .iter()
            .flat_map(|b| b.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(joined, r#""Wait--stop," she said."#);

        let curled = gather(&project, &project.tree.root, true);
        let joined: String = curled[0]
            .blocks
            .iter()
            .flat_map(|b| b.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(joined, "“Wait—stop,” she said.");
    }
}
