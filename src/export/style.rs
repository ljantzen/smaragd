//! Typesetting styles: the shared config model that drives DOCX, EPUB, and PDF
//! export together — a `TypesetStyle` picks fonts/sizes/page setup/running
//! headers/drop caps once, and each renderer (`export::docx`/`export::epub`/
//! `export::pdf`) reads from it instead of hardcoding its own literals.
//!
//! Modeled directly on `color_theme.rs`: 12 built-in presets, plus user-authored
//! `.toml` files dropped into [`global_styles_dir`] — same files-only authoring,
//! tolerant `load()`, and `find()`-by-id shape, no in-app style editor.
//!
//! Built-in presets stick to "Libertinus Serif" (body), "DejaVu Sans Mono"
//! (code), and "Atkinson Hyperlegible" (a sans-serif, used where legibility
//! matters more than a book-ish serif look) — not "Times New Roman"/"Courier
//! New"/"Arial" — because those three are what `export::pdf` can actually
//! guarantee are available: the first two bundled via `typst-kit`'s embedded
//! fonts (from `typst-assets`), the third registered directly (see
//! `export::pdf`'s own doc comment), so the PDF renderer never silently
//! substitutes an arbitrary fallback font. DOCX/EPUB treat the same names as
//! plain font *references* (Word/an e-reader substitutes if the font isn't
//! locally installed, same as any other font name) — one style genuinely
//! drives all three outputs with the same fonts, rather than PDF needing a
//! different "safe" font from DOCX/EPUB.

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
    /// A `.ttf`/`.otf` file to actually load and use for this slot in Preview
    /// and print-PDF, when `font` names a font that isn't one of the three
    /// bundled ones (`ui::markdown_preview::resolve_family` would otherwise
    /// fall back to a generic face on-screen, and PDF would depend on it
    /// being installed as a system font) — see `custom_font_files`. Resolved
    /// to an absolute path at load time if given as relative in a custom
    /// style's `.toml` (relative to that file's own directory); always `None`
    /// for a built-in style. Has no effect on DOCX/EPUB, which only ever
    /// reference `font` by name.
    pub font_file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeadingStyle {
    pub font: String,
    /// Point size per heading level 1-6, in that order.
    pub sizes_pt: [u32; 6],
    pub page_break_before: bool,
    /// Same as `BodyStyle::font_file`, for the headings font.
    pub font_file: Option<PathBuf>,
}

/// Shared shape for blockquote/code block styling — just a font/size/italic
/// override from the body style, not a full style of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockStyle {
    pub font: String,
    pub size_pt: u32,
    pub italic: bool,
    /// Same as `BodyStyle::font_file`, for this block's font.
    pub font_file: Option<PathBuf>,
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
/// `{title}`/`{subtitle}`/`{author}`/`{chapter}` tokens (substituted
/// per-renderer — PDF can do a real per-page "current chapter" lookback,
/// DOCX/EPUB use the book title/author since neither format renders our
/// running header per-chapter the same dynamic way — see each renderer for
/// exactly what it fills in).
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
            font_file: None,
        },
        headings: HeadingStyle {
            font: "Libertinus Serif".to_string(),
            sizes_pt: [20, 18, 16, 14, 13, 12],
            page_break_before: true,
            font_file: None,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 12,
            italic: true,
            font_file: None,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 11,
            italic: false,
            font_file: None,
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
            font_file: None,
        },
        headings: HeadingStyle {
            font: "Libertinus Serif".to_string(),
            sizes_pt: [24, 20, 17, 14, 13, 12],
            page_break_before: true,
            font_file: None,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 11,
            italic: true,
            font_file: None,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 10,
            italic: false,
            font_file: None,
        },
        drop_cap: Some(DropCapStyle { scale: 3.0 }),
        running_header: Some(RunningHeaderStyle {
            left: "{author}".to_string(),
            right: "{chapter}".to_string(),
        }),
    }
}

/// KDP's mass-market trim size. Smaller type and tighter margins than Trade
/// Paperback are typical of the format, not just a scaled-down version of it.
fn mass_market_paperback() -> TypesetStyle {
    TypesetStyle {
        id: "mass_market".to_string(),
        label: "Mass Market Paperback".to_string(),
        page: PageStyle {
            width_mm: 108.0,
            height_mm: 174.6,
            margin_mm: 12.0,
        },
        body: BodyStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 9,
            line_height: 1.15,
            justify: true,
            font_file: None,
        },
        headings: HeadingStyle {
            font: "Libertinus Serif".to_string(),
            sizes_pt: [18, 16, 14, 12, 11, 10],
            page_break_before: true,
            font_file: None,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 9,
            italic: true,
            font_file: None,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 8,
            italic: false,
            font_file: None,
        },
        drop_cap: Some(DropCapStyle { scale: 2.5 }),
        running_header: Some(RunningHeaderStyle {
            left: "{author}".to_string(),
            right: "{chapter}".to_string(),
        }),
    }
}

/// 5.5x8.5in — the common "digest"/A-format trim between Mass Market and Trade
/// Paperback, often used for novellas and shorter literary fiction.
fn digest() -> TypesetStyle {
    TypesetStyle {
        id: "digest".to_string(),
        label: "Digest".to_string(),
        page: PageStyle {
            width_mm: 139.7,
            height_mm: 215.9,
            margin_mm: 15.0,
        },
        body: BodyStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 11,
            line_height: 1.2,
            justify: true,
            font_file: None,
        },
        headings: HeadingStyle {
            font: "Libertinus Serif".to_string(),
            sizes_pt: [22, 19, 16, 14, 13, 12],
            page_break_before: true,
            font_file: None,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 11,
            italic: true,
            font_file: None,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 10,
            italic: false,
            font_file: None,
        },
        drop_cap: Some(DropCapStyle { scale: 3.0 }),
        running_header: Some(RunningHeaderStyle {
            left: "{author}".to_string(),
            right: "{chapter}".to_string(),
        }),
    }
}

/// KDP's hardcover trim — larger than Trade Paperback with roomier margins and
/// a bigger drop cap, matching the more generous, formal feel of a hardback.
/// Headings switch to a sans-serif (Atkinson Hyperlegible) over the serif
/// body text — a common real hardback convention (sans chapter titles over a
/// serif body) that also gives Hardcover a distinct identity from Trade
/// Paperback beyond just "bigger."
fn hardcover() -> TypesetStyle {
    TypesetStyle {
        id: "hardcover".to_string(),
        label: "Hardcover".to_string(),
        page: PageStyle {
            width_mm: 156.0,
            height_mm: 234.0,
            margin_mm: 22.0,
        },
        body: BodyStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 12,
            line_height: 1.3,
            justify: true,
            font_file: None,
        },
        headings: HeadingStyle {
            font: "Atkinson Hyperlegible".to_string(),
            sizes_pt: [26, 22, 18, 15, 14, 13],
            page_break_before: true,
            font_file: None,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 12,
            italic: true,
            font_file: None,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 11,
            italic: false,
            font_file: None,
        },
        drop_cap: Some(DropCapStyle { scale: 3.5 }),
        running_header: Some(RunningHeaderStyle {
            left: "{title}".to_string(),
            right: "{chapter}".to_string(),
        }),
    }
}

/// A4, ragged-right body text (APA-style guidance calls for a ragged right
/// margin, not justified, in a manuscript), no drop cap or running header —
/// for a thesis chapter or paper draft, not a book.
fn academic() -> TypesetStyle {
    TypesetStyle {
        id: "academic".to_string(),
        label: "Academic".to_string(),
        page: PageStyle {
            width_mm: 210.0,
            height_mm: 297.0,
            margin_mm: 25.0,
        },
        body: BodyStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 12,
            line_height: 1.5,
            justify: false,
            font_file: None,
        },
        headings: HeadingStyle {
            font: "Libertinus Serif".to_string(),
            sizes_pt: [18, 16, 14, 13, 12, 11],
            page_break_before: false,
            font_file: None,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 12,
            italic: true,
            font_file: None,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 10,
            italic: false,
            font_file: None,
        },
        drop_cap: None,
        running_header: None,
    }
}

/// A 7x10in trim with 18pt Atkinson Hyperlegible body text — a sans-serif
/// designed by the Braille Institute specifically for low-vision readers,
/// not just "the serif font scaled up" — ragged-right (uneven word spacing
/// from justification hurts legibility more than it helps at this size) and
/// no drop cap or running header: accessibility features, not decoration.
fn large_print() -> TypesetStyle {
    TypesetStyle {
        id: "large_print".to_string(),
        label: "Large Print".to_string(),
        page: PageStyle {
            width_mm: 177.8,
            height_mm: 254.0,
            margin_mm: 20.0,
        },
        body: BodyStyle {
            font: "Atkinson Hyperlegible".to_string(),
            size_pt: 18,
            line_height: 1.5,
            justify: false,
            font_file: None,
        },
        headings: HeadingStyle {
            font: "Atkinson Hyperlegible".to_string(),
            sizes_pt: [30, 27, 24, 22, 20, 19],
            page_break_before: true,
            font_file: None,
        },
        blockquote: BlockStyle {
            font: "Atkinson Hyperlegible".to_string(),
            size_pt: 18,
            italic: true,
            font_file: None,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 16,
            italic: false,
            font_file: None,
        },
        drop_cap: None,
        running_header: None,
    }
}

/// A small 5x8in trim with generous margins and ragged-right body text —
/// justification would fight a poem's own intentional line breaks, so this
/// stays unjustified regardless of `body.justify`'s usual "book" default.
/// No drop cap, keeping the spare look typical of a poetry chapbook; the
/// running header shows author/title rather than `{chapter}`, since a
/// chapbook's sections are poems, not chapters.
fn chapbook() -> TypesetStyle {
    TypesetStyle {
        id: "chapbook".to_string(),
        label: "Chapbook".to_string(),
        page: PageStyle {
            width_mm: 127.0,
            height_mm: 203.0,
            margin_mm: 20.0,
        },
        body: BodyStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 11,
            line_height: 1.4,
            justify: false,
            font_file: None,
        },
        headings: HeadingStyle {
            font: "Libertinus Serif".to_string(),
            sizes_pt: [20, 18, 16, 14, 13, 12],
            page_break_before: true,
            font_file: None,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 11,
            italic: true,
            font_file: None,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 10,
            italic: false,
            font_file: None,
        },
        drop_cap: None,
        running_header: Some(RunningHeaderStyle {
            left: "{author}".to_string(),
            right: "{title}".to_string(),
        }),
    }
}

/// The UK trade paperback size — Trade Paperback's US 6x9in counterpart, not
/// a scaled version of it: a narrower, taller trim with smaller type, the
/// standard most UK fiction is actually printed at.
fn uk_b_format() -> TypesetStyle {
    TypesetStyle {
        id: "uk_b_format".to_string(),
        label: "UK B-Format Paperback".to_string(),
        page: PageStyle {
            width_mm: 129.0,
            height_mm: 198.0,
            margin_mm: 16.0,
        },
        body: BodyStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 10,
            line_height: 1.25,
            justify: true,
            font_file: None,
        },
        headings: HeadingStyle {
            font: "Libertinus Serif".to_string(),
            sizes_pt: [22, 19, 16, 14, 13, 12],
            page_break_before: true,
            font_file: None,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 10,
            italic: true,
            font_file: None,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 9,
            italic: false,
            font_file: None,
        },
        drop_cap: Some(DropCapStyle { scale: 3.0 }),
        running_header: Some(RunningHeaderStyle {
            left: "{author}".to_string(),
            right: "{chapter}".to_string(),
        }),
    }
}

/// The UK mass-market paperback size — smaller and narrower than the US
/// equivalent (`mass_market_paperback`), not the same trim under a different
/// name.
fn uk_a_format() -> TypesetStyle {
    TypesetStyle {
        id: "uk_a_format".to_string(),
        label: "UK A-Format Paperback".to_string(),
        page: PageStyle {
            width_mm: 110.0,
            height_mm: 178.0,
            margin_mm: 13.0,
        },
        body: BodyStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 9,
            line_height: 1.15,
            justify: true,
            font_file: None,
        },
        headings: HeadingStyle {
            font: "Libertinus Serif".to_string(),
            sizes_pt: [18, 16, 14, 12, 11, 10],
            page_break_before: true,
            font_file: None,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 9,
            italic: true,
            font_file: None,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 8,
            italic: false,
            font_file: None,
        },
        drop_cap: Some(DropCapStyle { scale: 2.5 }),
        running_header: Some(RunningHeaderStyle {
            left: "{author}".to_string(),
            right: "{chapter}".to_string(),
        }),
    }
}

/// ISO 216 A5 (148x210mm exactly) — a common European trade/paperback trim,
/// often used as-is rather than cut down from a larger sheet the way the
/// US/UK trims above are.
fn a5() -> TypesetStyle {
    TypesetStyle {
        id: "a5".to_string(),
        label: "A5 Paperback".to_string(),
        page: PageStyle {
            width_mm: 148.0,
            height_mm: 210.0,
            margin_mm: 15.0,
        },
        body: BodyStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 10,
            line_height: 1.2,
            justify: true,
            font_file: None,
        },
        headings: HeadingStyle {
            font: "Libertinus Serif".to_string(),
            sizes_pt: [20, 18, 16, 14, 13, 12],
            page_break_before: true,
            font_file: None,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 10,
            italic: true,
            font_file: None,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 9,
            italic: false,
            font_file: None,
        },
        drop_cap: Some(DropCapStyle { scale: 2.8 }),
        running_header: Some(RunningHeaderStyle {
            left: "{author}".to_string(),
            right: "{chapter}".to_string(),
        }),
    }
}

/// `manuscript()`'s exact submission conventions (double-spaced, ragged-right,
/// no running header or drop cap) on ISO A4 instead of US Letter — for
/// submitting to an agent or publisher outside North America, where A4 (not
/// Letter) is the expected page size.
fn manuscript_a4() -> TypesetStyle {
    TypesetStyle {
        id: "manuscript_a4".to_string(),
        label: "Manuscript (A4)".to_string(),
        page: PageStyle {
            width_mm: 210.0,
            height_mm: 297.0,
            margin_mm: 25.0,
        },
        body: BodyStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 12,
            line_height: 2.0,
            justify: false,
            font_file: None,
        },
        headings: HeadingStyle {
            font: "Libertinus Serif".to_string(),
            sizes_pt: [20, 18, 16, 14, 13, 12],
            page_break_before: true,
            font_file: None,
        },
        blockquote: BlockStyle {
            font: "Libertinus Serif".to_string(),
            size_pt: 12,
            italic: true,
            font_file: None,
        },
        code: BlockStyle {
            font: "DejaVu Sans Mono".to_string(),
            size_pt: 11,
            italic: false,
            font_file: None,
        },
        drop_cap: None,
        running_header: None,
    }
}

/// The 12 built-in presets — the starting point [`load`] appends custom
/// styles onto. `manuscript`/`trade_paperback` stay first (index 0/1) since a
/// couple of tests elsewhere (`export::docx`/`epub`/`pdf`) grab them by that
/// index.
pub fn built_in_styles() -> Vec<TypesetStyle> {
    vec![
        manuscript(),
        trade_paperback(),
        mass_market_paperback(),
        digest(),
        hardcover(),
        academic(),
        large_print(),
        chapbook(),
        uk_b_format(),
        uk_a_format(),
        a5(),
        manuscript_a4(),
    ]
}

/// The always-loaded custom-style directory: `<config_dir>/smaragd/styles`,
/// the same base path `color_theme::global_themes_dir`/
/// `plugins::global_plugins_dir` use for their own subdirectories.
pub fn global_styles_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "smaragd").map(|dirs| dirs.config_dir().join("styles"))
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
    #[serde(default)]
    font_file: Option<PathBuf>,
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
    #[serde(default)]
    font_file: Option<PathBuf>,
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
    #[serde(default)]
    font_file: Option<PathBuf>,
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

/// Resolves a `font_file` path from a custom style's `.toml`: relative paths
/// are resolved against `base_dir` (the directory the `.toml` file itself
/// lives in, so a style and its font file can just sit next to each other),
/// an absolute path is left as-is.
fn resolve_font_file(path: Option<PathBuf>, base_dir: &Path) -> Option<PathBuf> {
    path.map(|p| if p.is_absolute() { p } else { base_dir.join(p) })
}

impl RawTypesetStyle {
    fn into_style(self, base_dir: &Path) -> TypesetStyle {
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
                font_file: resolve_font_file(self.body.font_file, base_dir),
            },
            headings: HeadingStyle {
                font: self.headings.font,
                sizes_pt: self.headings.sizes_pt,
                page_break_before: self.headings.page_break_before,
                font_file: resolve_font_file(self.headings.font_file, base_dir),
            },
            blockquote: BlockStyle {
                font: self.blockquote.font,
                size_pt: self.blockquote.size_pt,
                italic: self.blockquote.italic,
                font_file: resolve_font_file(self.blockquote.font_file, base_dir),
            },
            code: BlockStyle {
                font: self.code.font,
                size_pt: self.code.size_pt,
                italic: self.code.italic,
                font_file: resolve_font_file(self.code.font_file, base_dir),
            },
            drop_cap: self.drop_cap.map(|d| DropCapStyle { scale: d.scale }),
            running_header: self.running_header.map(|h| RunningHeaderStyle {
                left: h.left,
                right: h.right,
            }),
        }
    }
}

/// Load every style: the 12 built-ins, plus every `*.toml` file directly inside
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
            let base_dir = path.parent().unwrap_or(*dir);
            let style = raw.into_style(base_dir);
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

/// Every (font name, resolved font file) pair declared via a `font_file` across
/// `styles`' body/headings/blockquote/code slots — the list a caller with access
/// to an `egui::Context` (`editor_font::install`) actually loads and registers,
/// since this module stays egui-free (see the module doc comment). Deduplicated
/// by name, case-insensitively: if two styles (or two slots of the same style)
/// declare the same font name with different files, the first one found wins —
/// the same "first loaded wins" tolerance style/theme id collisions already get
/// elsewhere in this codebase, rather than making a font's actual identity
/// depend on style load order in a way nothing warns you about. A style's font
/// slot with no `font_file` contributes nothing here — most styles built entirely
/// from the three always-bundled fonts return an empty list.
pub fn custom_font_files(styles: &[TypesetStyle]) -> Vec<(String, PathBuf)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut consider = |name: &str, file: &Option<PathBuf>| {
        let Some(file) = file else { return };
        let key = name.to_lowercase();
        if seen.insert(key) {
            out.push((name.to_string(), file.clone()));
        }
    };
    for style in styles {
        consider(&style.body.font, &style.body.font_file);
        consider(&style.headings.font, &style.headings.font_file);
        consider(&style.blockquote.font, &style.blockquote.font_file);
        consider(&style.code.font, &style.code.font_file);
    }
    out
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
        assert_eq!(styles.len(), 12);
        let mut ids: Vec<&str> = styles.iter().map(|s| s.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 12);
    }

    #[test]
    fn find_locates_a_built_in_style_by_id() {
        let styles = built_in_styles();
        assert!(find(&styles, "manuscript").is_some());
        assert!(find(&styles, "trade_paperback").is_some());
        assert!(find(&styles, "mass_market").is_some());
        assert!(find(&styles, "digest").is_some());
        assert!(find(&styles, "hardcover").is_some());
        assert!(find(&styles, "academic").is_some());
        assert!(find(&styles, "large_print").is_some());
        assert!(find(&styles, "chapbook").is_some());
        assert!(find(&styles, "uk_b_format").is_some());
        assert!(find(&styles, "uk_a_format").is_some());
        assert!(find(&styles, "a5").is_some());
        assert!(find(&styles, "manuscript_a4").is_some());
        assert!(find(&styles, "nonexistent").is_none());
    }

    #[test]
    fn load_with_no_directories_returns_just_the_built_ins() {
        let (styles, errors) = load(&[]);
        assert_eq!(styles.len(), 12);
        assert!(errors.is_empty());
    }

    #[test]
    fn load_skips_a_missing_directory_without_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does_not_exist");
        let (styles, errors) = load(&[missing.as_path()]);
        assert_eq!(styles.len(), 12);
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
        assert_eq!(styles.len(), 13);
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
    fn load_resolves_a_relative_font_file_against_the_styles_own_directory() {
        let dir = tempfile::tempdir().unwrap();
        // The font file doesn't need to actually exist for `load`/`into_style`
        // to resolve its path correctly — reading and validating the bytes is
        // `editor_font::install_custom_fonts`'s job, not this module's (see its
        // own doc comment: this module stays egui-free).
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
font = "My Custom Font"
size_pt = 10
font_file = "MyCustomFont.ttf"

[headings]
font = "Libertinus Serif"
sizes_pt = [22, 19, 16, 14, 13, 12]

[blockquote]
font = "Libertinus Serif"
size_pt = 10

[code]
font = "DejaVu Sans Mono"
size_pt = 9
"#,
        );

        let (styles, errors) = load(&[dir.path()]);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        let custom = find(&styles, "my_style").unwrap();
        assert_eq!(
            custom.body.font_file,
            Some(dir.path().join("MyCustomFont.ttf"))
        );
    }

    #[test]
    fn load_leaves_an_absolute_font_file_path_unchanged() {
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
font = "My Custom Font"
size_pt = 10
font_file = "/opt/fonts/MyCustomFont.ttf"

[headings]
font = "Libertinus Serif"
sizes_pt = [22, 19, 16, 14, 13, 12]

[blockquote]
font = "Libertinus Serif"
size_pt = 10

[code]
font = "DejaVu Sans Mono"
size_pt = 9
"#,
        );

        let (styles, errors) = load(&[dir.path()]);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        let custom = find(&styles, "my_style").unwrap();
        assert_eq!(
            custom.body.font_file,
            Some(PathBuf::from("/opt/fonts/MyCustomFont.ttf"))
        );
    }

    #[test]
    fn custom_font_files_is_empty_for_built_in_styles() {
        assert!(custom_font_files(&built_in_styles()).is_empty());
    }

    #[test]
    fn custom_font_files_collects_every_slot_with_a_font_file() {
        let mut style = manuscript();
        style.body.font = "Body Font".to_string();
        style.body.font_file = Some(PathBuf::from("/fonts/body.ttf"));
        style.headings.font = "Heading Font".to_string();
        style.headings.font_file = Some(PathBuf::from("/fonts/heading.ttf"));

        let files = custom_font_files(&[style]);
        assert_eq!(files.len(), 2);
        assert!(files.contains(&("Body Font".to_string(), PathBuf::from("/fonts/body.ttf"))));
        assert!(files.contains(&(
            "Heading Font".to_string(),
            PathBuf::from("/fonts/heading.ttf")
        )));
    }

    #[test]
    fn custom_font_files_dedups_by_name_case_insensitively_first_wins() {
        let mut a = manuscript();
        a.id = "a".to_string();
        a.body.font = "Shared Font".to_string();
        a.body.font_file = Some(PathBuf::from("/fonts/first.ttf"));

        let mut b = trade_paperback();
        b.id = "b".to_string();
        b.body.font = "shared font".to_string();
        b.body.font_file = Some(PathBuf::from("/fonts/second.ttf"));

        let files = custom_font_files(&[a, b]);
        assert_eq!(
            files,
            vec![("Shared Font".to_string(), PathBuf::from("/fonts/first.ttf"))]
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
        assert_eq!(styles.len(), 12);
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
