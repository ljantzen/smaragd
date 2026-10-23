//! Phase 7: the remaining DockTab panels (app/src/app/dock.rs) that were
//! StubPanel placeholders since Phase 5 — Backlinks, Tags, Metadata, Word
//! Count, Streak, Bookmarks, Notes, Dashboard, Version Activity. Belief
//! Timeline needs no new command: it's just the existing story_cards_list
//! data (Phase 4), filtered/ordered differently in the frontend.
//!
//! All read-mostly, reusing `Project` query methods that already exist and
//! already recompute fresh from disk on every call (same convention as
//! Phase 2's `project_binder_tree`) — no new core logic, only IPC DTOs
//! around what's there. Deliberately lighter than the real egui panels:
//! Dashboard's activity-by-weekday/by-hour bar charts (egui_plot-specific)
//! aren't ported, Version Activity skips the live "this session's git
//! actions" log (app-level state not available here), and Metadata is
//! read-only (no save-frontmatter command yet) — each a later refinement,
//! not required to prove these panels dock and show real project data.

use std::path::Path;

use serde::Serialize;
use smaragd_core::git;
use smaragd_core::project::{Project, WordCountScope};
use smaragd_core::streak;
use uuid::Uuid;

fn load(root: &str) -> Result<Project, String> {
    Project::load_from_folder(Path::new(root)).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct BacklinkDto {
    source_path: String,
    source_title: String,
    snippet: String,
}

#[tauri::command]
pub fn backlinks_for(root: String, path: String) -> Result<Vec<BacklinkDto>, String> {
    let project = load(&root)?;
    Ok(project
        .backlinks(Path::new(&path))
        .into_iter()
        .map(|b| BacklinkDto {
            source_path: b.source_path.display().to_string(),
            source_title: b.source_title,
            snippet: b.snippet,
        })
        .collect())
}

#[derive(Serialize)]
pub struct TagGroupDto {
    tag: String,
    documents: Vec<(String, String)>,
}

#[tauri::command]
pub fn all_tags(root: String) -> Result<Vec<String>, String> {
    Ok(load(&root)?.all_tags())
}

#[tauri::command]
pub fn documents_with_tag(root: String, tag: String) -> Result<TagGroupDto, String> {
    let project = load(&root)?;
    let documents = project
        .documents_with_tag(&tag)
        .into_iter()
        .map(|(path, title)| (path.display().to_string(), title))
        .collect();
    Ok(TagGroupDto { tag, documents })
}

#[derive(Serialize)]
pub struct DocumentMetaDto {
    section_type: Option<String>,
    status: Option<String>,
    pov: Option<String>,
    word_count_target: Option<u32>,
    tags: Vec<String>,
}

#[tauri::command]
pub fn document_metadata(root: String, path: String) -> Result<DocumentMetaDto, String> {
    let project = load(&root)?;
    let meta = project
        .document_meta(Path::new(&path))
        .map_err(|e| e.to_string())?;
    Ok(DocumentMetaDto {
        section_type: meta.section_type,
        status: meta.status,
        pov: meta.pov,
        word_count_target: meta.word_count_target,
        tags: meta.tags,
    })
}

#[derive(Serialize)]
pub struct WordCountDto {
    manuscript_only: usize,
    everything_except_trash: usize,
    draft_target_words: Option<u32>,
    today_target_words: Option<u32>,
    today_words_so_far: u32,
}

#[tauri::command]
pub fn word_count(root: String) -> Result<WordCountDto, String> {
    let project = load(&root)?;
    let scope = project.meta.word_count_scope;
    let today_words_so_far =
        (project.word_count(scope) as u32).saturating_sub(project.today_baseline_words(scope));
    Ok(WordCountDto {
        manuscript_only: project.word_count(WordCountScope::ManuscriptOnly),
        everything_except_trash: project.word_count(WordCountScope::EverythingExceptTrash),
        draft_target_words: project.meta.draft_target_words,
        today_target_words: project.meta.today_target_words,
        today_words_so_far,
    })
}

#[derive(Serialize)]
pub struct StreakDto {
    enabled: bool,
    status: &'static str,
    actual_this_week: u32,
    target_this_week: u32,
}

#[tauri::command]
pub fn streak_status(root: String) -> Result<StreakDto, String> {
    let project = load(&root)?;
    let meta = &project.meta;
    if !meta.streak_enabled {
        return Ok(StreakDto {
            enabled: false,
            status: "disabled",
            actual_this_week: 0,
            target_this_week: 0,
        });
    }
    let today = chrono::Local::now().date_naive();
    let scope = meta.word_count_scope;
    let today_words_so_far =
        (project.word_count(scope) as u32).saturating_sub(project.today_baseline_words(scope));
    let status = streak::evaluate_streak(
        &meta.streak_schedule,
        meta.streak_evaluation_mode,
        meta.streak_red_threshold_weeks,
        &meta.daily_word_counts,
        today,
    );
    let (actual_this_week, target_this_week) = streak::current_week_progress(
        &meta.streak_schedule,
        &meta.daily_word_counts,
        today_words_so_far,
        today,
    );
    Ok(StreakDto {
        enabled: true,
        status: match status {
            streak::StreakStatus::Disabled => "disabled",
            streak::StreakStatus::InsufficientData => "insufficient_data",
            streak::StreakStatus::Green => "green",
            streak::StreakStatus::Yellow => "yellow",
            streak::StreakStatus::Red => "red",
        },
        actual_this_week,
        target_this_week,
    })
}

#[derive(Serialize)]
pub struct ResolvedBookmarkDto {
    id: String,
    path: String,
    line: usize,
    document_stem: Option<String>,
}

#[tauri::command]
pub fn bookmarks_list(root: String) -> Result<Vec<ResolvedBookmarkDto>, String> {
    let project = load(&root)?;
    Ok(project
        .resolved_bookmarks()
        .into_iter()
        .map(|b| ResolvedBookmarkDto {
            id: b.id.to_string(),
            path: b.path.display().to_string(),
            line: b.line,
            document_stem: b.document_stem,
        })
        .collect())
}

#[tauri::command]
pub fn bookmark_delete(root: String, id: String) -> Result<(), String> {
    let mut project = load(&root)?;
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    project.delete_bookmark(id).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct ResolvedNoteDto {
    id: String,
    path: String,
    line: usize,
    column: usize,
    text: String,
    document_stem: Option<String>,
}

#[tauri::command]
pub fn notes_list(root: String) -> Result<Vec<ResolvedNoteDto>, String> {
    let project = load(&root)?;
    Ok(project
        .resolved_notes()
        .into_iter()
        .map(|n| ResolvedNoteDto {
            id: n.id.to_string(),
            path: n.path.display().to_string(),
            line: n.line,
            column: n.column,
            text: n.text,
            document_stem: n.document_stem,
        })
        .collect())
}

#[tauri::command]
pub fn note_delete(root: String, id: String) -> Result<(), String> {
    let mut project = load(&root)?;
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    project.delete_note(id).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct DashboardDto {
    session_count: usize,
    total_minutes: u64,
    total_words: u64,
    documents_created: usize,
}

#[tauri::command]
pub fn dashboard_summary(root: String) -> Result<DashboardDto, String> {
    let project = load(&root)?;
    let summary = smaragd_core::dashboard::summary(&project.meta.session_log);
    Ok(DashboardDto {
        session_count: summary.session_count,
        total_minutes: summary.total_minutes,
        total_words: summary.total_words,
        documents_created: project.meta.document_created.len(),
    })
}

#[derive(Serialize)]
pub struct CommitDto {
    hash: String,
    author: String,
    relative_date: String,
    subject: String,
}

#[derive(Serialize)]
pub struct VersionActivityDto {
    git_available: bool,
    dirty_files: Vec<String>,
    commits: Vec<CommitDto>,
}

#[tauri::command]
pub fn version_activity(root: String) -> Result<VersionActivityDto, String> {
    let project = load(&root)?;
    let git_available = project.meta.git_enabled && git::is_repo(&project.root);
    if !git_available {
        return Ok(VersionActivityDto {
            git_available: false,
            dirty_files: Vec::new(),
            commits: Vec::new(),
        });
    }
    let dirty_files = git::status(&project.root)
        .map(|set| {
            let mut paths: Vec<String> = set.into_iter().map(|p| p.display().to_string()).collect();
            paths.sort();
            paths
        })
        .unwrap_or_default();
    let commits = git::log(&project.root, 20)
        .into_iter()
        .map(|c| CommitDto {
            hash: c.hash,
            author: c.author,
            relative_date: c.relative_date,
            subject: c.subject,
        })
        .collect();
    Ok(VersionActivityDto {
        git_available: true,
        dirty_files,
        commits,
    })
}
