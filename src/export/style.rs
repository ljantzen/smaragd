//! Typesetting styles: the shared config model that drives DOCX, EPUB, and PDF
//! export together — a `TypesetStyle` picks fonts/sizes/page setup/running
//! headers/drop caps once, and each renderer (`export::docx`/`export::epub`/
//! `export::pdf`) reads from it instead of hardcoding its own literals.
//!
//! Modeled directly on `color_theme.rs`: 2 built-in presets, plus user-authored
//! `.toml` files dropped into [`global_styles_dir`] — same files-only authoring,
//! tolerant `load()`, and `find()`-by-id shape, no in-app style editor.
//!
//! Built-in presets default to "Libertinus Serif" (body) and "DejaVu Sans Mono"
//! (code) — not "Times New Roman"/"Courier New" — because those two are what
//! `export::pdf` can actually guarantee are available: bundled via
//! `typst-kit`'s embedded fonts (from `typst-assets`), so the PDF renderer
//! never silently substitutes an arbitrary fallback font. DOCX/EPUB treat the
//! same names as plain font *references* (Word/an e-reader substitutes if the
//! font isn't locally installed, same as any other font name) — one style
//! genuinely drives all three outputs with the same fonts, rather than PDF
//! needing a different "safe" font from DOCX/EPUB.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct TypesetStyle {
    pub id: String,
    pub label: String,
    pub page: PageStyle,
    pub body: BodyStyle,
    pub headings: HeadingStyle,
    pub blockquote: BlockStyle,
    pub code: BlockStyle,
    pub drop_cap: Option<DropCapStyle>,
    pub running_header: Option<RunningHeaderStyle>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageStyle {
    pub width_mm: f32,
    pub height_mm: f32,
    pub margin_mm: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BodyStyle {
    pub font: String,
    pub size_pt: u32,
    /// Multiple of the font size, e.g. `2.0` for a manuscript's traditional
    /// double-spacing, `1.15`-ish for a trade paperback's tighter book leading.
    pub line_height: f32,
    pub justify: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeadingStyle {
    pub font: String,
    /// Point size per heading level 1-6, in that order.
    pub sizes_pt: [u32; 6],
    pub page_break_before: bool,
}

/// Shared shape for blockquote/code block styling — just a font/size/italic
/// override from the body style, not a full style of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockStyle {
    pub font: String,
    pub size_pt: u32,
    pub italic: bool,
}

/// An enlarged initial capital on each chapter's first paragraph. `scale` is a
/// multiple of `BodyStyle::size_pt` — e.g. `3.0` renders the first letter at
/// 3x body size. This is a *raised* cap (an oversized glyph sitting inline on
/// the first line), not a true multi-line-wrapping sunk drop cap — the latter
/// needs either a Typst package (network fetch, not available offline-only)
/// or hand-rolled multi-line layout math; out of scope for v1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropCapStyle {
    pub scale: f32,
}

/// A running header's left/right content, each a template string supporting
/// `{title}`/`{author}`/`{chapter}` tokens (substituted per-renderer — PDF can
/// do a real per-page "current chapter" lookback, DOCX/EPUB use the book
/// title/author since neither format renders our running header per-chapter
/// the same dynamic way — see each renderer for exactly what it fills in).
#[derive(Debug, Clone, PartialEq)]
pub struct RunningHeaderStyle {
    pub left: String,
    pub right: String,
}

fn manuscript() -> TypesetStyle {
    TypesetStyle {
        id: "manuscript".to_string(),
        label: "Manuscript".to_string(),
        page: PageStyle {
            width_mm: 215.9,
            height_mm: 279.4,
            margin_mm: 25.4,
        },
        body: BodyStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 12,
            line_height: 2.0,
            justify: false,
        },
        headings: HeadingStyle {
            font: "Libertinus Serif".to_string(),
            sizes_pt: [20, 18, 16, 14, 13, 12],
            page_break_before: true,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 12,
            italic: true,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 11,
            italic: false,
        },
        drop_cap: None,
        running_header: None,
    }
}

fn trade_paperback() -> TypesetStyle {
    TypesetStyle {
        id: "trade_paperback".to_string(),
        label: "Trade Paperback".to_string(),
        page: PageStyle {
            width_mm: 152.4,
            height_mm: 228.6,
            margin_mm: 19.0,
        },
        body: BodyStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 11,
            line_height: 1.3,
            justify: true,
        },
        headings: HeadingStyle {
            font: "Libertinus Serif".to_string(),
            sizes_pt: [24, 20, 17, 14, 13, 12],
            page_break_before: true,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 11,
            italic: true,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 10,
            italic: false,
        },
        drop_cap: Some(DropCapStyle { scale: 3.0 }),
        running_header: Some(RunningHeaderStyle {
            left: "{author}".to_string(),
            right: "{chapter}".to_string(),
        }),
    }
}

/// The 2 built-in presets — the starting point [`load`] appends custom styles
/// onto.
pub fn built_in_styles() -> Vec<TypesetStyle> {
    vec![manuscript(), trade_paperback()]
}

/// The always-loaded custom-style directory: `<config_dir>/tachylite/styles`,
/// the same base path `color_theme::global_themes_dir`/
/// `plugins::global_plugins_dir` use for their own subdirectories.
pub fn global_styles_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "tachylite").map(|dirs| dirs.config_dir().join("styles"))
}

#[derive(Deserialize)]
struct RawPageStyle {
    width_mm: f32,
    height_mm: f32,
    margin_mm: f32,
}

#[derive(Deserialize)]
struct RawBodyStyle {
    font: String,
    size_pt: u32,
    #[serde(default = "default_line_height")]
    line_height: f32,
    #[serde(default)]
    justify: bool,
}

fn default_line_height() -> f32 {
    1.15
}

#[derive(Deserialize)]
struct RawHeadingStyle {
    font: String,
    sizes_pt: [u32; 6],
    #[serde(default = "default_true")]
    page_break_before: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct RawBlockStyle {
    font: String,
    size_pt: u32,
    #[serde(default)]
    italic: bool,
}

#[derive(Deserialize)]
struct RawDropCapStyle {
    scale: f32,
}

#[derive(Deserialize)]
struct RawRunningHeaderStyle {
    #[serde(default)]
    left: String,
    #[serde(default)]
    right: String,
}

/// The on-disk shape of a custom style `.toml` file.
#[derive(Deserialize)]
struct RawTypesetStyle {
    id: String,
    label: String,
    page: RawPageStyle,
    body: RawBodyStyle,
    headings: RawHeadingStyle,
    blockquote: RawBlockStyle,
    code: RawBlockStyle,
    #[serde(default)]
    drop_cap: Option<RawDropCapStyle>,
    #[serde(default)]
    running_header: Option<RawRunningHeaderStyle>,
}

impl RawTypesetStyle {
    fn into_style(self) -> TypesetStyle {
        TypesetStyle {
            id: self.id.to_lowercase(),
            label: self.label,
            page: PageStyle {
                width_mm: self.page.width_mm,
                height_mm: self.page.height_mm,
                margin_mm: self.page.margin_mm,
            },
            body: BodyStyle {
                font: self.body.font,
                size_pt: self.body.size_pt,
                line_height: self.body.line_height,
                justify: self.body.justify,
            },
            headings: HeadingStyle {
                font: self.headings.font,
                sizes_pt: self.headings.sizes_pt,
                page_break_before: self.headings.page_break_before,
            },
            blockquote: BlockStyle {
                font: self.blockquote.font,
                size_pt: self.blockquote.size_pt,
                italic: self.blockquote.italic,
            },
            code: BlockStyle {
                font: self.code.font,
                size_pt: self.code.size_pt,
                italic: self.code.italic,
            },
            drop_cap: self.drop_cap.map(|d| DropCapStyle { scale: d.scale }),
            running_header: self.running_header.map(|h| RunningHeaderStyle {
                left: h.left,
                right: h.right,
            }),
        }
    }
}

/// Load every style: the 2 built-ins, plus every `*.toml` file directly inside
/// each of `dirs` (flat, not recursive; a missing directory is silently
/// skipped, not an error). Never fails outright: a file that doesn't parse or
/// whose `id` collides with an already-loaded style (a built-in, or an
/// earlier custom one — first loaded wins) is skipped, with a message
/// appended to the returned `Vec<String>` — same tolerance and shape as
/// `color_theme::load`.
pub fn load(dirs: &[&Path]) -> (Vec<TypesetStyle>, Vec<String>) {
    let mut styles = built_in_styles();
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
                .unwrap_or("style")
                .to_string();

            let source = match std::fs::read_to_string(&path) {
                Ok(source) => source,
                Err(err) => {
                    errors.push(format!("{name}: couldn't read file: {err}"));
                    continue;
                }
            };
            let raw: RawTypesetStyle = match toml::from_str(&source) {
                Ok(raw) => raw,
                Err(err) => {
                    errors.push(format!("{name}: {err}"));
                    continue;
                }
            };
            let style = raw.into_style();
            if let Some(existing) = styles.iter().find(|s| s.id == style.id) {
                errors.push(format!(
                    "{name}: style id \"{}\" is already used by \"{}\", skipping",
                    style.id, existing.label
                ));
                continue;
            }
            styles.push(style);
        }
    }

    (styles, errors)
}

pub fn find<'a>(styles: &'a [TypesetStyle], id: &str) -> Option<&'a TypesetStyle> {
    styles.iter().find(|style| style.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, contents: &str) -> PathBuf {
        let path = dir.join(rel);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn built_in_styles_have_unique_ids() {
        let styles = built_in_styles();
        assert_eq!(styles.len(), 2);
        let mut ids: Vec<&str> = styles.iter().map(|s| s.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn find_locates_a_built_in_style_by_id() {
        let styles = built_in_styles();
        assert!(find(&styles, "manuscript").is_some());
        assert!(find(&styles, "trade_paperback").is_some());
        assert!(find(&styles, "nonexistent").is_none());
    }

    #[test]
    fn load_with_no_directories_returns_just_the_built_ins() {
        let (styles, errors) = load(&[]);
        assert_eq!(styles.len(), 2);
        assert!(errors.is_empty());
    }

    #[test]
    fn load_skips_a_missing_directory_without_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does_not_exist");
        let (styles, errors) = load(&[missing.as_path()]);
        assert_eq!(styles.len(), 2);
        assert!(errors.is_empty());
    }

    #[test]
    fn load_reads_a_valid_custom_style() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "custom.toml",
            r#"
id = "my_style"
label = "My Style"

[page]
width_mm = 148.0
height_mm = 210.0
margin_mm = 15.0

[body]
font = "Libertinus Serif"
size_pt = 10
line_height = 1.2
justify = true

[headings]
font = "Libertinus Serif"
sizes_pt = [22, 19, 16, 14, 13, 12]

[blockquote]
font = "Libertinus Serif"
size_pt = 10
italic = true

[code]
font = "DejaVu Sans Mono"
size_pt = 9

[drop_cap]
scale = 2.5

[running_header]
left = "{title}"
right = "{chapter}"
"#,
        );

        let (styles, errors) = load(&[dir.path()]);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(styles.len(), 3);
        let custom = find(&styles, "my_style").unwrap();
        assert_eq!(custom.label, "My Style");
        assert_eq!(custom.page.width_mm, 148.0);
        assert!(custom.body.justify);
        assert_eq!(custom.drop_cap, Some(DropCapStyle { scale: 2.5 }));
        assert_eq!(
            custom.running_header,
            Some(RunningHeaderStyle {
                left: "{title}".to_string(),
                right: "{chapter}".to_string(),
            })
        );
    }

    #[test]
    fn load_a_minimal_style_without_drop_cap_or_running_header() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "minimal.toml",
            r#"
id = "minimal"
label = "Minimal"

[page]
width_mm = 210.0
height_mm = 297.0
margin_mm = 20.0

[body]
font = "Libertinus Serif"
size_pt = 11

[headings]
font = "Libertinus Serif"
sizes_pt = [20, 18, 16, 14, 13, 12]

[blockquote]
font = "Libertinus Serif"
size_pt = 11

[code]
font = "DejaVu Sans Mono"
size_pt = 10
"#,
        );

        let (styles, errors) = load(&[dir.path()]);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        let minimal = find(&styles, "minimal").unwrap();
        assert!(minimal.drop_cap.is_none());
        assert!(minimal.running_header.is_none());
        // default_line_height / default_true kick in
        assert_eq!(minimal.body.line_height, 1.15);
        assert!(minimal.headings.page_break_before);
        assert!(!minimal.body.justify);
    }

    #[test]
    fn load_skips_a_custom_style_colliding_with_a_built_in_id() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "collide.toml",
            r#"
id = "manuscript"
label = "Fake Manuscript"

[page]
width_mm = 210.0
height_mm = 297.0
margin_mm = 20.0

[body]
font = "Libertinus Serif"
size_pt = 11

[headings]
font = "Libertinus Serif"
sizes_pt = [20, 18, 16, 14, 13, 12]

[blockquote]
font = "Libertinus Serif"
size_pt = 11

[code]
font = "DejaVu Sans Mono"
size_pt = 10
"#,
        );

        let (styles, errors) = load(&[dir.path()]);
        assert_eq!(styles.len(), 2);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("manuscript"));
    }

    #[test]
    fn load_skips_a_malformed_toml_file_but_keeps_going() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "broken.toml", "this is not valid toml {{{");
        write(
            dir.path(),
            "zzz_valid.toml",
            r#"
id = "valid_one"
label = "Valid One"

[page]
width_mm = 210.0
height_mm = 297.0
margin_mm = 20.0

[body]
font = "Libertinus Serif"
size_pt = 11

[headings]
font = "Libertinus Serif"
sizes_pt = [20, 18, 16, 14, 13, 12]

[blockquote]
font = "Libertinus Serif"
size_pt = 11

[code]
font = "DejaVu Sans Mono"
size_pt = 10
"#,
        );

        let (styles, errors) = load(&[dir.path()]);
        assert_eq!(errors.len(), 1);
        assert!(find(&styles, "valid_one").is_some());
    }
}
