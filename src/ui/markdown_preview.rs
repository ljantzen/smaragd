use std::path::Path;

use egui::{Color32, FontId, RichText, TextFormat, text::LayoutJob};

use crate::editor_font::EditorFont;
use crate::markdown::{self, Block, BlockKind, ImageRef, Span};
use crate::ui::WikilinkActivation;

// Approximates the "dark"/dracula-family palette glow (charmbracelet/glow) renders
// markdown with in a terminal: pink/cyan/green/purple heading hierarchy, a muted
// blue-gray blockquote bar, and a dark panel for code. Kept as the *dark*-mode half
// of `Palette` below — used verbatim when `ui.visuals().dark_mode` is true.
const HEADING_COLORS_DARK: [Color32; 6] = [
    Color32::from_rgb(0xFF, 0x79, 0xC6), // h1 - pink
    Color32::from_rgb(0x8B, 0xE9, 0xFD), // h2 - cyan
    Color32::from_rgb(0x50, 0xFA, 0x7B), // h3 - green
    Color32::from_rgb(0xBD, 0x93, 0xF9), // h4 - purple
    Color32::from_rgb(0xF1, 0xFA, 0x8C), // h5 - yellow
    Color32::from_rgb(0xFF, 0xB8, 0x6C), // h6 - orange
];
// Darker, more saturated variants of the same hues (same order), legible against a
// light background instead — the light-mode half of `Palette`.
const HEADING_COLORS_LIGHT: [Color32; 6] = [
    Color32::from_rgb(0xB3, 0x00, 0x60), // h1 - pink
    Color32::from_rgb(0x00, 0x76, 0x8C), // h2 - cyan
    Color32::from_rgb(0x1E, 0x8E, 0x3A), // h3 - green
    Color32::from_rgb(0x6C, 0x3F, 0xA6), // h4 - purple
    Color32::from_rgb(0x8A, 0x6D, 0x00), // h5 - yellow
    Color32::from_rgb(0xB5, 0x5A, 0x00), // h6 - orange
];
const WIKILINK_COLOR_DARK: Color32 = Color32::from_rgb(0x50, 0xFA, 0x7B);
const WIKILINK_COLOR_LIGHT: Color32 = Color32::from_rgb(0x1E, 0x8E, 0x3A);
const QUOTE_BAR_DARK: Color32 = Color32::from_rgb(0x62, 0x72, 0xA4);
const QUOTE_BAR_LIGHT: Color32 = Color32::from_rgb(0x4A, 0x55, 0x78);

/// The body size the fixed per-level heading sizes below were tuned against —
/// `heading_size` scales them proportionally to whatever body size `Settings`
/// actually configures, so the H1..H6 hierarchy's *relative* proportions stay
/// the same regardless of the user's chosen font size.
const REFERENCE_BODY_SIZE: f32 = 15.0;
const BLOCK_SPACING: f32 = 10.0;
const INDENT_PER_DEPTH: f32 = 20.0;

/// Context needed to resolve a markdown `![](src)` into a loadable path: `dir` (the
/// open document's own folder) resolves a relative `src`, and `project_root` — when
/// both are set — bounds where that resolution is allowed to land. `Copy` so it
/// threads through the render functions below exactly like `base_dir` used to,
/// without extra parameters at every call site.
#[derive(Clone, Copy)]
struct ImageContext<'a> {
    dir: Option<&'a Path>,
    project_root: Option<&'a Path>,
}

/// Colors the preview renders with, derived from the current `egui::Visuals` (which
/// reflects dark/light mode and any active `color_theme`) once per `show()` call.
/// Body text, quote text, code backgrounds, and link color come straight from
/// `Visuals` so they're always correctly contrasted against whatever the current
/// theme's background is; headings and wikilinks keep the "glow"-style rainbow
/// hierarchy this module was modeled on, picking whichever of the two hardcoded hue
/// sets above actually reads clearly against a dark or light background.
struct Palette {
    heading: [Color32; 6],
    body: Color32,
    quote_bar: Color32,
    quote_text: Color32,
    code_bg: Color32,
    code_inline_bg: Color32,
    link: Color32,
    wikilink: Color32,
    dark_mode: bool,
    /// The user's configured body font/size (`Settings::editor_font`/
    /// `editor_font_size`, shared with the Editor) — everything except code
    /// blocks (always monospace, matching every other markdown renderer's
    /// convention) renders in this.
    body_font: EditorFont,
    body_size: f32,
}

impl Palette {
    /// `active_theme`, if given, may override `heading`/`wikilink`/`quote_bar`
    /// (via `ColorTheme::preview_heading`/`preview_wikilink`/`preview_quote_bar`)
    /// — a theme that leaves any of those `None` falls back to the hardcoded
    /// dark/light default for that one color, unchanged from before this existed.
    fn new(
        visuals: &egui::Visuals,
        active_theme: Option<&crate::color_theme::ColorTheme>,
        body_font: EditorFont,
        body_size: f32,
    ) -> Self {
        let (heading, quote_bar, wikilink) = if visuals.dark_mode {
            (HEADING_COLORS_DARK, QUOTE_BAR_DARK, WIKILINK_COLOR_DARK)
        } else {
            (HEADING_COLORS_LIGHT, QUOTE_BAR_LIGHT, WIKILINK_COLOR_LIGHT)
        };
        let heading = active_theme
            .and_then(|theme| theme.preview_heading)
            .unwrap_or(heading);
        let wikilink = active_theme
            .and_then(|theme| theme.preview_wikilink)
            .unwrap_or(wikilink);
        let quote_bar = active_theme
            .and_then(|theme| theme.preview_quote_bar)
            .unwrap_or(quote_bar);
        Self {
            heading,
            body: visuals.text_color(),
            quote_bar,
            quote_text: visuals.weak_text_color(),
            code_bg: visuals.code_bg_color,
            code_inline_bg: visuals.code_bg_color,
            link: visuals.hyperlink_color,
            wikilink,
            dark_mode: visuals.dark_mode,
            body_font,
            body_size,
        }
    }

    /// The body font at `size` points — every non-code-block text run resolves
    /// its `FontId` through this, rather than a hardcoded `FontId::proportional`.
    fn font_id(&self, size: f32) -> FontId {
        FontId::new(size, self.body_font.family())
    }
}

/// H1..H6 point size, scaled from the fixed reference sizes below (tuned at
/// `REFERENCE_BODY_SIZE`) so the hierarchy's proportions hold regardless of the
/// user's configured body size.
fn heading_size(level: u8, body_size: f32) -> f32 {
    let reference = match level {
        1 => 28.0,
        2 => 24.0,
        3 => 20.0,
        4 => 18.0,
        5 => 16.5,
        _ => 15.5,
    };
    reference * (body_size / REFERENCE_BODY_SIZE)
}

/// Push `color` further from the background — toward white in dark mode, toward
/// black in light mode — approximating a "stronger" emphasis color the way
/// `RichText::strong()` does, since egui has no real bold font weight.
fn emphasize(color: Color32, dark_mode: bool) -> Color32 {
    if dark_mode {
        Color32::from_rgb(
            color.r() + (255 - color.r()) / 2,
            color.g() + (255 - color.g()) / 2,
            color.b() + (255 - color.b()) / 2,
        )
    } else {
        Color32::from_rgb(color.r() / 2, color.g() / 2, color.b() / 2)
    }
}

/// Render `markdown` styled like the `glow` CLI's terminal preview: colored heading
/// hierarchy, a barred blockquote, a boxed code block, and a striped table, laid out
/// with egui widgets. `base_dir` (typically the open document's folder) resolves a
/// relative image path; `project_root`, if given, additionally bounds where that
/// resolution — and an absolute or `..`-escaping `src` — is allowed to land, so a
/// document can't make the preview read a file outside the project (see
/// `resolve_image_uri`). Pass `None` for either when there's no meaningful base/root.
///
/// Returns `Some` if the user clicked a `[[wikilink]]` during this frame — the caller
/// is responsible for finding (and, if `force_create` is set because Ctrl/Cmd was
/// held, creating) the matching document.
///
/// `typewriter_quotes` mirrors `Settings::typewriter_quotes` — when set, the
/// parsed blocks are run through `markdown::apply_typewriter_quotes` before
/// rendering, so the preview shows the same curly quotes/em dash/ellipsis an
/// export with the same setting would produce, without altering `markdown_text`
/// itself.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    markdown_text: &str,
    base_dir: Option<&Path>,
    project_root: Option<&Path>,
    active_theme: Option<&crate::color_theme::ColorTheme>,
    body_font: EditorFont,
    body_size: f32,
    typewriter_quotes: bool,
) -> Option<WikilinkActivation> {
    let base_dir = ImageContext {
        dir: base_dir,
        project_root,
    };
    let mut blocks = markdown::parse(crate::frontmatter::strip(markdown_text));
    if typewriter_quotes {
        markdown::apply_typewriter_quotes(&mut blocks);
    }
    let palette = Palette::new(ui.visuals(), active_theme, body_font, body_size);
    egui::ScrollArea::vertical()
        .id_salt("markdown_preview_scroll")
        .show(ui, |ui| {
            if blocks.is_empty() {
                ui.weak("Nothing to preview yet.");
                return None;
            }
            let mut clicked = None;
            for block in &blocks {
                if let Some(target) = render_block(ui, &palette, block, base_dir) {
                    clicked = Some(target);
                }
                ui.add_space(BLOCK_SPACING);
            }
            clicked
        })
        .inner
}

fn render_block(
    ui: &mut egui::Ui,
    palette: &Palette,
    block: &Block,
    base_dir: ImageContext<'_>,
) -> Option<WikilinkActivation> {
    match &block.kind {
        BlockKind::Heading(level) => render_heading(ui, palette, *level, &block.spans, base_dir),
        BlockKind::Paragraph => render_spans(
            ui,
            palette,
            &block.spans,
            palette.font_id(palette.body_size),
            palette.body,
            base_dir,
        ),
        BlockKind::CodeBlock { language } => {
            render_code_block(ui, palette, language.as_deref(), &block.spans);
            None
        }
        BlockKind::BlockQuote => render_blockquote(ui, palette, &block.spans, base_dir),
        BlockKind::ListItem {
            ordered,
            index,
            depth,
        } => render_list_item(
            ui,
            palette,
            *ordered,
            *index,
            *depth,
            &block.spans,
            base_dir,
        ),
        BlockKind::Rule => {
            ui.add_space(4.0);
            ui.separator();
            None
        }
        BlockKind::Table { header, rows, .. } => render_table(ui, palette, header, rows, base_dir),
    }
}

fn render_heading(
    ui: &mut egui::Ui,
    palette: &Palette,
    level: u8,
    spans: &[Span],
    base_dir: ImageContext<'_>,
) -> Option<WikilinkActivation> {
    let color = palette.heading[(level.saturating_sub(1).min(5)) as usize];
    let size = heading_size(level, palette.body_size);
    let clicked = render_spans(ui, palette, spans, palette.font_id(size), color, base_dir);
    if level == 1 {
        ui.add_space(2.0);
        ui.separator();
    }
    clicked
}

fn render_code_block(ui: &mut egui::Ui, palette: &Palette, language: Option<&str>, spans: &[Span]) {
    egui::Frame::new()
        .fill(palette.code_bg)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            if let Some(lang) = language {
                ui.weak(RichText::new(lang).size(11.0));
            }
            let text: String = spans.iter().map(|s| s.text.as_str()).collect();
            ui.add(egui::Label::new(
                RichText::new(text.trim_end_matches('\n'))
                    .font(FontId::monospace(palette.body_size))
                    .color(palette.body),
            ));
        });
}

fn render_blockquote(
    ui: &mut egui::Ui,
    palette: &Palette,
    spans: &[Span],
    base_dir: ImageContext<'_>,
) -> Option<WikilinkActivation> {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(3.0, ui.spacing().interact_size.y),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(rect, 0.0, palette.quote_bar);
        let mut italic_spans = spans.to_vec();
        for span in &mut italic_spans {
            span.italic = true;
        }
        render_spans(
            ui,
            palette,
            &italic_spans,
            palette.font_id(palette.body_size),
            palette.quote_text,
            base_dir,
        )
    })
    .inner
}

fn render_list_item(
    ui: &mut egui::Ui,
    palette: &Palette,
    ordered: bool,
    index: Option<u64>,
    depth: u8,
    spans: &[Span],
    base_dir: ImageContext<'_>,
) -> Option<WikilinkActivation> {
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * INDENT_PER_DEPTH);
        let bullet = if ordered {
            format!("{}.", index.unwrap_or(1))
        } else if depth == 0 {
            "•".to_string()
        } else {
            "◦".to_string()
        };
        ui.label(RichText::new(bullet).color(palette.body).strong());
        render_spans(
            ui,
            palette,
            spans,
            palette.font_id(palette.body_size),
            palette.body,
            base_dir,
        )
    })
    .inner
}

/// Render a GFM table as a striped grid. Column alignment (`:---:` etc.) is parsed
/// into the block already but not yet reflected here — every cell is left-aligned.
fn render_table(
    ui: &mut egui::Ui,
    palette: &Palette,
    header: &[Vec<Span>],
    rows: &[Vec<Vec<Span>>],
    base_dir: ImageContext<'_>,
) -> Option<WikilinkActivation> {
    let mut clicked = None;
    egui::Frame::new()
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            egui::Grid::new(ui.id().with("md_table"))
                .striped(true)
                .spacing(egui::vec2(16.0, 6.0))
                .show(ui, |ui| {
                    for cell in header {
                        if let Some(activation) = render_spans(
                            ui,
                            palette,
                            cell,
                            palette.font_id(palette.body_size),
                            emphasize(palette.body, palette.dark_mode),
                            base_dir,
                        ) {
                            clicked = Some(activation);
                        }
                    }
                    ui.end_row();
                    for row in rows {
                        for cell in row {
                            if let Some(activation) = render_spans(
                                ui,
                                palette,
                                cell,
                                palette.font_id(palette.body_size),
                                palette.body,
                                base_dir,
                            ) {
                                clicked = Some(activation);
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    clicked
}

/// Render a run of spans, laying out plain-text runs as wrapped, styled text, each
/// `[[wikilink]]` span as a clickable link, and each `![image](src)` span as a loaded
/// image (falling back to its alt text while loading or on error). Returns the
/// clicked wikilink, if any — holding Ctrl (Cmd on macOS) while clicking sets
/// `force_create`.
fn render_spans(
    ui: &mut egui::Ui,
    palette: &Palette,
    spans: &[Span],
    base_font: FontId,
    base_color: Color32,
    base_dir: ImageContext<'_>,
) -> Option<WikilinkActivation> {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut clicked = None;
        let mut buffer: Vec<Span> = Vec::new();
        for span in spans {
            if let Some(image) = &span.image {
                if !buffer.is_empty() {
                    ui.label(build_layout_job(
                        palette,
                        &buffer,
                        base_font.clone(),
                        base_color,
                    ));
                    buffer.clear();
                }
                render_image(ui, image, &span.text, base_dir);
            } else if let Some(target) = &span.wikilink {
                if !buffer.is_empty() {
                    ui.label(build_layout_job(
                        palette,
                        &buffer,
                        base_font.clone(),
                        base_color,
                    ));
                    buffer.clear();
                }
                let response = ui.link(RichText::new(&span.text).color(palette.wikilink));
                if response.clicked() {
                    let force_create = ui.input(|i| i.modifiers.command);
                    clicked = Some(WikilinkActivation {
                        target: target.clone(),
                        force_create,
                    });
                }
            } else {
                buffer.push(span.clone());
            }
        }
        if !buffer.is_empty() {
            ui.label(build_layout_job(palette, &buffer, base_font, base_color));
        }
        clicked
    })
    .inner
}

/// Load and display an image, resolving a relative `src` against `base_dir`. Loading
/// state and failures are handled by egui's own image widget (spinner while pending,
/// `alt` text if it can't be loaded) — remote `http(s)://` images won't load, since no
/// network image loader is installed, and will just show their alt text.
fn render_image(ui: &mut egui::Ui, image: &ImageRef, alt: &str, base_dir: ImageContext<'_>) {
    let uri = resolve_image_uri(&image.src, base_dir);
    let alt_text = if alt.is_empty() {
        image.src.clone()
    } else {
        alt.to_string()
    };
    let available_width = ui.available_width();
    ui.add(
        egui::Image::from_uri(uri)
            .max_width(available_width)
            .shrink_to_fit()
            .alt_text(alt_text),
    );
}

/// Turn a markdown image `src` into a URI egui's image loaders understand: passed
/// through unchanged if it's already a URI (`http://`, `file://`, `data:`, ...),
/// otherwise resolved as a filesystem path (relative to `base_dir` if it's not
/// already absolute).
///
/// When `project_root` is set, the resolved path is required to actually live under
/// it — checked by canonicalizing both, which also resolves any symlinks in the
/// path — before being turned into a `file://` URI; otherwise an unloadable sentinel
/// URI is returned (egui shows the image's alt text, same as any other load
/// failure). Without this, an absolute `src`, a `../`-escaping relative one, or a
/// symlink planted inside the project (e.g. by a collaborator's `git pull`) could
/// make previewing a document silently read an arbitrary file elsewhere on disk.
fn resolve_image_uri(src: &str, base_dir: ImageContext<'_>) -> String {
    if src.starts_with("data:") || src.contains("://") {
        return src.to_string();
    }
    let path = Path::new(src);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match base_dir.dir {
            Some(base) => base.join(path),
            None => path.to_path_buf(),
        }
    };
    if !is_within_project(&resolved, base_dir.project_root) {
        return "smaragd-blocked:outside-project".to_string();
    }
    format!("file://{}", resolved.display())
}

/// Whether `resolved` is (once symlinks are resolved) actually inside `project_root`
/// — or `project_root` wasn't given, in which case there's nothing to bound against
/// (e.g. a caller with no project context, or these unit tests). A path that doesn't
/// exist, or a `project_root` that doesn't, can't be canonicalized and is treated as
/// *not* contained — fail closed rather than let an unresolvable path through.
fn is_within_project(resolved: &Path, project_root: Option<&Path>) -> bool {
    let Some(project_root) = project_root else {
        return true;
    };
    let (Ok(resolved), Ok(project_root)) = (resolved.canonicalize(), project_root.canonicalize())
    else {
        return false;
    };
    resolved.starts_with(project_root)
}

/// Like `resolve_image_uri`, but for a caller (`export.rs`) that wants to read the
/// image's bytes off disk rather than hand egui a URI: resolves `src` relative to
/// `doc_dir` and returns the filesystem path only if it's actually contained within
/// `project_root` (same symlink-aware containment check as `resolve_image_uri`), or
/// `None` for a remote `http(s)://`/`data:` URI (never fetched, per `resolve_image_uri`)
/// or one that fails containment.
pub(crate) fn resolve_image_fs_path(
    src: &str,
    doc_dir: &Path,
    project_root: &Path,
) -> Option<std::path::PathBuf> {
    if src.starts_with("data:") || src.contains("://") {
        return None;
    }
    let path = Path::new(src);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        doc_dir.join(path)
    };
    is_within_project(&resolved, Some(project_root)).then_some(resolved)
}

fn build_layout_job(
    palette: &Palette,
    spans: &[Span],
    base_font: FontId,
    base_color: Color32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    for span in spans {
        let mut format = TextFormat {
            font_id: base_font.clone(),
            color: base_color,
            ..Default::default()
        };
        if span.bold {
            // egui has no bundled bold-weight font; like `RichText::strong()`, we
            // signal emphasis by pushing the color further from the background
            // rather than switching fonts.
            format.color = emphasize(base_color, palette.dark_mode);
        }
        if span.italic {
            format.italics = true;
        }
        if span.strikethrough {
            format.strikethrough = egui::Stroke::new(1.0, format.color);
        }
        if span.code {
            format.font_id = FontId::monospace(base_font.size * 0.95);
            format.background = palette.code_inline_bg;
        }
        if span.link.is_some() {
            format.color = palette.link;
            format.underline = egui::Stroke::new(1.0, palette.link);
        }
        job.append(&span.text, 0.0, format);
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_project_context() -> ImageContext<'static> {
        ImageContext {
            dir: None,
            project_root: None,
        }
    }

    #[test]
    fn resolve_image_uri_passes_through_existing_uris() {
        assert_eq!(
            resolve_image_uri("https://example.com/pic.png", no_project_context()),
            "https://example.com/pic.png"
        );
        assert_eq!(
            resolve_image_uri("data:image/png;base64,abc", no_project_context()),
            "data:image/png;base64,abc"
        );
    }

    #[test]
    fn resolve_image_uri_resolves_relative_paths_against_base_dir() {
        assert_eq!(
            resolve_image_uri(
                "images/pic.png",
                ImageContext {
                    dir: Some(Path::new("/vault/notes")),
                    project_root: None,
                }
            ),
            "file:///vault/notes/images/pic.png"
        );
    }

    #[test]
    fn resolve_image_uri_leaves_absolute_paths_alone_when_theres_no_project_root_to_check() {
        assert_eq!(
            resolve_image_uri(
                "/abs/pic.png",
                ImageContext {
                    dir: Some(Path::new("/vault/notes")),
                    project_root: None,
                }
            ),
            "file:///abs/pic.png"
        );
    }

    #[test]
    fn resolve_image_uri_without_base_dir_uses_the_relative_path_as_is() {
        assert_eq!(
            resolve_image_uri("pic.png", no_project_context()),
            "file://pic.png"
        );
    }

    #[test]
    fn resolve_image_uri_allows_a_relative_path_that_stays_inside_the_project_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("images")).unwrap();
        std::fs::write(dir.path().join("images/pic.png"), b"").unwrap();

        let uri = resolve_image_uri(
            "images/pic.png",
            ImageContext {
                dir: Some(dir.path()),
                project_root: Some(dir.path()),
            },
        );

        assert_eq!(
            uri,
            format!("file://{}", dir.path().join("images/pic.png").display())
        );
    }

    #[test]
    fn resolve_image_uri_blocks_a_relative_path_that_escapes_the_project_root_with_dot_dot() {
        // `doc_dir` (the project root) contains a document linking, via `..`, to an
        // image in `doc_dir`'s own parent — outside the project entirely.
        let outer = tempfile::tempdir().unwrap();
        let doc_dir = outer.path().join("project_root");
        std::fs::create_dir(&doc_dir).unwrap();
        std::fs::write(outer.path().join("secret.png"), b"").unwrap();

        let uri = resolve_image_uri(
            "../secret.png",
            ImageContext {
                dir: Some(&doc_dir),
                project_root: Some(&doc_dir),
            },
        );

        assert_eq!(uri, "smaragd-blocked:outside-project");
    }

    #[test]
    fn resolve_image_uri_blocks_an_absolute_path_outside_the_project_root() {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.png");
        std::fs::write(&secret, b"").unwrap();

        let uri = resolve_image_uri(
            secret.to_str().unwrap(),
            ImageContext {
                dir: Some(project.path()),
                project_root: Some(project.path()),
            },
        );

        assert_eq!(uri, "smaragd-blocked:outside-project");
    }

    #[cfg(unix)]
    #[test]
    fn resolve_image_uri_blocks_a_symlink_that_resolves_outside_the_project_root() {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.png");
        std::fs::write(&secret, b"").unwrap();
        std::os::unix::fs::symlink(&secret, project.path().join("Notes.png")).unwrap();

        let uri = resolve_image_uri(
            "Notes.png",
            ImageContext {
                dir: Some(project.path()),
                project_root: Some(project.path()),
            },
        );

        assert_eq!(uri, "smaragd-blocked:outside-project");
    }

    #[test]
    fn palette_from_dark_visuals_uses_the_dark_heading_set() {
        let palette = Palette::new(&egui::Visuals::dark(), None, EditorFont::Proportional, 15.0);
        assert_eq!(palette.heading, HEADING_COLORS_DARK);
        assert!(palette.dark_mode);
    }

    #[test]
    fn palette_from_light_visuals_uses_the_light_heading_set() {
        let palette = Palette::new(
            &egui::Visuals::light(),
            None,
            EditorFont::Proportional,
            15.0,
        );
        assert_eq!(palette.heading, HEADING_COLORS_LIGHT);
        assert!(!palette.dark_mode);
    }

    fn theme_with_preview_overrides() -> crate::color_theme::ColorTheme {
        let mut theme = crate::color_theme::built_in_themes().remove(0);
        theme.preview_heading = Some([Color32::WHITE; 6]);
        theme.preview_wikilink = Some(Color32::WHITE);
        theme.preview_quote_bar = Some(Color32::WHITE);
        theme
    }

    #[test]
    fn a_theme_with_preview_overrides_replaces_the_hardcoded_palette() {
        let theme = theme_with_preview_overrides();
        let palette = Palette::new(
            &egui::Visuals::dark(),
            Some(&theme),
            EditorFont::Proportional,
            15.0,
        );
        assert_eq!(palette.heading, [Color32::WHITE; 6]);
        assert_eq!(palette.wikilink, Color32::WHITE);
        assert_eq!(palette.quote_bar, Color32::WHITE);
    }

    #[test]
    fn a_theme_without_preview_overrides_leaves_the_hardcoded_palette_untouched() {
        let theme = crate::color_theme::built_in_themes().remove(0);
        assert!(theme.preview_heading.is_none());
        let palette = Palette::new(
            &egui::Visuals::dark(),
            Some(&theme),
            EditorFont::Proportional,
            15.0,
        );
        assert_eq!(palette.heading, HEADING_COLORS_DARK);
        assert_eq!(palette.wikilink, WIKILINK_COLOR_DARK);
        assert_eq!(palette.quote_bar, QUOTE_BAR_DARK);
    }

    #[test]
    fn emphasize_moves_toward_white_in_dark_mode_and_black_in_light_mode() {
        let mid_gray = Color32::from_rgb(0x80, 0x80, 0x80);
        let dark = emphasize(mid_gray, true);
        let light = emphasize(mid_gray, false);
        assert!(dark.r() > mid_gray.r());
        assert!(light.r() < mid_gray.r());
    }
}
