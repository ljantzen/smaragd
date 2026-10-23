use std::path::Path;

use smaragd_core::project::{Project, StoryCard};
use uuid::Uuid;

fn load(root: &str) -> Result<Project, String> {
    Project::load_from_folder(Path::new(root)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn story_cards_list(root: String) -> Result<Vec<StoryCard>, String> {
    Ok(load(&root)?.meta.story_cards)
}

#[tauri::command]
pub fn story_card_upsert(root: String, card: StoryCard) -> Result<Vec<StoryCard>, String> {
    let mut project = load(&root)?;
    project.upsert_story_card(card).map_err(|e| e.to_string())?;
    Ok(project.meta.story_cards)
}

#[tauri::command]
pub fn story_card_delete(root: String, id: Uuid) -> Result<Vec<StoryCard>, String> {
    let mut project = load(&root)?;
    project.delete_story_card(id).map_err(|e| e.to_string())?;
    Ok(project.meta.story_cards)
}

#[tauri::command]
pub fn story_card_move(root: String, id: Uuid, new_index: usize) -> Result<Vec<StoryCard>, String> {
    let mut project = load(&root)?;
    project
        .move_story_card(id, new_index)
        .map_err(|e| e.to_string())?;
    Ok(project.meta.story_cards)
}
