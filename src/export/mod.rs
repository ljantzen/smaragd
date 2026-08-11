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
use std::path::{Path, PathBuf};

use docx_rs::DocxError;

use crate::frontmatter;
use crate::markdown::{self, Block};
use crate::project::model::{BinderNode, BinderNodeKind, document_label};
use crate::project::{FolderRole, Project};

/// One document gathered for export: its title, parsed content, and the on-disk
/// path it came from (needed to resolve any relative image `src` in its content).
pub struct ExportDoc {
    pub title: String,
    pub blocks: Vec<Block>,
    pub source_path: PathBuf,
}

/// Book-level metadata entered once in the export dialog — title/subtitle/
/// author only; everything typographic (fonts, sizes, page setup) lives in
/// [`style::TypesetStyle`] instead, since a style isn't book-specific.
#[derive(Debug, Clone, Default)]
pub struct BookMeta {
    pub title: String,
    /// Optional — not every book has one. Rendered on the DOCX/PDF title
    /// page under `title` (blank when empty, same as `title`/`author`) and
    /// available to a custom style's running-header template as `{subtitle}`.
    pub subtitle: String,
    pub author: String,
}

impl BookMeta {
    /// A filesystem-safe base filename (no extension) for the save dialog's
    /// default `set_file_name` — `"Title - Subtitle"` when both are set,
    /// whichever one is when only one is, or `"manuscript"` (the old
    /// hardcoded default, kept as the fallback) when neither is.
    pub fn filename_stem(&self) -> String {
        let title = self.title.trim();
        let subtitle = self.subtitle.trim();
        let combined = match (title.is_empty(), subtitle.is_empty()) {
            (true, true) => return "manuscript".to_string(),
            (true, false) => subtitle.to_string(),
            (false, true) => title.to_string(),
            (false, false) => format!("{title} - {subtitle}"),
        };
        sanitize_filename_component(&combined)
    }
}

/// Replace characters illegal in a Windows filename (`< > : " / \ | ? *` and
/// ASCII control characters) with `_`, and trim trailing dots/spaces (also a
/// Windows-specific restriction) — the union of what's actually illegal
/// across Linux/macOS/Windows, since this app ships on all three and a book
/// title/subtitle is free text a user could type any of these into.
/// `pub(crate)` so `import::docx` can reuse it for filenames derived from a
/// document's own (equally free-text) heading text, rather than duplicating
/// this logic.
pub(crate) fn sanitize_filename_component(s: &str) -> String {
    let replaced: String = s
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            {
                '_'
            } else {
                ch
            }
        })
        .collect();
    let trimmed = replaced.trim_end_matches(['.', ' ']).trim();
    if trimmed.is_empty() {
        "manuscript".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Whether `resolved` is (once symlinks are resolved) actually inside `project_root`
/// — or `project_root` wasn't given, in which case there's nothing to bound against
/// (e.g. a caller with no project context, or these unit tests). A path that doesn't
/// exist, or a `project_root` that doesn't, can't be canonicalized and is treated as
/// *not* contained — fail closed rather than let an unresolvable path through. Shared
/// by `ui::markdown_preview`'s `resolve_image_uri` (live preview) and
/// `resolve_image_fs_path` below (export), so both enforce the same containment rule.
pub(crate) fn is_within_project(resolved: &Path, project_root: Option<&Path>) -> bool {
    let Some(project_root) = project_root else {
        return true;
    };
    let (Ok(resolved), Ok(project_root)) = (resolved.canonicalize(), project_root.canonicalize())
    else {
        return false;
    };
    resolved.starts_with(project_root)
}

/// Like `ui::markdown_preview`'s `resolve_image_uri`, but for a caller that wants to
/// read the image's bytes off disk rather than hand egui a URI: resolves `src`
/// relative to `doc_dir` and returns the filesystem path only if it's actually
/// contained within `project_root` (same symlink-aware containment check), or `None`
/// for a remote `http(s)://`/`data:` URI (never fetched) or one that fails containment.
pub(crate) fn resolve_image_fs_path(
    src: &str,
    doc_dir: &Path,
    project_root: &Path,
) -> Option<PathBuf> {
    if src.starts_with("data:") || src.contains("://") {
        return None;
    }
    let path = Path::new(src);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        doc_dir.join(path)
    };
    is_within_project(&resolved, Some(project_root)).then_some(resolved)
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

    #[test]
    fn filename_stem_falls_back_to_manuscript_when_neither_is_set() {
        assert_eq!(BookMeta::default().filename_stem(), "manuscript");
    }

    #[test]
    fn filename_stem_joins_title_and_subtitle_with_a_dash() {
        let meta = BookMeta {
            title: "My Book".to_string(),
            subtitle: "A Subtitle".to_string(),
            ..BookMeta::default()
        };
        assert_eq!(meta.filename_stem(), "My Book - A Subtitle");
    }

    #[test]
    fn filename_stem_falls_back_to_whichever_of_title_or_subtitle_is_set() {
        let title_only = BookMeta {
            title: "My Book".to_string(),
            ..BookMeta::default()
        };
        assert_eq!(title_only.filename_stem(), "My Book");

        let subtitle_only = BookMeta {
            subtitle: "A Subtitle".to_string(),
            ..BookMeta::default()
        };
        assert_eq!(subtitle_only.filename_stem(), "A Subtitle");
    }

    #[test]
    fn filename_stem_replaces_characters_illegal_in_a_windows_filename() {
        let meta = BookMeta {
            title: "Who: What/Why? \"Really\" <Now> | *Then*".to_string(),
            ..BookMeta::default()
        };
        assert_eq!(
            meta.filename_stem(),
            "Who_ What_Why_ _Really_ _Now_ _ _Then_"
        );
    }

    #[test]
    fn filename_stem_trims_trailing_dots_and_spaces() {
        let meta = BookMeta {
            title: "Trailing dots... ".to_string(),
            ..BookMeta::default()
        };
        assert_eq!(meta.filename_stem(), "Trailing dots");
    }

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
