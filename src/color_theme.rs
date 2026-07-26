//! Helix-style color themes: a curated set of 15 popular color schemes, their
//! background/foreground/accent colors verified against Helix's own
//! `runtime/themes/*.toml` sources. Selectable via `:theme <id>` and the View > Theme
//! menu.
//!
//! Deliberately a separate concept from the `:dmode`/dark-mode-toggle "appearance"
//! switch (see the `dark_mode_vs_theme_naming` convention this codebase follows): a
//! color theme picks a whole palette, the way Helix's own `:theme` command does,
//! layered as an `egui::Visuals` override on top of whichever base (`Dark`/`Light`)
//! the theme itself calls for.
//!
//! Tachylite's editor is a single plain-text `TextEdit` with no tokenizing/syntax
//! highlighting pipeline (unlike Helix itself), so these themes reproduce each
//! palette's overall look — background, body text, and one signature accent color
//! used for selection/links — not full per-token syntax highlighting.

use egui::Color32;

pub struct ColorTheme {
    /// Canonical id, matched against `:theme <id>` — mirrors the corresponding Helix
    /// theme file's name (`runtime/themes/<id>.toml`).
    pub id: &'static str,
    pub label: &'static str,
    /// Which base (`egui::Theme::Dark`/`Light`) this theme's palette is built for.
    pub dark: bool,
    pub background: Color32,
    pub foreground: Color32,
    pub accent: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const THEMES: &[ColorTheme] = &[
    ColorTheme {
        id: "gruvbox",
        label: "Gruvbox",
        dark: true,
        background: rgb(0x28, 0x28, 0x28),
        foreground: rgb(0xeb, 0xdb, 0xb2),
        accent: rgb(0xfe, 0x80, 0x19),
    },
    ColorTheme {
        id: "gruvbox_light",
        label: "Gruvbox Light",
        dark: false,
        background: rgb(0xfb, 0xf1, 0xc7),
        foreground: rgb(0x3c, 0x38, 0x36),
        accent: rgb(0xaf, 0x3a, 0x03),
    },
    ColorTheme {
        id: "dracula",
        label: "Dracula",
        dark: true,
        background: rgb(0x28, 0x2a, 0x36),
        foreground: rgb(0xf8, 0xf8, 0xf2),
        accent: rgb(0xbd, 0x93, 0xf9),
    },
    ColorTheme {
        id: "nord",
        label: "Nord",
        dark: true,
        background: rgb(0x2e, 0x34, 0x40),
        foreground: rgb(0xd8, 0xde, 0xe9),
        accent: rgb(0x88, 0xc0, 0xd0),
    },
    ColorTheme {
        id: "nord_light",
        label: "Nord Light",
        dark: false,
        background: rgb(0xec, 0xef, 0xf4),
        foreground: rgb(0x2e, 0x34, 0x40),
        accent: rgb(0x5e, 0x81, 0xac),
    },
    ColorTheme {
        id: "solarized_dark",
        label: "Solarized Dark",
        dark: true,
        background: rgb(0x00, 0x2b, 0x36),
        foreground: rgb(0x93, 0xa1, 0xa1),
        accent: rgb(0x26, 0x8b, 0xd2),
    },
    ColorTheme {
        id: "solarized_light",
        label: "Solarized Light",
        dark: false,
        background: rgb(0xfd, 0xf6, 0xe3),
        foreground: rgb(0x58, 0x6e, 0x75),
        accent: rgb(0x26, 0x8b, 0xd2),
    },
    ColorTheme {
        id: "catppuccin_mocha",
        label: "Catppuccin Mocha",
        dark: true,
        background: rgb(0x1e, 0x1e, 0x2e),
        foreground: rgb(0xcd, 0xd6, 0xf4),
        accent: rgb(0xcb, 0xa6, 0xf7),
    },
    ColorTheme {
        id: "catppuccin_latte",
        label: "Catppuccin Latte",
        dark: false,
        background: rgb(0xef, 0xf1, 0xf5),
        foreground: rgb(0x4c, 0x4f, 0x69),
        accent: rgb(0x88, 0x39, 0xef),
    },
    ColorTheme {
        id: "onedark",
        label: "One Dark",
        dark: true,
        background: rgb(0x28, 0x2c, 0x34),
        foreground: rgb(0xab, 0xb2, 0xbf),
        accent: rgb(0x61, 0xaf, 0xef),
    },
    ColorTheme {
        id: "onelight",
        label: "One Light",
        dark: false,
        background: rgb(0xfa, 0xfa, 0xfa),
        foreground: rgb(0x28, 0x2c, 0x34),
        accent: rgb(0x00, 0x61, 0xff),
    },
    ColorTheme {
        id: "tokyonight",
        label: "Tokyo Night",
        dark: true,
        background: rgb(0x1a, 0x1b, 0x26),
        foreground: rgb(0xc0, 0xca, 0xf5),
        accent: rgb(0x7a, 0xa2, 0xf7),
    },
    ColorTheme {
        id: "everforest_dark",
        label: "Everforest Dark",
        dark: true,
        background: rgb(0x2d, 0x35, 0x3b),
        foreground: rgb(0xd3, 0xc6, 0xaa),
        accent: rgb(0xa7, 0xc0, 0x80),
    },
    ColorTheme {
        id: "everforest_light",
        label: "Everforest Light",
        dark: false,
        background: rgb(0xfd, 0xf6, 0xe3),
        foreground: rgb(0x5c, 0x6a, 0x72),
        accent: rgb(0x8d, 0xa1, 0x01),
    },
    ColorTheme {
        id: "ayu_dark",
        label: "Ayu Dark",
        dark: true,
        background: rgb(0x0f, 0x14, 0x19),
        foreground: rgb(0xbf, 0xbd, 0xb6),
        accent: rgb(0xff, 0x8f, 0x40),
    },
];

/// `THEMES`' ids, in the same order — kept as a flat, directly-completable list for
/// `ui/command_prompt.rs`'s `:theme` argument autocomplete (guarded against drifting
/// out of sync with `THEMES` by a test below).
pub const THEME_IDS: &[&str] = &[
    "gruvbox",
    "gruvbox_light",
    "dracula",
    "nord",
    "nord_light",
    "solarized_dark",
    "solarized_light",
    "catppuccin_mocha",
    "catppuccin_latte",
    "onedark",
    "onelight",
    "tokyonight",
    "everforest_dark",
    "everforest_light",
    "ayu_dark",
];

pub fn find(id: &str) -> Option<&'static ColorTheme> {
    THEMES.iter().find(|theme| theme.id == id)
}

/// Apply `theme`'s palette on top of whichever base (`Dark`/`Light`) it's built for,
/// and switch the active `egui::Theme` to match — so everything this doesn't
/// explicitly override (button hover/press states, etc.) still comes from a base
/// that's appropriately light or dark for the theme.
pub fn apply(ctx: &egui::Context, theme: &ColorTheme) {
    let base = if theme.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(base);
    ctx.style_mut_of(base, |style| {
        let visuals = &mut style.visuals;
        visuals.panel_fill = theme.background;
        visuals.window_fill = theme.background;
        visuals.extreme_bg_color = theme.background;
        visuals.text_edit_bg_color = Some(theme.background);
        visuals.override_text_color = Some(theme.foreground);
        visuals.hyperlink_color = theme.accent;
        visuals.selection.bg_fill = theme.accent;
        for widgets in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widgets.fg_stroke.color = theme.foreground;
        }
    });
}

/// Undo `apply`'s overrides for both bases, back to each's plain `egui::Visuals`
/// defaults — reverting to "no color theme" (plain `:dmode` dark/light styling).
pub fn reset(ctx: &egui::Context) {
    for base in [egui::Theme::Dark, egui::Theme::Light] {
        let defaults = if base == egui::Theme::Dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        ctx.style_mut_of(base, |style| {
            let visuals = &mut style.visuals;
            visuals.panel_fill = defaults.panel_fill;
            visuals.window_fill = defaults.window_fill;
            visuals.extreme_bg_color = defaults.extreme_bg_color;
            // Matches the app-startup sync in `app.rs` that keeps the editor
            // background from looking darker than the surrounding chrome.
            visuals.text_edit_bg_color = Some(defaults.panel_fill);
            visuals.override_text_color = None;
            visuals.hyperlink_color = defaults.hyperlink_color;
            visuals.selection = defaults.selection;
            visuals.widgets = defaults.widgets;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn there_are_fifteen_themes() {
        assert_eq!(THEMES.len(), 15);
    }

    #[test]
    fn theme_ids_are_unique() {
        let ids: HashSet<&str> = THEMES.iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), THEMES.len());
    }

    #[test]
    fn theme_ids_matches_themes_in_the_same_order() {
        assert_eq!(THEME_IDS.len(), THEMES.len());
        for (id, theme) in THEME_IDS.iter().zip(THEMES.iter()) {
            assert_eq!(*id, theme.id);
        }
    }

    #[test]
    fn find_locates_a_known_theme() {
        let theme = find("dracula").unwrap();
        assert_eq!(theme.label, "Dracula");
        assert!(theme.dark);
    }

    #[test]
    fn find_returns_none_for_an_unknown_id() {
        assert!(find("not-a-real-theme").is_none());
    }

    #[test]
    fn both_dark_and_light_themes_are_represented() {
        assert!(THEMES.iter().any(|t| t.dark));
        assert!(THEMES.iter().any(|t| !t.dark));
    }
}
