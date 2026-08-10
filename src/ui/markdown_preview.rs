use std::path::Path;

use egui::{Color32, FontId, RichText, TextFormat, text::LayoutJob};

use crate::editor_font::EditorFont;
use crate::export::style::{self, DropCapStyle, TypesetStyle};
use crate::markdown::{self, Block, BlockKind, ImageRef, Span};
use crate::ui::WikilinkActivation;

const BLOCK_SPACING: f32 = 10.0;
const INDENT_PER_DEPTH: f32 = 20.0;

/// Rough mm -> px scale used only to frame the preview's text column to a
/// style's page width, so switching between (e.g.) Manuscript and Trade
/// Paperback visibly narrows/widens the column the way switching styles
/// should — not meant to be print-accurate (screen DPI varies).
const PX_PER_MM: f32 = 3.0;
const MIN_CONTENT_WIDTH: f32 = 100.0;

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

/// Resolves a `TypesetStyle` font name to one of the families egui actually has
/// registered — either one of the three always-bundled ones
/// (`editor_font::install`), or, if `custom_fonts` names it, a custom font a
/// style loaded via `font_file` (`editor_font::install_custom_fonts`). A
/// style's font is a free-text string — fine for DOCX/EPUB (which just
/// reference a font by name) and for PDF (which resolves it via Typst,
/// falling back to *some* available font if it's not installed) — but egui
/// itself only knows what's actually been registered. `custom_fonts` must
/// only ever contain names `install_custom_fonts` actually succeeded on —
/// building an `egui::FontFamily::Name` for a name that was never registered
/// panics the next time it's used to lay out text, so this never does that
/// optimistically just because a style's `font_file` was set. An unrecognized
/// name falls back to `fallback` here, same "some available font rather than
/// none" spirit `export::pdf` already documents for its own resolution.
fn resolve_family(
    font_name: &str,
    custom_fonts: &[String],
    fallback: egui::FontFamily,
) -> egui::FontFamily {
    if font_name.eq_ignore_ascii_case("Libertinus Serif") {
        EditorFont::LibertinusSerif.family()
    } else if font_name.eq_ignore_ascii_case("DejaVu Sans Mono") {
        EditorFont::DejaVuSansMono.family()
    } else if font_name.eq_ignore_ascii_case("Atkinson Hyperlegible") {
        EditorFont::AtkinsonHyperlegible.family()
    } else if let Some(registered) = custom_fonts
        .iter()
        .find(|name| name.eq_ignore_ascii_case(font_name))
    {
        // Uses `registered`'s own casing, not `font_name`'s — they can differ if
        // two styles declare the same font under different casing, and only the
        // registered one is guaranteed to actually be the family key
        // `editor_font::install_custom_fonts` registered.
        egui::FontFamily::Name(registered.clone().into())
    } else {
        fallback
    }
}

/// Rendering inputs derived once per `show()` call from the selected
/// `TypesetStyle` and the current `egui::Visuals` — the styling analog of the
/// old Glow-palette `Palette`, but driven by what will actually get exported
/// instead of a fixed dev-preview color scheme. Colors stay theme-aware
/// (`Visuals::text_color`/`weak_text_color`/`hyperlink_color`) since a
/// `TypesetStyle` has no color concept of its own — book export is
/// effectively monochrome ink on a page.
struct PreviewStyle<'a> {
    style: &'a TypesetStyle,
    body_family: egui::FontFamily,
    heading_family: egui::FontFamily,
    quote_family: egui::FontFamily,
    code_family: egui::FontFamily,
    text_color: Color32,
    quote_text_color: Color32,
    quote_bar_color: Color32,
    code_bg: Color32,
    link_color: Color32,
    dark_mode: bool,
}

impl<'a> PreviewStyle<'a> {
    fn new(visuals: &egui::Visuals, style: &'a TypesetStyle, custom_fonts: &[String]) -> Self {
        Self {
            style,
            body_family: resolve_family(
                &style.body.font,
                custom_fonts,
                egui::FontFamily::Proportional,
            ),
            heading_family: resolve_family(
                &style.headings.font,
                custom_fonts,
                egui::FontFamily::Proportional,
            ),
            quote_family: resolve_family(
                &style.blockquote.font,
                custom_fonts,
                egui::FontFamily::Proportional,
            ),
            code_family: resolve_family(
                &style.code.font,
                custom_fonts,
                egui::FontFamily::Monospace,
            ),
            text_color: visuals.text_color(),
            quote_text_color: visuals.weak_text_color(),
            quote_bar_color: visuals.weak_text_color(),
            code_bg: visuals.code_bg_color,
            link_color: visuals.hyperlink_color,
            dark_mode: visuals.dark_mode,
        }
    }

    fn body_font(&self, size: f32) -> FontId {
        FontId::new(size, self.body_family.clone())
    }

    fn heading_font(&self, level: u8) -> FontId {
        let size = self.style.headings.sizes_pt[(level.saturating_sub(1).min(5)) as usize] as f32;
        FontId::new(size, self.heading_family.clone())
    }

    fn quote_font(&self) -> FontId {
        FontId::new(
            self.style.blockquote.size_pt as f32,
            self.quote_family.clone(),
        )
    }

    fn code_font(&self) -> FontId {
        FontId::new(self.style.code.size_pt as f32, self.code_family.clone())
    }
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

/// The Preview tab's `show()` result: a clicked wikilink (if any, same as
/// before) plus the new style id, when the inline Style picker changed it this
/// frame — the caller (`app::dock_tab_viewer`) is responsible for both.
pub struct PreviewOutcome {
    pub wikilink: Option<WikilinkActivation>,
    pub style_changed: Option<String>,
}

/// Render `markdown_text` styled as it will actually appear once exported: fonts,
/// sizes, justification, drop cap, and page-width proportions all come from
/// `style_id`'s `TypesetStyle` (looked up in `styles`, falling back to the first
/// loaded style if `style_id` doesn't resolve — same fallback `app::open_export`
/// uses) rather than a fixed dev-preview palette. An inline Style combo box above
/// the rendered text lets the style be switched live; picking a different one is
/// reported via `PreviewOutcome::style_changed` for the caller to persist onto
/// `ProjectMeta::book_style` — the same field the Export dialog reads/writes — so
/// Preview and Export always agree on what you'll get.
///
/// `base_dir` (typically the open document's folder) resolves a relative image
/// path; `project_root`, if given, additionally bounds where that resolution —
/// and an absolute or `..`-escaping `src` — is allowed to land, so a document
/// can't make the preview read a file outside the project (see
/// `resolve_image_uri`). Pass `None` for either when there's no meaningful
/// base/root.
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
    styles: &[TypesetStyle],
    style_id: &str,
    custom_fonts: &[String],
    typewriter_quotes: bool,
) -> PreviewOutcome {
    let base_dir = ImageContext {
        dir: base_dir,
        project_root,
    };

    let mut selected_id = style_id.to_string();
    let current_label = style::find(styles, &selected_id)
        .map(|s| s.label.as_str())
        .unwrap_or("(none)");
    // `with_layout` alone claims the *entire* remaining height of the panel as
    // its rect (right_to_left is still a single row, but nothing bounds that
    // row to one line's height) — wrapping it in `horizontal` first bounds the
    // row to its tallest child, so only then does right-aligning inside it
    // actually look like a slim top bar instead of pushing everything below
    // (the separator, the rendered document) down past the visible area.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::from_id_salt("preview_style_combo")
                .selected_text(current_label)
                .show_ui(ui, |ui| {
                    for candidate in styles {
                        ui.selectable_value(
                            &mut selected_id,
                            candidate.id.clone(),
                            &candidate.label,
                        );
                    }
                });
        });
    });
    let style_changed = (selected_id != style_id).then(|| selected_id.clone());
    ui.separator();

    let Some(style) = style::find(styles, &selected_id).or_else(|| styles.first()) else {
        ui.weak("No typesetting style available.");
        return PreviewOutcome {
            wikilink: None,
            style_changed,
        };
    };

    let mut blocks = markdown::parse(crate::frontmatter::strip(markdown_text));
    if typewriter_quotes {
        markdown::apply_typewriter_quotes(&mut blocks);
    }
    let ps = PreviewStyle::new(ui.visuals(), style, custom_fonts);

    let wikilink = egui::ScrollArea::vertical()
        .id_salt("markdown_preview_scroll")
        .show(ui, |ui| {
            if blocks.is_empty() {
                ui.weak("Nothing to preview yet.");
                return None;
            }
            let content_width = ((style.page.width_mm - 2.0 * style.page.margin_mm) * PX_PER_MM)
                .max(MIN_CONTENT_WIDTH)
                .min(ui.available_width());
            ui.set_max_width(content_width);

            let mut clicked = None;
            let mut first_paragraph_seen = false;
            for block in &blocks {
                let drop_cap_here =
                    !first_paragraph_seen && matches!(block.kind, BlockKind::Paragraph);
                if matches!(block.kind, BlockKind::Paragraph) {
                    first_paragraph_seen = true;
                }
                if let Some(target) = render_block(ui, &ps, block, base_dir, drop_cap_here) {
                    clicked = Some(target);
                }
                ui.add_space(BLOCK_SPACING);
            }
            clicked
        })
        .inner;

    PreviewOutcome {
        wikilink,
        style_changed,
    }
}

fn render_block(
    ui: &mut egui::Ui,
    ps: &PreviewStyle,
    block: &Block,
    base_dir: ImageContext<'_>,
    drop_cap_here: bool,
) -> Option<WikilinkActivation> {
    match &block.kind {
        BlockKind::Heading(level) => render_heading(ui, ps, *level, &block.spans, base_dir),
        BlockKind::Paragraph => render_paragraph(ui, ps, &block.spans, base_dir, drop_cap_here),
        BlockKind::CodeBlock { language } => {
            render_code_block(ui, ps, language.as_deref(), &block.spans);
            None
        }
        BlockKind::BlockQuote => render_blockquote(ui, ps, &block.spans, base_dir),
        BlockKind::ListItem {
            ordered,
            index,
            depth,
        } => render_list_item(ui, ps, *ordered, *index, *depth, &block.spans, base_dir),
        BlockKind::Rule => {
            ui.add_space(4.0);
            ui.separator();
            None
        }
        BlockKind::Table { header, rows, .. } => render_table(ui, ps, header, rows, base_dir),
    }
}

fn render_heading(
    ui: &mut egui::Ui,
    ps: &PreviewStyle,
    level: u8,
    spans: &[Span],
    base_dir: ImageContext<'_>,
) -> Option<WikilinkActivation> {
    let font = ps.heading_font(level);
    let clicked = render_spans_wrapped(ui, ps, spans, font, ps.text_color, base_dir);
    if ps.style.headings.page_break_before {
        ui.add_space(2.0);
        ui.separator();
    }
    clicked
}

/// Renders a paragraph as a single justifiable `LayoutJob` when it's plain text
/// (the common case for manuscript body copy), so `style.body.justify` can
/// actually take effect. A paragraph containing a wikilink or image needs real
/// interactive widgets egui can't express inside one `LayoutJob`, so those fall
/// back to the same wrapped multi-widget rendering headings/blockquotes use —
/// left-aligned regardless of `justify`, a known v1 limitation.
fn render_paragraph(
    ui: &mut egui::Ui,
    ps: &PreviewStyle,
    spans: &[Span],
    base_dir: ImageContext<'_>,
    drop_cap_here: bool,
) -> Option<WikilinkActivation> {
    let needs_widget = spans
        .iter()
        .any(|s| s.image.is_some() || s.wikilink.is_some());
    if needs_widget {
        let font = ps.body_font(ps.style.body.size_pt as f32);
        return render_spans_wrapped(ui, ps, spans, font, ps.text_color, base_dir);
    }
    let drop_cap = drop_cap_here.then_some(ps.style.drop_cap).flatten();
    let job = build_paragraph_job(ps, spans, drop_cap, ui.available_width());
    ui.add(egui::Label::new(job).wrap());
    None
}

/// Builds a whole-paragraph `LayoutJob` with `wrap.max_width`/`justify` set from
/// `style.body`, optionally rendering the first character oversized as a raised
/// drop cap (matches `export::pdf`'s own "raised, not sunk" cap — see
/// `DropCapStyle`'s doc comment — so Preview and PDF agree on what this looks
/// like) when `drop_cap` is `Some` — only ever passed for a document's first
/// paragraph, mirroring `export::pdf::blocks_to_typst`'s own
/// `first_paragraph_seen` gating.
fn build_paragraph_job(
    ps: &PreviewStyle,
    spans: &[Span],
    drop_cap: Option<DropCapStyle>,
    wrap_width: f32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width.max(1.0);
    job.justify = ps.style.body.justify;
    let base_size = ps.style.body.size_pt as f32;
    let base_font = ps.body_font(base_size);

    let mut start = 0;
    if let Some(DropCapStyle { scale }) = drop_cap
        && let Some(first) = spans.first()
        && let Some(ch) = first.text.chars().next()
    {
        let cap_font = FontId::new(base_size * scale, ps.body_family.clone());
        job.append(
            &ch.to_string(),
            0.0,
            TextFormat {
                font_id: cap_font,
                color: ps.text_color,
                ..Default::default()
            },
        );
        let rest_byte = ch.len_utf8();
        if rest_byte < first.text.len() {
            let mut remainder = first.clone();
            remainder.text = first.text[rest_byte..].to_string();
            append_span(&mut job, ps, &remainder, base_font.clone(), ps.text_color);
        }
        start = 1;
    }
    for span in &spans[start..] {
        append_span(&mut job, ps, span, base_font.clone(), ps.text_color);
    }
    job
}

fn render_code_block(ui: &mut egui::Ui, ps: &PreviewStyle, language: Option<&str>, spans: &[Span]) {
    egui::Frame::new()
        .fill(ps.code_bg)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            if let Some(lang) = language {
                ui.weak(RichText::new(lang).size(11.0));
            }
            let text: String = spans.iter().map(|s| s.text.as_str()).collect();
            ui.add(egui::Label::new(
                RichText::new(text.trim_end_matches('\n'))
                    .font(ps.code_font())
                    .color(ps.text_color),
            ));
        });
}

fn render_blockquote(
    ui: &mut egui::Ui,
    ps: &PreviewStyle,
    spans: &[Span],
    base_dir: ImageContext<'_>,
) -> Option<WikilinkActivation> {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(3.0, ui.spacing().interact_size.y),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(rect, 0.0, ps.quote_bar_color);
        let mut styled_spans = spans.to_vec();
        if ps.style.blockquote.italic {
            for span in &mut styled_spans {
                span.italic = true;
            }
        }
        render_spans_wrapped(
            ui,
            ps,
            &styled_spans,
            ps.quote_font(),
            ps.quote_text_color,
            base_dir,
        )
    })
    .inner
}

fn render_list_item(
    ui: &mut egui::Ui,
    ps: &PreviewStyle,
    ordered: bool,
    index: Option<u64>,
    depth: u8,
    spans: &[Span],
    base_dir: ImageContext<'_>,
) -> Option<WikilinkActivation> {
    let size = ps.style.body.size_pt as f32;
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * INDENT_PER_DEPTH);
        let bullet = if ordered {
            format!("{}.", index.unwrap_or(1))
        } else if depth == 0 {
            "•".to_string()
        } else {
            "◦".to_string()
        };
        ui.label(
            RichText::new(bullet)
                .color(ps.text_color)
                .font(ps.body_font(size))
                .strong(),
        );
        render_spans_wrapped(ui, ps, spans, ps.body_font(size), ps.text_color, base_dir)
    })
    .inner
}

/// Render a GFM table as a striped grid. Column alignment (`:---:` etc.) is parsed
/// into the block already but not yet reflected here — every cell is left-aligned.
fn render_table(
    ui: &mut egui::Ui,
    ps: &PreviewStyle,
    header: &[Vec<Span>],
    rows: &[Vec<Vec<Span>>],
    base_dir: ImageContext<'_>,
) -> Option<WikilinkActivation> {
    let size = ps.style.body.size_pt as f32;
    let mut clicked = None;
    egui::Frame::new()
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            egui::Grid::new(ui.id().with("md_table"))
                .striped(true)
                .spacing(egui::vec2(16.0, 6.0))
                .show(ui, |ui| {
                    for cell in header {
                        if let Some(activation) = render_spans_wrapped(
                            ui,
                            ps,
                            cell,
                            ps.body_font(size),
                            emphasize(ps.text_color, ps.dark_mode),
                            base_dir,
                        ) {
                            clicked = Some(activation);
                        }
                    }
                    ui.end_row();
                    for row in rows {
                        for cell in row {
                            if let Some(activation) = render_spans_wrapped(
                                ui,
                                ps,
                                cell,
                                ps.body_font(size),
                                ps.text_color,
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
/// `force_create`. Used for headings/blockquotes/lists/tables and any paragraph
/// containing a wikilink or image — see `render_paragraph` for why plain-text
/// paragraphs instead go through `build_paragraph_job` (justify support).
fn render_spans_wrapped(
    ui: &mut egui::Ui,
    ps: &PreviewStyle,
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
                    ui.label(build_inline_job(ps, &buffer, base_font.clone(), base_color));
                    buffer.clear();
                }
                render_image(ui, image, &span.text, base_dir);
            } else if let Some(target) = &span.wikilink {
                if !buffer.is_empty() {
                    ui.label(build_inline_job(ps, &buffer, base_font.clone(), base_color));
                    buffer.clear();
                }
                let response = ui.link(
                    RichText::new(&span.text)
                        .color(ps.link_color)
                        .font(base_font.clone()),
                );
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
            ui.label(build_inline_job(ps, &buffer, base_font, base_color));
        }
        clicked
    })
    .inner
}

fn build_inline_job(
    ps: &PreviewStyle,
    spans: &[Span],
    base_font: FontId,
    base_color: Color32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    for span in spans {
        append_span(&mut job, ps, span, base_font.clone(), base_color);
    }
    job
}

fn append_span(job: &mut LayoutJob, ps: &PreviewStyle, span: &Span, font: FontId, color: Color32) {
    let mut format = TextFormat {
        font_id: font,
        color,
        ..Default::default()
    };
    if span.bold {
        // egui has no bundled bold-weight font; like `RichText::strong()`, we
        // signal emphasis by pushing the color further from the background
        // rather than switching fonts.
        format.color = emphasize(color, ps.dark_mode);
    }
    if span.italic {
        format.italics = true;
    }
    if span.strikethrough {
        format.strikethrough = egui::Stroke::new(1.0, format.color);
    }
    if span.code {
        format.font_id = FontId::new(format.font_id.size * 0.95, ps.code_family.clone());
        format.background = ps.code_bg;
    }
    if span.link.is_some() {
        format.color = ps.link_color;
        format.underline = egui::Stroke::new(1.0, ps.link_color);
    }
    job.append(&span.text, 0.0, format);
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
    fn emphasize_moves_toward_white_in_dark_mode_and_black_in_light_mode() {
        let mid_gray = Color32::from_rgb(0x80, 0x80, 0x80);
        let dark = emphasize(mid_gray, true);
        let light = emphasize(mid_gray, false);
        assert!(dark.r() > mid_gray.r());
        assert!(light.r() < mid_gray.r());
    }

    #[test]
    fn resolve_family_matches_the_bundled_fonts_case_insensitively() {
        assert_eq!(
            resolve_family("libertinus serif", &[], egui::FontFamily::Proportional),
            EditorFont::LibertinusSerif.family()
        );
        assert_eq!(
            resolve_family("DEJAVU SANS MONO", &[], egui::FontFamily::Proportional),
            EditorFont::DejaVuSansMono.family()
        );
        assert_eq!(
            resolve_family("atkinson hyperlegible", &[], egui::FontFamily::Proportional),
            EditorFont::AtkinsonHyperlegible.family()
        );
    }

    #[test]
    fn resolve_family_falls_back_for_an_unrecognized_font_name() {
        assert_eq!(
            resolve_family("Comic Sans MS", &[], egui::FontFamily::Monospace),
            egui::FontFamily::Monospace
        );
    }

    #[test]
    fn resolve_family_matches_a_registered_custom_font_name_case_insensitively() {
        // The registered entry's own casing ("My Custom Font") is what
        // `install_custom_fonts` actually used as the family key — the result
        // must use that casing, not the differently-cased lookup arg, or it'd
        // reference a family that was never registered.
        let custom_fonts = vec!["My Custom Font".to_string()];
        assert_eq!(
            resolve_family(
                "my custom font",
                &custom_fonts,
                egui::FontFamily::Proportional
            ),
            egui::FontFamily::Name("My Custom Font".into())
        );
    }

    #[test]
    fn resolve_family_does_not_trust_an_unregistered_custom_font_name() {
        // A style's `font_file` might have failed to load — `custom_fonts` only
        // ever contains names that actually succeeded, so an empty list (as if
        // registration failed) must fall back, not optimistically build a
        // `FontFamily::Name` for something that was never registered (which
        // would panic the next time it's used to lay out text).
        assert_eq!(
            resolve_family("My Custom Font", &[], egui::FontFamily::Monospace),
            egui::FontFamily::Monospace
        );
    }

    fn plain_span(text: &str) -> Span {
        Span {
            text: text.to_string(),
            bold: false,
            italic: false,
            strikethrough: false,
            code: false,
            link: None,
            wikilink: None,
            image: None,
        }
    }

    fn preview_style(style: &TypesetStyle) -> PreviewStyle<'_> {
        PreviewStyle::new(&egui::Visuals::dark(), style, &[])
    }

    #[test]
    fn build_paragraph_job_honors_style_justify() {
        let styles = style::built_in_styles();
        let trade = style::find(&styles, "trade_paperback").unwrap();
        let manuscript = style::find(&styles, "manuscript").unwrap();

        let job = build_paragraph_job(&preview_style(trade), &[plain_span("Hello.")], None, 300.0);
        assert!(job.justify);

        let job = build_paragraph_job(
            &preview_style(manuscript),
            &[plain_span("Hello.")],
            None,
            300.0,
        );
        assert!(!job.justify);
    }

    #[test]
    fn build_paragraph_job_splits_off_a_drop_cap_from_the_first_span() {
        let styles = style::built_in_styles();
        let trade = style::find(&styles, "trade_paperback").unwrap();
        let ps = preview_style(trade);
        let drop_cap = trade.drop_cap;
        assert!(drop_cap.is_some());

        let job = build_paragraph_job(&ps, &[plain_span("Hello there.")], drop_cap, 300.0);
        assert_eq!(job.text, "Hello there.");
        // The first section is just the split-off capital letter, at the
        // enlarged size; the rest keeps the ordinary body size.
        let cap_size = drop_cap.unwrap().scale * ps.style.body.size_pt as f32;
        assert_eq!(job.sections[0].format.font_id.size, cap_size);
        let range = job.sections[0].byte_range.start.0..job.sections[0].byte_range.end.0;
        assert_eq!(&job.text[range], "H");
    }

    #[test]
    fn build_paragraph_job_without_drop_cap_keeps_a_single_section() {
        let styles = style::built_in_styles();
        let manuscript = style::find(&styles, "manuscript").unwrap();
        let ps = preview_style(manuscript);

        let job = build_paragraph_job(&ps, &[plain_span("Hello there.")], None, 300.0);
        assert_eq!(job.sections.len(), 1);
    }
}
