pub mod backlinks_panel;
pub mod binder_panel;
pub mod command_prompt;
pub mod corkboard_panel;
pub mod editor_panel;
pub mod find_replace_panel;
pub mod markdown_preview;
pub mod metadata_panel;
pub mod name_prompt;
pub mod settings_panel;

/// A user request to navigate to a `[[wikilink]]` target, raised by a click in the
/// preview or a keyboard shortcut in the editor. `force_create` is set when the user
/// held Ctrl (Cmd on macOS) — `app.rs` creates the document if it doesn't exist yet,
/// rather than just reporting "not found".
pub struct WikilinkActivation {
    pub target: String,
    pub force_create: bool,
}
