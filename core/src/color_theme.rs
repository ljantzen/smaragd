//! Helix-style whole-palette color themes — the portable counterpart of the
//! real app's `color_theme.rs`. A theme here is 4 colors (background,
//! foreground, accent, broken-wikilink) as hex strings, not an
//! `egui::Color32` — the frontend applies them as CSS custom properties on
//! `:root` rather than through `egui::Visuals`. Real hex values copied
//! verbatim from the real app's built-in theme table so switching between
//! the two apps doesn't change how a theme actually looks.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorTheme {
    pub id: String,
    pub label: String,
    pub dark: bool,
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub broken_wikilink: String,
}

macro_rules! theme {
    ($id:literal, $label:literal, $dark:literal, $bg:literal, $fg:literal, $accent:literal, $broken:literal) => {
        ColorTheme {
            id: $id.to_string(),
            label: $label.to_string(),
            dark: $dark,
            background: $bg.to_string(),
            foreground: $fg.to_string(),
            accent: $accent.to_string(),
            broken_wikilink: $broken.to_string(),
        }
    };
}

/// The 15 built-in themes, in the real app's own declared order.
pub fn built_in_themes() -> Vec<ColorTheme> {
    vec![
        theme!(
            "gruvbox", "Gruvbox", true, "#282828", "#ebdbb2", "#fe8019", "#fb4934"
        ),
        theme!(
            "gruvbox_light",
            "Gruvbox Light",
            false,
            "#fbf1c7",
            "#3c3836",
            "#af3a03",
            "#9d0006"
        ),
        theme!(
            "dracula", "Dracula", true, "#282a36", "#f8f8f2", "#bd93f9", "#ff5555"
        ),
        theme!(
            "nord", "Nord", true, "#2e3440", "#d8dee9", "#88c0d0", "#bf616a"
        ),
        theme!(
            "nord_light",
            "Nord Light",
            false,
            "#eceff4",
            "#2e3440",
            "#5e81ac",
            "#bf616a"
        ),
        theme!(
            "solarized_dark",
            "Solarized Dark",
            true,
            "#002b36",
            "#93a1a1",
            "#268bd2",
            "#dc322f"
        ),
        theme!(
            "solarized_light",
            "Solarized Light",
            false,
            "#fdf6e3",
            "#586e75",
            "#268bd2",
            "#dc322f"
        ),
        theme!(
            "catppuccin_mocha",
            "Catppuccin Mocha",
            true,
            "#1e1e2e",
            "#cdd6f4",
            "#cba6f7",
            "#f38ba8"
        ),
        theme!(
            "catppuccin_latte",
            "Catppuccin Latte",
            false,
            "#eff1f5",
            "#4c4f69",
            "#8839ef",
            "#d20f39"
        ),
        theme!(
            "onedark", "One Dark", true, "#282c34", "#abb2bf", "#61afef", "#e06c75"
        ),
        theme!(
            "onelight",
            "One Light",
            false,
            "#fafafa",
            "#282c34",
            "#0061ff",
            "#e45649"
        ),
        theme!(
            "tokyonight",
            "Tokyo Night",
            true,
            "#1a1b26",
            "#c0caf5",
            "#7aa2f7",
            "#f7768e"
        ),
        theme!(
            "everforest_dark",
            "Everforest Dark",
            true,
            "#2d353b",
            "#d3c6aa",
            "#a7c080",
            "#e67e80"
        ),
        theme!(
            "everforest_light",
            "Everforest Light",
            false,
            "#fdf6e3",
            "#5c6a72",
            "#8da101",
            "#f85552"
        ),
        theme!(
            "ayu_dark", "Ayu Dark", true, "#0f1419", "#bfbdb6", "#ff8f40", "#f07178"
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_themes_has_fifteen_unique_ids() {
        let themes = built_in_themes();
        assert_eq!(themes.len(), 15);
        let mut ids: Vec<&str> = themes.iter().map(|t| t.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 15, "built-in theme ids must be unique");
    }

    #[test]
    fn every_color_is_a_well_formed_hex_string() {
        for theme in built_in_themes() {
            for color in [
                &theme.background,
                &theme.foreground,
                &theme.accent,
                &theme.broken_wikilink,
            ] {
                assert!(
                    color.starts_with('#') && color.len() == 7,
                    "{} has a malformed color: {color}",
                    theme.id
                );
            }
        }
    }
}
