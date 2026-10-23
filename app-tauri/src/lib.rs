mod pomodoro;
mod project;

use pomodoro::PomodoroHandle;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(PomodoroHandle::default())
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
        ])
        .setup(|app| {
            pomodoro::spawn_ticker(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Smaragd Tauri app");
}
