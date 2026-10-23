mod collab;
mod pomodoro;
mod project;
mod story_cards;
mod sync;

use collab::CollabState;
use pomodoro::PomodoroHandle;
use sync::SyncState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(PomodoroHandle::default())
        .manage(CollabState::default())
        .manage(SyncState::default())
        .invoke_handler(tauri::generate_handler![
            pomodoro::pomodoro_snapshot,
            pomodoro::pomodoro_start,
            pomodoro::pomodoro_pause,
            pomodoro::pomodoro_skip,
            project::project_open,
            project::project_close,
            project::project_binder_tree,
            project::document_read,
            project::document_save,
            project::render_markdown,
            project::binder_create_document,
            project::binder_create_folder,
            project::binder_rename,
            project::binder_delete,
            project::binder_move,
            project::binder_move_before,
            story_cards::story_cards_list,
            story_cards::story_card_upsert,
            story_cards::story_card_delete,
            story_cards::story_card_move,
            collab::collab_status,
            collab::collab_host,
            collab::collab_join,
            collab::collab_end,
            collab::collab_sync_local_edit,
            sync::sync_demo_start,
            sync::sync_demo_stop,
            sync::sync_demo_now,
        ])
        .setup(|app| {
            pomodoro::spawn_ticker(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Smaragd Tauri app");
}
