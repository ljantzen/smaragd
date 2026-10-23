//! UI-agnostic application logic for Smaragd: project storage, sync/collab,
//! export/import, and the editor's text-buffer model. Carries no `egui`,
//! `eframe`, or native-integration (`rfd`/`arboard`/`opener`/`notify-rust`)
//! dependency, so it can be driven identically by the desktop app and (once
//! built) a Tauri mobile shell.

pub mod attachments;
pub mod backup;
pub mod collab;
pub mod dashboard;
pub mod debug_flags;
pub mod document_history;
pub mod editor;
pub mod export;
pub mod fonts;
pub mod frontmatter;
pub mod fuzzy;
pub mod git;
pub mod import;
pub mod markdown;
pub mod pomodoro;
pub mod project;
pub mod project_template;
pub mod save_debug;
pub mod search;
pub mod spellcheck;
pub mod streak;
pub mod sync;
pub mod templates;
