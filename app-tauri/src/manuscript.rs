//! Export Manuscript (and the docx/epub/pdf/Scrivener Import submenu) — the
//! real egui app's menu items, wired to core::export/core::import, which
//! were already fully portable (ported in Phase 0, never used by a command
//! until now).

use std::path::Path;

use serde::Serialize;
use smaragd_core::export::style::built_in_styles;
use smaragd_core::export::{BookMeta, gather};
use smaragd_core::import::{
    docx as import_docx, epub as import_epub, pdf as import_pdf, scrivener,
};
use smaragd_core::project::{FolderRole, Project};

fn load(root: &str) -> Result<Project, String> {
    Project::load_from_folder(Path::new(root)).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct ManuscriptFolder {
    path: String,
    name: String,
}

/// Every folder with the Manuscript role, for the frontend to decide: zero
/// means "export the whole project," one means "just export it," two or
/// more means "ask which one" — same three-way branch the real File menu's
/// Export Manuscript item takes.
#[tauri::command]
pub fn manuscript_folders(root: String) -> Result<Vec<ManuscriptFolder>, String> {
    let project = load(&root)?;
    Ok(project
        .folder_role_paths(FolderRole::Manuscript)
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            ManuscriptFolder {
                path: path.display().to_string(),
                name,
            }
        })
        .collect())
}

#[tauri::command]
pub fn export_manuscript(
    root: String,
    folder: String,
    format: String,
    title: String,
    subtitle: String,
    author: String,
    out_path: String,
) -> Result<(), String> {
    let project = load(&root)?;
    let folder_node = project
        .tree
        .find_by_path(Path::new(&folder))
        .ok_or_else(|| format!("{folder} is no longer in the binder"))?;
    let docs = gather(&project, folder_node, false);
    let meta = BookMeta {
        title,
        subtitle,
        author,
    };
    // The first built-in style, same "a style has to be chosen somehow with
    // no style-picker UI yet" simplification the rest of this port's export
    // path doesn't have a precedent for — later refinement once a real
    // style picker exists.
    let style = built_in_styles()
        .into_iter()
        .next()
        .ok_or("no built-in export style available")?;
    let out_path = Path::new(&out_path);

    match format.as_str() {
        "docx" => {
            smaragd_core::export::docx::export_docx(&docs, &meta, &style, &project.root, out_path)
                .map_err(|e| e.to_string())
        }
        "epub" => {
            smaragd_core::export::epub::export_epub(&docs, &meta, &style, &project.root, out_path)
                .map_err(|e| e.to_string())
        }
        "pdf" => {
            smaragd_core::export::pdf::export_pdf(&docs, &meta, &style, &project.root, out_path)
                .map(|_spine_width_in| ())
                .map_err(|e| e.to_string())
        }
        other => Err(format!("unknown export format: {other}")),
    }
}

#[tauri::command]
pub fn import_file(
    root: String,
    parent: String,
    format: String,
    file_path: String,
) -> Result<(), String> {
    let mut project = load(&root)?;
    let bytes = std::fs::read(&file_path).map_err(|e| e.to_string())?;
    let fallback_title = Path::new(&file_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".to_string());

    let nodes = match format.as_str() {
        "docx" => import_docx::parse(&bytes, &fallback_title).map_err(|e| e.to_string())?,
        "epub" => import_epub::parse(&bytes).map_err(|e| e.to_string())?,
        "pdf" => import_pdf::parse(&bytes, &fallback_title).map_err(|e| e.to_string())?,
        other => return Err(format!("unknown import format: {other}")),
    };
    smaragd_core::import::write_imported_tree(&mut project, Path::new(&parent), &nodes)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn import_scrivener(root: String, parent: String, scrivener_dir: String) -> Result<(), String> {
    let mut project = load(&root)?;
    let nodes = scrivener::parse(Path::new(&scrivener_dir)).map_err(|e| e.to_string())?;
    smaragd_core::import::write_imported_tree(&mut project, Path::new(&parent), &nodes)
        .map_err(|e| e.to_string())?;
    Ok(())
}
