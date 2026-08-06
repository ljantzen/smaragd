//! Helix-style color themes: a curated set of 15 built-in color schemes, their
//! background/foreground/accent colors verified against Helix's own
//! `runtime/themes/*.toml` sources, plus user-contributed *custom* themes loaded
//! from `.toml` files in [`global_themes_dir`] (see [`load`]). Selectable via
//! `:theme <id>` and the View > Theme menu.
//!
//! Deliberately a separate concept from the `:dmode`/dark-mode-toggle "appearance"
//! switch (see the `dark_mode_vs_theme_naming` convention this codebase follows): a
//! color theme picks a whole palette, the way Helix's own `:theme` command does,
//! layered as an `egui::Visuals` override on top of whichever base (`Dark`/`Light`)
//! the theme itself calls for.
//!
//! Smaragd's editor is a single plain-text `TextEdit` with no tokenizing/syntax
//! highlighting pipeline (unlike Helix itself), so these themes reproduce each
//! palette's overall look — background, body text, and one signature accent color
//! used for selection/links — not full per-token syntax highlighting. A theme can
//! optionally also override the markdown preview's heading/wikilink/quote-bar
//! colors (`preview_heading`/`preview_wikilink`/`preview_quote_bar`) — see
//! `ui::markdown_preview::Palette`; a theme that leaves these `None` renders the
//! preview exactly as it always has.

use std::path::PathBuf;

use egui::Color32;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct ColorTheme {
    /// Canonical id, matched against `:theme <id>` — mirrors the corresponding
    /// Helix theme file's name for built-ins, or a custom theme's own `id` key.
    /// Always lowercase (custom ids are lowercased at load time, matching how
    /// `:theme` itself lowercases its argument), so lookups are a plain `==`.
    pub id: String,
    pub label: String,
    /// Which base (`egui::Theme::Dark`/`Light`) this theme's palette is built for.
    pub dark: bool,
    pub background: Color32,
    pub foreground: Color32,
    pub accent: Color32,
    pub preview_heading: Option<[Color32; 6]>,
    pub preview_wikilink: Option<Color32>,
    pub preview_quote_bar: Option<Color32>,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// A built-in theme's raw color data — kept as a plain `const`-friendly struct
/// (unlike the public, owned `ColorTheme`) purely so the 15 entries below stay a
/// compact literal array; [`built_in_themes`] maps each into a real `ColorTheme`.
struct BuiltIn {
    id: &'static str,
    label: &'static str,
    dark: bool,
    background: Color32,
    foreground: Color32,
    accent: Color32,
}

const BUILT_IN: &[BuiltIn] = &[
    BuiltIn {
        id: "gruvbox",
        label: "Gruvbox",
        dark: true,
        background: rgb(0x28, 0x28, 0x28),
        foreground: rgb(0xeb, 0xdb, 0xb2),
        accent: rgb(0xfe, 0x80, 0x19),
    },
    BuiltIn {
        id: "gruvbox_light",
        label: "Gruvbox Light",
        dark: false,
        background: rgb(0xfb, 0xf1, 0xc7),
        foreground: rgb(0x3c, 0x38, 0x36),
        accent: rgb(0xaf, 0x3a, 0x03),
    },
    BuiltIn {
        id: "dracula",
        label: "Dracula",
        dark: true,
        background: rgb(0x28, 0x2a, 0x36),
        foreground: rgb(0xf8, 0xf8, 0xf2),
        accent: rgb(0xbd, 0x93, 0xf9),
    },
    BuiltIn {
        id: "nord",
        label: "Nord",
        dark: true,
        background: rgb(0x2e, 0x34, 0x40),
        foreground: rgb(0xd8, 0xde, 0xe9),
        accent: rgb(0x88, 0xc0, 0xd0),
    },
    BuiltIn {
        id: "nord_light",
        label: "Nord Light",
        dark: false,
        background: rgb(0xec, 0xef, 0xf4),
        foreground: rgb(0x2e, 0x34, 0x40),
        accent: rgb(0x5e, 0x81, 0xac),
    },
    BuiltIn {
        id: "solarized_dark",
        label: "Solarized Dark",
        dark: true,
        background: rgb(0x00, 0x2b, 0x36),
        foreground: rgb(0x93, 0xa1, 0xa1),
        accent: rgb(0x26, 0x8b, 0xd2),
    },
    BuiltIn {
        id: "solarized_light",
        label: "Solarized Light",
        dark: false,
        background: rgb(0xfd, 0xf6, 0xe3),
        foreground: rgb(0x58, 0x6e, 0x75),
        accent: rgb(0x26, 0x8b, 0xd2),
    },
    BuiltIn {
        id: "catppuccin_mocha",
        label: "Catppuccin Mocha",
        dark: true,
        background: rgb(0x1e, 0x1e, 0x2e),
        foreground: rgb(0xcd, 0xd6, 0xf4),
        accent: rgb(0xcb, 0xa6, 0xf7),
    },
    BuiltIn {
        id: "catppuccin_latte",
        label: "Catppuccin Latte",
        dark: false,
        background: rgb(0xef, 0xf1, 0xf5),
        foreground: rgb(0x4c, 0x4f, 0x69),
        accent: rgb(0x88, 0x39, 0xef),
    },
    BuiltIn {
        id: "onedark",
        label: "One Dark",
        dark: true,
        background: rgb(0x28, 0x2c, 0x34),
        foreground: rgb(0xab, 0xb2, 0xbf),
        accent: rgb(0x61, 0xaf, 0xef),
    },
    BuiltIn {
        id: "onelight",
        label: "One Light",
        dark: false,
        background: rgb(0xfa, 0xfa, 0xfa),
        foreground: rgb(0x28, 0x2c, 0x34),
        accent: rgb(0x00, 0x61, 0xff),
    },
    BuiltIn {
        id: "tokyonight",
        label: "Tokyo Night",
        dark: true,
        background: rgb(0x1a, 0x1b, 0x26),
        foreground: rgb(0xc0, 0xca, 0xf5),
        accent: rgb(0x7a, 0xa2, 0xf7),
    },
    BuiltIn {
        id: "everforest_dark",
        label: "Everforest Dark",
        dark: true,
        background: rgb(0x2d, 0x35, 0x3b),
        foreground: rgb(0xd3, 0xc6, 0xaa),
        accent: rgb(0xa7, 0xc0, 0x80),
    },
    BuiltIn {
        id: "everforest_light",
        label: "Everforest Light",
        dark: false,
        background: rgb(0xfd, 0xf6, 0xe3),
        foreground: rgb(0x5c, 0x6a, 0x72),
        accent: rgb(0x8d, 0xa1, 0x01),
    },
    BuiltIn {
        id: "ayu_dark",
        label: "Ayu Dark",
        dark: true,
        background: rgb(0x0f, 0x14, 0x19),
        foreground: rgb(0xbf, 0xbd, 0xb6),
        accent: rgb(0xff, 0x8f, 0x40),
    },
];

/// The 15 built-in themes, as owned `ColorTheme`s (none with a preview-color
/// override) — the starting point [`load`] appends custom themes onto.
pub fn built_in_themes() -> Vec<ColorTheme> {
    BUILT_IN
        .iter()
        .map(|theme| ColorTheme {
            id: theme.id.to_string(),
            label: theme.label.to_string(),
            dark: theme.dark,
            background: theme.background,
            foreground: theme.foreground,
            accent: theme.accent,
            preview_heading: None,
            preview_wikilink: None,
            preview_quote_bar: None,
        })
        .collect()
}

/// The always-loaded custom-theme directory: `<config_dir>/smaragd/themes`, the
/// same base path `plugins::global_plugins_dir` uses for its own `plugins`
/// subdirectory. `None` if the platform's config directory can't be determined.
pub fn global_themes_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "smaragd").map(|dirs| dirs.config_dir().join("themes"))
}

/// Parse a `"#RRGGBB"` (or `"RRGGBB"`, the `#` is optional) hex color.
/// `pub(crate)` (not just this module's own `RawTheme` parsing) since
/// `project::status_colors` reuses it too, to turn a persisted
/// `ProjectMeta::status_colors` hex string back into a paintable `Color32`.
pub(crate) fn parse_hex_color(s: &str) -> Option<Color32> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color32::from_rgb(r, g, b))
}

/// `parse_hex_color`'s inverse — used by the Metadata dock's status
/// color-picker to turn a user-picked `Color32` into the `"#RRGGBB"` string
/// `ProjectMeta::status_colors` persists.
pub(crate) fn to_hex_string(color: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

// Same red/yellow/green anchors as `ui::streak_panel::status_color`, for
// visual consistency with that feature's own traffic-light vocabulary —
// duplicated as local consts rather than imported, since this module sits
// below `ui/` in the dependency graph and can't import from it.
const PROGRESS_RED: Color32 = Color32::from_rgb(200, 60, 60);
const PROGRESS_YELLOW: Color32 = Color32::from_rgb(230, 180, 30);
const PROGRESS_GREEN: Color32 = Color32::from_rgb(60, 180, 75);

/// A red→yellow→green gradient for `BinderColorMode::WordCountProgress` —
/// `fraction` is `word_count as f32 / target as f32`, clamped to `[0.0, 1.0]`
/// before interpolating: pure red at 0%, yellow at 50%, pure green at 100%
/// *and beyond* (a row past its target reads as "done," not as something
/// that keeps escalating the further over it goes). Whether a target is even
/// set at all (`None`/`0` meaning "no color") is the caller's job — this
/// only ever receives an already-valid fraction.
pub(crate) fn word_count_progress_color(fraction: f32) -> Color32 {
    let fraction = fraction.clamp(0.0, 1.0);
    let (from, to, t) = if fraction < 0.5 {
        (PROGRESS_RED, PROGRESS_YELLOW, fraction / 0.5)
    } else {
        (PROGRESS_YELLOW, PROGRESS_GREEN, (fraction - 0.5) / 0.5)
    };
    Color32::from_rgb(
        lerp_u8(from.r(), to.r(), t),
        lerp_u8(from.g(), to.g(), t),
        lerp_u8(from.b(), to.b(), t),
    )
}

fn lerp_u8(from: u8, to: u8, t: f32) -> u8 {
    (from as f32 + (to as f32 - from as f32) * t).round() as u8
}

/// The `[preview]` table of a custom theme's TOML file — every field optional,
/// matching `ColorTheme::preview_*`.
#[derive(Deserialize, Default)]
struct RawPreview {
    #[serde(default)]
    heading: Option<[String; 6]>,
    #[serde(default)]
    wikilink: Option<String>,
    #[serde(default)]
    quote_bar: Option<String>,
}

/// The on-disk shape of a custom theme `.toml` file, with colors still as hex
/// strings — converted (and validated) into a real `ColorTheme` by
/// [`RawTheme::into_theme`].
#[derive(Deserialize)]
struct RawTheme {
    id: String,
    label: String,
    dark: bool,
    background: String,
    foreground: String,
    accent: String,
    #[serde(default)]
    preview: RawPreview,
}

impl RawTheme {
    fn into_theme(self) -> Result<ColorTheme, String> {
        let parse = |field: &str, value: &str| {
            parse_hex_color(value).ok_or_else(|| format!("invalid color for {field}: {value:?}"))
        };
        let preview_heading = match self.preview.heading {
            Some(hexes) => {
                let mut colors = [Color32::BLACK; 6];
                for (i, hex) in hexes.iter().enumerate() {
                    colors[i] = parse("preview.heading", hex)?;
                }
                Some(colors)
            }
            None => None,
        };
        let preview_wikilink = self
            .preview
            .wikilink
            .as_deref()
            .map(|hex| parse("preview.wikilink", hex))
            .transpose()?;
        let preview_quote_bar = self
            .preview
            .quote_bar
            .as_deref()
            .map(|hex| parse("preview.quote_bar", hex))
            .transpose()?;

        Ok(ColorTheme {
            id: self.id.to_lowercase(),
            label: self.label,
            dark: self.dark,
            background: parse("background", &self.background)?,
            foreground: parse("foreground", &self.foreground)?,
            accent: parse("accent", &self.accent)?,
            preview_heading,
            preview_wikilink,
            preview_quote_bar,
        })
    }
}

/// Load every theme: the 15 built-ins, plus every `*.toml` file directly inside
/// each of `dirs` (flat, not recursive; a missing directory is silently skipped,
/// not an error — same shape and tolerance as `plugins::load`). Directories are
/// scanned in the order given and files within one in sorted-name order, so load
/// — and therefore id-collision resolution — is deterministic. In practice always
/// called as `load(&[global_themes_dir()...])`; taking the directory list as a
/// parameter (rather than resolving it internally) keeps this unit-testable
/// against a real temp directory.
///
/// Never fails outright: a file that doesn't parse, has an invalid color, or
/// whose `id` collides with an already-loaded theme (a built-in, or an earlier
/// custom one — first loaded wins) is skipped, with a message describing why
/// appended to the returned list, rather than the whole load failing.
pub fn load(dirs: &[&std::path::Path]) -> (Vec<ColorTheme>, Vec<String>) {
    let mut themes = built_in_themes();
    let mut errors = Vec::new();

    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };

        let mut paths: Vec<_> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
            .collect();
        paths.sort();

        for path in paths {
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("theme")
                .to_string();

            let source = match std::fs::read_to_string(&path) {
                Ok(source) => source,
                Err(err) => {
                    errors.push(format!("{name}: couldn't read file: {err}"));
                    continue;
                }
            };
            let raw: RawTheme = match toml::from_str(&source) {
                Ok(raw) => raw,
                Err(err) => {
                    errors.push(format!("{name}: {err}"));
                    continue;
                }
            };
            let theme = match raw.into_theme() {
                Ok(theme) => theme,
                Err(err) => {
                    errors.push(format!("{name}: {err}"));
                    continue;
                }
            };
            if let Some(existing) = themes.iter().find(|t| t.id == theme.id) {
                errors.push(format!(
                    "{name}: theme id \"{}\" is already used by \"{}\", skipping",
                    theme.id, existing.label
                ));
                continue;
            }
            themes.push(theme);
        }
    }

    (themes, errors)
}

pub fn find<'a>(themes: &'a [ColorTheme], id: &str) -> Option<&'a ColorTheme> {
    themes.iter().find(|theme| theme.id == id)
}

/// Give inactive text-input widgets (any `TextEdit`) a always-visible outline,
/// matching `noninteractive`'s separator/window-border stroke, instead of egui's
/// default `inactive.bg_stroke` (zero-width, so the frame only appears once the
/// mouse hovers it). Called after anything that (re)sets `visuals.widgets`.
pub fn show_input_frame(visuals: &mut egui::Visuals) {
    visuals.widgets.inactive.bg_stroke = visuals.widgets.noninteractive.bg_stroke;
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
        show_input_frame(visuals);
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
            show_input_frame(visuals);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn there_are_fifteen_built_in_themes() {
        assert_eq!(built_in_themes().len(), 15);
    }

    #[test]
    fn built_in_theme_ids_are_unique() {
        let themes = built_in_themes();
        let ids: HashSet<&str> = themes.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids.len(), themes.len());
    }

    #[test]
    fn find_locates_a_known_theme() {
        let themes = built_in_themes();
        let theme = find(&themes, "dracula").unwrap();
        assert_eq!(theme.label, "Dracula");
        assert!(theme.dark);
    }

    #[test]
    fn find_returns_none_for_an_unknown_id() {
        let themes = built_in_themes();
        assert!(find(&themes, "not-a-real-theme").is_none());
    }

    #[test]
    fn both_dark_and_light_built_in_themes_are_represented() {
        let themes = built_in_themes();
        assert!(themes.iter().any(|t| t.dark));
        assert!(themes.iter().any(|t| !t.dark));
    }

    #[test]
    fn parse_hex_color_accepts_a_leading_hash() {
        assert_eq!(
            parse_hex_color("#ff8800"),
            Some(Color32::from_rgb(0xff, 0x88, 0x00))
        );
    }

    #[test]
    fn parse_hex_color_accepts_no_leading_hash() {
        assert_eq!(
            parse_hex_color("ff8800"),
            Some(Color32::from_rgb(0xff, 0x88, 0x00))
        );
    }

    #[test]
    fn parse_hex_color_rejects_the_wrong_length() {
        assert_eq!(parse_hex_color("#fff"), None);
        assert_eq!(parse_hex_color("#ff88000"), None);
    }

    #[test]
    fn parse_hex_color_rejects_non_hex_characters() {
        assert_eq!(parse_hex_color("#zzzzzz"), None);
    }

    #[test]
    fn word_count_progress_color_at_zero_is_red() {
        assert_eq!(word_count_progress_color(0.0), PROGRESS_RED);
    }

    #[test]
    fn word_count_progress_color_at_fifty_percent_is_yellow() {
        assert_eq!(word_count_progress_color(0.5), PROGRESS_YELLOW);
    }

    #[test]
    fn word_count_progress_color_at_full_is_green() {
        assert_eq!(word_count_progress_color(1.0), PROGRESS_GREEN);
    }

    #[test]
    fn word_count_progress_color_clamps_past_full_to_green() {
        assert_eq!(word_count_progress_color(2.5), PROGRESS_GREEN);
    }

    #[test]
    fn word_count_progress_color_clamps_negative_to_red() {
        assert_eq!(word_count_progress_color(-1.0), PROGRESS_RED);
    }

    fn write_theme(dir: &std::path::Path, filename: &str, contents: &str) {
        std::fs::write(dir.join(filename), contents).unwrap();
    }

    #[test]
    fn load_is_just_the_built_ins_when_no_custom_theme_files_exist() {
        let dir = tempfile::tempdir().unwrap();
        let (themes, errors) = load(&[dir.path()]);
        assert!(errors.is_empty());
        assert_eq!(themes.len(), 15);
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        let (themes, errors) = load(&[std::path::Path::new("/does/not/exist")]);
        assert!(errors.is_empty());
        assert_eq!(themes.len(), 15);
    }

    #[test]
    fn load_picks_up_a_valid_custom_theme_file() {
        let dir = tempfile::tempdir().unwrap();
        write_theme(
            dir.path(),
            "my_theme.toml",
            r##"
                id = "my_theme"
                label = "My Theme"
                dark = true
                background = "#1e1e2e"
                foreground = "#cdd6f4"
                accent = "#cba6f7"
            "##,
        );
        let (themes, errors) = load(&[dir.path()]);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(themes.len(), 16);
        assert!(find(&themes, "my_theme").is_some());
    }

    #[test]
    fn a_custom_theme_id_colliding_with_a_built_in_is_skipped_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        write_theme(
            dir.path(),
            "fake_dracula.toml",
            r##"
                id = "dracula"
                label = "Fake Dracula"
                dark = true
                background = "#000000"
                foreground = "#ffffff"
                accent = "#ffffff"
            "##,
        );
        let (themes, errors) = load(&[dir.path()]);
        assert_eq!(themes.len(), 15);
        assert!(errors.iter().any(|e| e.contains("already used")));
        // The real built-in survives untouched.
        assert_eq!(find(&themes, "dracula").unwrap().label, "Dracula");
    }

    #[test]
    fn two_custom_themes_racing_for_the_same_id_keeps_the_first() {
        let dir = tempfile::tempdir().unwrap();
        write_theme(
            dir.path(),
            "a_first.toml",
            r##"
                id = "dup"
                label = "First"
                dark = true
                background = "#000000"
                foreground = "#ffffff"
                accent = "#ffffff"
            "##,
        );
        write_theme(
            dir.path(),
            "b_second.toml",
            r##"
                id = "dup"
                label = "Second"
                dark = true
                background = "#000000"
                foreground = "#ffffff"
                accent = "#ffffff"
            "##,
        );
        let (themes, errors) = load(&[dir.path()]);
        assert!(errors.iter().any(|e| e.contains("already used")));
        assert_eq!(find(&themes, "dup").unwrap().label, "First");
    }

    #[test]
    fn a_malformed_theme_file_is_skipped_and_does_not_prevent_others_loading() {
        let dir = tempfile::tempdir().unwrap();
        write_theme(dir.path(), "broken.toml", "not = [valid");
        write_theme(
            dir.path(),
            "fine.toml",
            r##"
                id = "fine"
                label = "Fine"
                dark = true
                background = "#000000"
                foreground = "#ffffff"
                accent = "#ffffff"
            "##,
        );
        let (themes, errors) = load(&[dir.path()]);
        assert!(errors.iter().any(|e| e.starts_with("broken:")));
        assert!(find(&themes, "fine").is_some());
    }

    #[test]
    fn a_custom_theme_without_a_preview_table_has_no_overrides() {
        let dir = tempfile::tempdir().unwrap();
        write_theme(
            dir.path(),
            "custom.toml",
            r##"
                id = "my_theme"
                label = "My Theme"
                dark = true
                background = "#1e1e2e"
                foreground = "#cdd6f4"
                accent = "#cba6f7"
            "##,
        );
        let raw: RawTheme =
            toml::from_str(&std::fs::read_to_string(dir.path().join("custom.toml")).unwrap())
                .unwrap();
        let theme = raw.into_theme().unwrap();
        assert_eq!(theme.id, "my_theme");
        assert_eq!(theme.background, Color32::from_rgb(0x1e, 0x1e, 0x2e));
        assert!(theme.preview_heading.is_none());
        assert!(theme.preview_wikilink.is_none());
        assert!(theme.preview_quote_bar.is_none());
    }

    #[test]
    fn a_custom_theme_with_a_preview_table_has_overrides() {
        let source = r##"
            id = "My_Theme"
            label = "My Theme"
            dark = true
            background = "#1e1e2e"
            foreground = "#cdd6f4"
            accent = "#cba6f7"

            [preview]
            heading = ["#f38ba8", "#89b4fa", "#a6e3a1", "#cba6f7", "#f9e2af", "#fab387"]
            wikilink = "#a6e3a1"
            quote_bar = "#6c7086"
        "##;
        let raw: RawTheme = toml::from_str(source).unwrap();
        let theme = raw.into_theme().unwrap();
        // Ids are lowercased, matching how `:theme` lowercases its argument.
        assert_eq!(theme.id, "my_theme");
        assert_eq!(
            theme.preview_heading,
            Some([
                Color32::from_rgb(0xf3, 0x8b, 0xa8),
                Color32::from_rgb(0x89, 0xb4, 0xfa),
                Color32::from_rgb(0xa6, 0xe3, 0xa1),
                Color32::from_rgb(0xcb, 0xa6, 0xf7),
                Color32::from_rgb(0xf9, 0xe2, 0xaf),
                Color32::from_rgb(0xfa, 0xb3, 0x87),
            ])
        );
        assert_eq!(
            theme.preview_wikilink,
            Some(Color32::from_rgb(0xa6, 0xe3, 0xa1))
        );
        assert_eq!(
            theme.preview_quote_bar,
            Some(Color32::from_rgb(0x6c, 0x70, 0x86))
        );
    }

    #[test]
    fn an_invalid_color_is_rejected() {
        let source = r##"
            id = "bad"
            label = "Bad"
            dark = true
            background = "not-a-color"
            foreground = "#ffffff"
            accent = "#ffffff"
        "##;
        let raw: RawTheme = toml::from_str(source).unwrap();
        assert!(raw.into_theme().is_err());
    }
}
