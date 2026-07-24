//! Remappable keyboard shortcuts: which app operation each `egui::KeyboardShortcut`
//! triggers, persisted as part of `Settings`.

use std::collections::BTreeMap;

use egui::{Key, KeyboardShortcut, Modifiers};
use serde::{Deserialize, Serialize};

/// An app operation that can be triggered by a keyboard shortcut. Split into
/// operations with a well-defined global scope, and ones that act on whatever
/// document is currently selected in the binder (`TachyliteApp::selected_path`).
/// Folder-only operations (Folder Role, Empty Trash) aren't included: the app has no
/// concept of a "selected folder" today (only right-click targets one directly), and
/// inventing one just for two rare shortcuts is out of scope here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    NewProject,
    OpenProject,
    OpenSettings,
    Exit,
    TogglePreview,
    Save,
    NewFile,
    NewFolder,
    Rename,
    Delete,
    Restore,
}

impl ShortcutAction {
    pub const ALL: &'static [ShortcutAction] = &[
        Self::NewProject,
        Self::OpenProject,
        Self::OpenSettings,
        Self::Exit,
        Self::TogglePreview,
        Self::Save,
        Self::NewFile,
        Self::NewFolder,
        Self::Rename,
        Self::Delete,
        Self::Restore,
    ];

    /// Display label shown in the menu bar and the shortcuts settings list.
    pub fn label(&self) -> &'static str {
        match self {
            Self::NewProject => "New Project",
            Self::OpenProject => "Open Project",
            Self::OpenSettings => "Settings",
            Self::Exit => "Exit",
            Self::TogglePreview => "Toggle Preview",
            Self::Save => "Save",
            Self::NewFile => "New File",
            Self::NewFolder => "New Folder",
            Self::Rename => "Rename",
            Self::Delete => "Delete",
            Self::Restore => "Restore",
        }
    }

    /// Stable identifier used as the TOML key for this action, independent of enum
    /// declaration order or the display label (which may change).
    fn id(&self) -> &'static str {
        match self {
            Self::NewProject => "new_project",
            Self::OpenProject => "open_project",
            Self::OpenSettings => "open_settings",
            Self::Exit => "exit",
            Self::TogglePreview => "toggle_preview",
            Self::Save => "save",
            Self::NewFile => "new_file",
            Self::NewFolder => "new_folder",
            Self::Rename => "rename",
            Self::Delete => "delete",
            Self::Restore => "restore",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|action| action.id() == id)
    }

    /// The keybinding this action starts out with in a fresh `ShortcutMap`. All use
    /// `Modifiers::COMMAND` (Ctrl on Windows/Linux, Cmd on Mac) rather than raw
    /// `Modifiers::CTRL`, matching the app's one pre-existing shortcut (Ctrl+S).
    /// Deliberately avoids bare Delete/Backspace or Ctrl+Backspace for `Delete`:
    /// shortcuts are consumed unconditionally regardless of focus (see
    /// `sorted_by_specificity`'s doc comment), so a default that overlaps a normal
    /// text-editing chord (forward-delete, delete-word-backward) would silently
    /// break typing in the editor.
    pub fn default_shortcut(&self) -> KeyboardShortcut {
        match self {
            Self::NewProject => KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::ALT, Key::N),
            Self::OpenProject => KeyboardShortcut::new(Modifiers::COMMAND, Key::O),
            Self::OpenSettings => KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma),
            Self::Exit => KeyboardShortcut::new(Modifiers::COMMAND, Key::Q),
            Self::TogglePreview => {
                KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::P)
            }
            Self::Save => KeyboardShortcut::new(Modifiers::COMMAND, Key::S),
            Self::NewFile => KeyboardShortcut::new(Modifiers::COMMAND, Key::N),
            Self::NewFolder => KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::F),
            Self::Rename => KeyboardShortcut::new(Modifiers::NONE, Key::F2),
            Self::Delete => {
                KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Backspace)
            }
            Self::Restore => KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::R),
        }
    }
}

/// True for keys that never correspond to a typed character (function keys and
/// Escape) — safe to bind without requiring a modifier, since doing so can't make
/// any character impossible to type.
pub fn is_modifier_free_safe_key(key: Key) -> bool {
    matches!(
        key,
        Key::Escape
            | Key::F1
            | Key::F2
            | Key::F3
            | Key::F4
            | Key::F5
            | Key::F6
            | Key::F7
            | Key::F8
            | Key::F9
            | Key::F10
            | Key::F11
            | Key::F12
            | Key::F13
            | Key::F14
            | Key::F15
            | Key::F16
            | Key::F17
            | Key::F18
            | Key::F19
            | Key::F20
            | Key::F21
            | Key::F22
            | Key::F23
            | Key::F24
            | Key::F25
            | Key::F26
            | Key::F27
            | Key::F28
            | Key::F29
            | Key::F30
            | Key::F31
            | Key::F32
            | Key::F33
            | Key::F34
            | Key::F35
    )
}

/// Whether `shortcut` is safe to bind: it must either require at least one modifier,
/// or use a key from `is_modifier_free_safe_key` — otherwise binding it would make
/// some character impossible to type anywhere in the app.
pub fn is_safe_binding(shortcut: &KeyboardShortcut) -> bool {
    !shortcut.modifiers.is_none() || is_modifier_free_safe_key(shortcut.logical_key)
}

/// How many modifier keys `shortcut` requires — used to check the most specific
/// shortcuts first each frame. egui's `consume_shortcut` ignores *extra* Shift/Alt
/// modifiers on a pattern that doesn't require them (e.g. an Alt-less "Ctrl+N"
/// pattern still matches a Ctrl+Alt+N press), so a less-specific shortcut checked
/// first can silently swallow a more-specific one's key combo — egui's own docs on
/// `InputState::consume_shortcut` warn to check the most specific shortcuts first.
fn specificity(shortcut: &KeyboardShortcut) -> u32 {
    let m = shortcut.modifiers;
    m.alt as u32 + m.shift as u32 + m.ctrl as u32 + m.mac_cmd as u32 + m.command as u32
}

/// Order `(action, shortcut)` pairs by descending modifier specificity, so the
/// per-frame consumption loop checks the most specific shortcuts first and a
/// less-specific shortcut can never swallow a more-specific one's keypress.
pub fn sorted_by_specificity(
    mut pairs: Vec<(ShortcutAction, KeyboardShortcut)>,
) -> Vec<(ShortcutAction, KeyboardShortcut)> {
    pairs.sort_by_key(|(_, shortcut)| std::cmp::Reverse(specificity(shortcut)));
    pairs
}

/// Persisted action -> shortcut bindings. Keyed by `ShortcutAction::id()` (a stable
/// string), not the enum directly, so the on-disk TOML is human-readable and doesn't
/// depend on how serde happens to encode enum-variant map keys through the `toml`
/// crate. An action absent from the map is unbound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShortcutMap(BTreeMap<String, KeyboardShortcut>);

impl Default for ShortcutMap {
    fn default() -> Self {
        let map = ShortcutAction::ALL
            .iter()
            .map(|action| (action.id().to_string(), action.default_shortcut()))
            .collect();
        Self(map)
    }
}

impl ShortcutMap {
    pub fn get(&self, action: ShortcutAction) -> Option<KeyboardShortcut> {
        self.0.get(action.id()).copied()
    }

    /// Bind `action` to `shortcut`, first clearing that exact combo from whichever
    /// other action currently holds it — keeps bindings 1:1 so two actions can never
    /// silently race for the same keypress. `None` unbinds `action` outright.
    pub fn set(&mut self, action: ShortcutAction, shortcut: Option<KeyboardShortcut>) {
        if let Some(shortcut) = shortcut {
            let previous_owner = self
                .0
                .iter()
                .find(|(id, bound)| **bound == shortcut && *id != action.id())
                .map(|(id, _)| id.clone());
            if let Some(id) = previous_owner {
                self.0.remove(&id);
            }
            self.0.insert(action.id().to_string(), shortcut);
        } else {
            self.0.remove(action.id());
        }
    }

    /// All currently bound actions, ready for the per-frame consumption loop.
    pub fn bindings(&self) -> Vec<(ShortcutAction, KeyboardShortcut)> {
        self.0
            .iter()
            .filter_map(|(id, shortcut)| {
                ShortcutAction::from_id(id).map(|action| (action, *shortcut))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_default_binding() {
        let map = ShortcutMap::default();
        for action in ShortcutAction::ALL {
            assert!(
                map.get(*action).is_some(),
                "{} has no default",
                action.label()
            );
        }
    }

    #[test]
    fn default_bindings_are_all_safe() {
        for action in ShortcutAction::ALL {
            assert!(
                is_safe_binding(&action.default_shortcut()),
                "{}'s default isn't safe to bind unconditionally",
                action.label()
            );
        }
    }

    #[test]
    fn set_binds_and_get_returns_it() {
        let mut map = ShortcutMap::default();
        let shortcut = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        map.set(ShortcutAction::Save, Some(shortcut));
        assert_eq!(map.get(ShortcutAction::Save), Some(shortcut));
    }

    #[test]
    fn set_none_unbinds() {
        let mut map = ShortcutMap::default();
        map.set(ShortcutAction::Save, None);
        assert_eq!(map.get(ShortcutAction::Save), None);
    }

    #[test]
    fn set_clears_the_shortcut_from_its_previous_owner() {
        let mut map = ShortcutMap::default();
        let open_project_shortcut = map.get(ShortcutAction::OpenProject).unwrap();

        map.set(ShortcutAction::Save, Some(open_project_shortcut));

        assert_eq!(map.get(ShortcutAction::Save), Some(open_project_shortcut));
        assert_eq!(map.get(ShortcutAction::OpenProject), None);
    }

    #[test]
    fn set_reassigning_an_action_to_itself_does_not_unbind_it() {
        let mut map = ShortcutMap::default();
        let shortcut = map.get(ShortcutAction::Save).unwrap();

        map.set(ShortcutAction::Save, Some(shortcut));

        assert_eq!(map.get(ShortcutAction::Save), Some(shortcut));
    }

    #[test]
    fn unbound_action_has_no_binding() {
        let map = ShortcutMap(BTreeMap::new());
        assert_eq!(map.get(ShortcutAction::Save), None);
    }

    #[test]
    fn bindings_round_trip_through_toml() {
        let map = ShortcutMap::default();
        let serialized = toml::to_string(&map).unwrap();
        let deserialized: ShortcutMap = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized, map);
    }

    #[test]
    fn sorted_by_specificity_puts_more_modifiers_first() {
        let less_specific = (
            ShortcutAction::NewFile,
            KeyboardShortcut::new(Modifiers::COMMAND, Key::N),
        );
        let more_specific = (
            ShortcutAction::NewProject,
            KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::ALT, Key::N),
        );

        let sorted = sorted_by_specificity(vec![less_specific, more_specific]);

        assert_eq!(sorted[0].0, ShortcutAction::NewProject);
        assert_eq!(sorted[1].0, ShortcutAction::NewFile);
    }

    #[test]
    fn is_safe_binding_rejects_bare_printable_keys() {
        assert!(!is_safe_binding(&KeyboardShortcut::new(
            Modifiers::NONE,
            Key::R
        )));
    }

    #[test]
    fn is_safe_binding_accepts_function_keys_and_escape_without_a_modifier() {
        assert!(is_safe_binding(&KeyboardShortcut::new(
            Modifiers::NONE,
            Key::F2
        )));
        assert!(is_safe_binding(&KeyboardShortcut::new(
            Modifiers::NONE,
            Key::Escape
        )));
    }

    #[test]
    fn is_safe_binding_accepts_any_key_with_a_modifier() {
        assert!(is_safe_binding(&KeyboardShortcut::new(
            Modifiers::COMMAND,
            Key::R
        )));
    }
}
