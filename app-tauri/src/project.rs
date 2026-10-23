use std::path::Path;

use serde::Serialize;
use smaragd_core::project::Project;
use smaragd_core::project::model::{BinderNode, BinderNodeKind};

#[derive(Serialize)]
#[serde(tag = "kind")]
pub enum BinderNodeDto {
    Folder {
        name: String,
        path: String,
        children: Vec<BinderNodeDto>,
    },
    Document {
        name: String,
        path: String,
    },
}

impl From<&BinderNode> for BinderNodeDto {
    fn from(node: &BinderNode) -> Self {
        match &node.kind {
            BinderNodeKind::Folder { children } => BinderNodeDto::Folder {
                name: node.name.clone(),
                path: node.path.display().to_string(),
                children: children.iter().map(BinderNodeDto::from).collect(),
            },
            BinderNodeKind::Document => BinderNodeDto::Document {
                name: node.name.clone(),
                path: node.path.display().to_string(),
            },
        }
    }
}

#[derive(Serialize)]
pub struct ProjectSummary {
    root: String,
    book_title: Option<String>,
    tree: BinderNodeDto,
}

impl From<&Project> for ProjectSummary {
    fn from(project: &Project) -> Self {
        ProjectSummary {
            root: project.root.display().to_string(),
            book_title: project.meta.book_title.clone(),
            tree: BinderNodeDto::from(&project.tree.root),
        }
    }
}

/// Open (or re-open) a project at `root` and return its binder tree. Stateless
/// for now — each call re-scans from disk — which is fine at Phase 2 scope; a
/// held-in-memory `Project` (via `tauri::State`) is a later refinement once the
/// editor/autosave work (Phase 3) actually needs one.
#[tauri::command]
pub fn project_open(root: String) -> Result<ProjectSummary, String> {
    let project = Project::load_from_folder(Path::new(&root)).map_err(|e| e.to_string())?;
    Ok(ProjectSummary::from(&project))
}

/// No held state to release yet (see `project_open`'s doc comment) — kept as a
/// real command now so the frontend's open/close lifecycle doesn't need to
/// change shape once one exists.
#[tauri::command]
pub fn project_close() -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub fn project_binder_tree(root: String) -> Result<BinderNodeDto, String> {
    let project = Project::load_from_folder(Path::new(&root)).map_err(|e| e.to_string())?;
    Ok(BinderNodeDto::from(&project.tree.root))
}

#[tauri::command]
pub fn document_read(path: String) -> Result<String, String> {
    smaragd_core::project::store::native_store()
        .read_to_string(Path::new(&path))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn document_save(path: String, contents: String) -> Result<(), String> {
    smaragd_core::project::store::native_store()
        .write(Path::new(&path), contents.as_bytes())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn render_markdown(markdown: String) -> String {
    smaragd_core::markdown::render_html(&markdown)
}
