use std::path::Path;

use egui::{Color32, FontId, RichText, TextFormat, text::LayoutJob};

use crate::autocomplete::char_offset_to_byte;
use crate::editor_font::EditorFont;
use crate::export::is_within_project;
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
    verse_family: egui::FontFamily,
    text_color: Color32,
    quote_text_color: Color32,
    quote_bar_color: Color32,
    code_bg: Color32,
    link_color: Color32,
    /// Color for a `[[wikilink]]` whose target doesn't resolve to any
    /// document in the project, so a dead/mistyped/not-yet-created target
    /// is visible at a glance instead of looking like any other link.
    /// See `wikilink_color`.
    /// Themeable: this is just `visuals.error_fg_color`, which `color_theme::apply` sets to the
    /// active theme's `broken_wikilink`, falling back to plain egui default
    /// red with no theme active.
    broken_link_color: Color32,
    /// The project's document filenames (without extension) — what a
    /// wikilink target is checked against to decide `link_color` vs
    /// `broken_link_color`. Empty (so every wikilink renders as broken) when
    /// there's no project, e.g. viewing a joined collaboration session's
    /// shared content.
    note_titles: &'a [String],
    dark_mode: bool,
    /// Subtle translucent pill background painted behind an inline `#tag` span (see
    /// `append_span`) — derived from `link_color` rather than a separate themed field,
    /// since a tag isn't a distinct "broken/resolved" concept the way a wikilink is
    /// (every tag in a rendered document exists by definition), just a visually
    /// distinct kind of link.
    tag_bg_color: Color32,
    /// `Settings::resolve_preview_zoom()` — a multiplier applied to every font
    /// size this style would otherwise use (see `body_font`/`heading_font`/
    /// `quote_font`/`code_font`), so Ctrl+scrolling or the zoom shortcuts scale
    /// the rendered document without touching `style` itself (which stays
    /// whatever it'll actually export at).
    zoom: f32,
}

impl<'a> PreviewStyle<'a> {
    fn new(
        visuals: &egui::Visuals,
        style: &'a TypesetStyle,
        custom_fonts: &[String],
        note_titles: &'a [String],
        zoom: f32,
    ) -> Self {
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
            verse_family: resolve_family(
                &style.verse.font,
                custom_fonts,
                egui::FontFamily::Proportional,
            ),
            text_color: visuals.text_color(),
            quote_text_color: visuals.weak_text_color(),
            quote_bar_color: visuals.weak_text_color(),
            code_bg: visuals.code_bg_color,
            link_color: visuals.hyperlink_color,
            broken_link_color: visuals.error_fg_color,
            note_titles,
            dark_mode: visuals.dark_mode,
            tag_bg_color: visuals.hyperlink_color.gamma_multiply(0.16),
            zoom,
        }
    }

    /// `link_color` if `target` resolves to a document in the project,
    /// `broken_link_color` otherwise — see both fields' doc comments.
    fn wikilink_color(&self, target: &str) -> Color32 {
        if markdown::wikilink_resolves(target, self.note_titles) {
            self.link_color
        } else {
            self.broken_link_color
        }
    }

    /// `size` is the style's own, un-zoomed point size — every caller passes
    /// through the raw `*.size_pt` a `TypesetStyle` (and thus an export) would
    /// actually use; `zoom` is applied once, here, so it can't be missed at a
    /// call site or double-applied by one that also scales its input.
    fn body_font(&self, size: f32) -> FontId {
        FontId::new(size * self.zoom, self.body_family.clone())
    }

    fn heading_font(&self, level: u8) -> FontId {
        let size = self.style.headings.sizes_pt[(level.saturating_sub(1).min(5)) as usize] as f32;
        FontId::new(size * self.zoom, self.heading_family.clone())
    }

    fn quote_font(&self) -> FontId {
        FontId::new(
            self.style.blockquote.size_pt as f32 * self.zoom,
            self.quote_family.clone(),
        )
    }

    fn code_font(&self) -> FontId {
        FontId::new(
            self.style.code.size_pt as f32 * self.zoom,
            self.code_family.clone(),
        )
    }

    fn verse_font(&self) -> FontId {
        FontId::new(
            self.style.verse.size_pt as f32 * self.zoom,
            self.verse_family.clone(),
        )
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

/// What was clicked in the rendered preview text — either a `[[wikilink]]` (see
/// `WikilinkActivation`) or an inline `#tag` (see `inline_tag_spans`), each rendered
/// as its own distinctly-styled/hit-tested range within the same combined galley
/// (see `render_spans_wrapped`'s doc comment for why they're not separate widgets).
pub enum PreviewClick {
    Wikilink(WikilinkActivation),
    /// The tag name, without its leading `#`.
    Tag(String),
}

/// The Preview tab's `show()` result: a clicked wikilink/tag (if any) plus the new
/// style id, when the inline Style picker changed it this frame — the caller
/// (`app::dock_tab_viewer`) is responsible for both.
pub struct PreviewOutcome {
    pub click: Option<PreviewClick>,
    pub style_changed: Option<String>,
    /// New value for `Settings::preview_zoom`, when Ctrl+scrolling over the
    /// pane changed it this frame — see `show`'s doc comment on `preview_zoom`.
    /// Like `style_changed`, the caller (`app::dock_tab_viewer`) is
    /// responsible for persisting it.
    pub zoom_changed: Option<f32>,
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
///
/// `note_titles` (every document filename in the project, without extension —
/// the same list the Editor's wikilink autocomplete uses) decides which
/// `[[wikilink]]`s render as ordinary links versus "broken" links,
/// see `PreviewStyle::wikilink_color`.
///
/// `document_title` (the open document's filename stem, same as the
/// Editor's own heading above its text — see `editor_panel::show`) is shown
/// at the left of the top bar, alongside the Style picker, so it's clear
/// which document is being previewed even though the Preview tab itself is
/// just labeled "Preview".
///
/// `preview_zoom` is `Settings::resolve_preview_zoom()` — a multiplier applied
/// on top of every font size `style` would otherwise use (see
/// `PreviewStyle::zoom`). Ctrl+scrolling while the pointer is over this pane
/// changes it live and reports the new value via `PreviewOutcome::zoom_changed`
/// for the caller to persist, the same shape `style_changed` already uses;
/// `ShortcutAction::PreviewZoomIn`/`PreviewZoomOut`/reset are the keyboard
/// equivalent but — having no notion of "the pointer is over the Preview
/// tab" — are dispatched unconditionally by `app::dispatch_shortcut_action`
/// instead of routing through here.
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
    note_titles: &[String],
    document_title: Option<&str>,
    preview_zoom: f32,
) -> PreviewOutcome {
    let base_dir = ImageContext {
        dir: base_dir,
        project_root,
    };

    // A ctrl-held wheel event never reaches the `ScrollArea` below as a pan —
    // egui's `InputState::begin_pass` already diverts it into `zoom_delta()`
    // instead of `smooth_scroll_delta` the moment ctrl is held, regardless of
    // which widget is hovered — so the only thing left to do here is scope
    // *that* to this pane specifically, by gating on the pointer actually
    // being over it.
    let zoom_changed = (ui.rect_contains_pointer(ui.clip_rect())
        && ui.input(|i| i.zoom_delta()) != 1.0)
        .then(|| crate::settings::clamp_preview_zoom(preview_zoom * ui.input(|i| i.zoom_delta())));
    let preview_zoom = zoom_changed.unwrap_or(preview_zoom);

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
        if let Some(title) = document_title {
            ui.heading(title);
        }
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
            click: None,
            style_changed,
            zoom_changed,
        };
    };

    let mut blocks = markdown::parse(crate::frontmatter::strip(markdown_text));
    if typewriter_quotes {
        markdown::apply_typewriter_quotes(&mut blocks);
    }
    let ps = PreviewStyle::new(ui.visuals(), style, custom_fonts, note_titles, preview_zoom);

    let click = egui::ScrollArea::vertical()
        .id_salt("markdown_preview_scroll")
        .show(ui, |ui| {
            if blocks.is_empty() {
                ui.weak("Nothing to preview yet.");
                return None;
            }
            // Scaled by `preview_zoom` too, not just the fonts inside it — otherwise
            // zooming in just wraps the now-larger text harder inside an
            // unchanged-width column instead of also growing the column, leaving
            // the extra pane width unused (up to `ui.available_width()`, same as
            // at 100%).
            let content_width =
                ((style.page.width_mm - 2.0 * style.page.margin_mm) * PX_PER_MM * preview_zoom)
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
        click,
        style_changed,
        zoom_changed,
    }
}

fn render_block(
    ui: &mut egui::Ui,
    ps: &PreviewStyle,
    block: &Block,
    base_dir: ImageContext<'_>,
    drop_cap_here: bool,
) -> Option<PreviewClick> {
    match &block.kind {
        BlockKind::Heading(level) => render_heading(ui, ps, *level, &block.spans, base_dir),
        BlockKind::Paragraph => render_paragraph(ui, ps, &block.spans, base_dir, drop_cap_here),
        BlockKind::CodeBlock { language } => {
            render_code_block(ui, ps, language.as_deref(), &block.spans);
            None
        }
        BlockKind::Verse => {
            render_verse_block(ui, ps, &block.spans);
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
) -> Option<PreviewClick> {
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
/// actually take effect. A paragraph containing an image needs a real widget
/// egui can't express inside one `LayoutJob`, so those fall back to the same
/// wrapped rendering headings/blockquotes use — left-aligned regardless of
/// `justify`, a known v1 limitation. A wikilink alone doesn't force that
/// fallback (it's just a colored range within one combined galley — see
/// `render_spans_wrapped`), but still routes through the same fallback here
/// rather than duplicating that galley/hit-testing machinery into
/// `build_paragraph_job` too, so it's left-aligned the same as an image
/// paragraph would be.
fn render_paragraph(
    ui: &mut egui::Ui,
    ps: &PreviewStyle,
    spans: &[Span],
    base_dir: ImageContext<'_>,
    drop_cap_here: bool,
) -> Option<PreviewClick> {
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
        let cap_font = FontId::new(base_size * scale * ps.zoom, ps.body_family.clone());
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

/// Renders a `BlockKind::Verse` block's raw joined text (embedded `\n`s
/// preserved verbatim, same "join every span" trick `render_code_block` above
/// uses) as one `LayoutJob` run in the verse font, upright/italic per
/// `style.verse.italic` — deliberately not routed through
/// `render_spans_wrapped`/`append_span` the way `render_blockquote` is, since
/// a fenced verse block's spans carry only raw text (no bold/italic/wikilink
/// formatting to preserve — see `BlockKind::Verse`'s doc comment), so there's
/// only ever one formatting run to build. A literal `\n` inside `job.append`'s
/// text already forces a real line break in egui's own layout — the same
/// mechanism an explicit hard break (`Event::HardBreak`) already relies on
/// when it flows through the ordinary paragraph pipeline, not a new trick.
fn render_verse_block(ui: &mut egui::Ui, ps: &PreviewStyle, spans: &[Span]) {
    let text: String = spans.iter().map(|s| s.text.as_str()).collect();
    let mut job = LayoutJob::default();
    job.append(
        text.trim_end_matches('\n'),
        0.0,
        TextFormat {
            font_id: ps.verse_font(),
            color: ps.text_color,
            italics: ps.style.verse.italic,
            ..Default::default()
        },
    );
    ui.add(egui::Label::new(job));
}

fn render_blockquote(
    ui: &mut egui::Ui,
    ps: &PreviewStyle,
    spans: &[Span],
    base_dir: ImageContext<'_>,
) -> Option<PreviewClick> {
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
) -> Option<PreviewClick> {
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
) -> Option<PreviewClick> {
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

/// Render a run of spans as wrapped, styled text, with each `![image](src)` span
/// pulled out as a loaded image (falling back to its alt text while loading or on
/// error). Returns the clicked wikilink, if any — holding Ctrl (Cmd on macOS) while
/// clicking sets `force_create`. Used for headings/blockquotes/lists/tables and any
/// paragraph containing a wikilink or image — see `render_paragraph` for why
/// plain-text paragraphs instead go through `build_paragraph_job` (justify support).
///
/// A `[[wikilink]]` is *not* a separate widget here (unlike plain text runs, which
/// used to flush into their own `ui.label()`, with each wikilink as its own
/// `ui.link()` sitting beside them) — it's just a differently-colored/underlined
/// range within the same combined galley as its surrounding plain text (colored via
/// `append_span`, see `PreviewStyle::wikilink_color`), hit-tested against `wikilinks`
/// after the fact by `render_text_run`. GitHub issue #66: a `ui.link()` sitting next
/// to a `ui.label()` on the same wrapped line visibly diverged in size/alignment
/// from its neighbor for some (but not all) fonts, for reasons that resisted every
/// attempt to pin down in egui's own widget code; rendering the whole run as one
/// widget/one paint call sidesteps the question entirely — there's nothing left for
/// it to diverge *from*.
/// A clickable/hoverable range within a rendered `LayoutJob`: either a `[[wikilink]]`
/// or a `#tag`, tracked by `render_spans_wrapped`/`render_text_run` alongside the
/// byte range it occupies in the job's combined text.
enum ClickTarget {
    Wikilink(String),
    Tag(String),
}

fn render_spans_wrapped(
    ui: &mut egui::Ui,
    ps: &PreviewStyle,
    spans: &[Span],
    base_font: FontId,
    base_color: Color32,
    base_dir: ImageContext<'_>,
) -> Option<PreviewClick> {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut clicked = None;
        let mut job = LayoutJob::default();
        let mut targets: Vec<(std::ops::Range<usize>, ClickTarget)> = Vec::new();

        for span in spans {
            if let Some(image) = &span.image {
                if !job.text.is_empty() {
                    if let Some(activation) =
                        render_text_run(ui, ps, std::mem::take(&mut job), &targets)
                    {
                        clicked = Some(activation);
                    }
                    targets.clear();
                }
                render_image(ui, image, &span.text, base_dir);
            } else {
                let start = job.text.len();
                append_span(&mut job, ps, span, base_font.clone(), base_color);
                if let Some(target) = &span.wikilink {
                    targets.push((start..job.text.len(), ClickTarget::Wikilink(target.clone())));
                } else if let Some(tag) = &span.tag {
                    targets.push((start..job.text.len(), ClickTarget::Tag(tag.clone())));
                }
            }
        }
        if !job.text.is_empty()
            && let Some(activation) = render_text_run(ui, ps, job, &targets)
        {
            clicked = Some(activation);
        }
        clicked
    })
    .inner
}

/// Renders `job` (already fully styled, including any wikilinks'/tags' colors — see
/// `render_spans_wrapped`'s doc comment) as a single widget, then hit-tests the
/// mouse position against `targets` (byte ranges into `job.text`, paired with their
/// wikilink/tag) to decide hover cursor/tooltip and clicks — reimplements the
/// relevant slices of `egui::Label`/`egui::widgets::Link`'s own `Widget` impls
/// (`layout_in_ui` for the shared layout/wrapping logic, `LabelSelectionState` for
/// the same click-and-drag text selection a plain label gets) rather than calling
/// either directly, since neither has a hook for "part of this text is also a
/// click/hover target with its own tooltip."
fn render_text_run(
    ui: &mut egui::Ui,
    ps: &PreviewStyle,
    job: LayoutJob,
    targets: &[(std::ops::Range<usize>, ClickTarget)],
) -> Option<PreviewClick> {
    let label = egui::Label::new(job).sense(egui::Sense::click());
    let (galley_pos, galley, mut response) = label.layout_in_ui(ui);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), galley.text())
    });

    let hovered = response.hover_pos().and_then(|pos| {
        let char_index = galley.cursor_from_pos(pos - galley_pos).index.0;
        let byte_offset = char_offset_to_byte(&galley.job.text, char_index);
        targets
            .iter()
            .find(|(range, _)| range.contains(&byte_offset))
    });

    if ui.is_rect_visible(response.rect) {
        if ui.style().interaction.selectable_labels {
            egui::text_selection::LabelSelectionState::label_text_selection(
                ui,
                &response,
                galley_pos,
                galley,
                ps.text_color,
                egui::Stroke::NONE,
            );
        } else {
            ui.painter().add(egui::epaint::TextShape::new(
                galley_pos,
                galley,
                ps.text_color,
            ));
        }
    }

    if let Some((_, target)) = hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        response = response.on_hover_text(match target {
            ClickTarget::Wikilink(wikilink) => {
                if markdown::wikilink_resolves(wikilink, ps.note_titles) {
                    wikilink.clone()
                } else {
                    "No document found — Ctrl+click to create it".to_string()
                }
            }
            ClickTarget::Tag(tag) => format!("#{tag}"),
        });
    }

    if response.clicked()
        && let Some((_, target)) = hovered
    {
        return Some(match target {
            ClickTarget::Wikilink(wikilink) => {
                let force_create = ui.input(|i| i.modifiers.command);
                PreviewClick::Wikilink(WikilinkActivation {
                    target: wikilink.clone(),
                    force_create,
                })
            }
            ClickTarget::Tag(tag) => PreviewClick::Tag(tag.clone()),
        });
    }
    None
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
    if let Some(target) = &span.wikilink {
        format.color = ps.wikilink_color(target);
        format.underline = egui::Stroke::new(1.0, format.color);
    }
    if span.tag.is_some() {
        format.color = ps.link_color;
        format.background = ps.tag_bg_color;
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
            tag: None,
            image: None,
        }
    }

    fn wikilink_span(target: &str) -> Span {
        Span {
            text: target.to_string(),
            bold: false,
            italic: false,
            strikethrough: false,
            code: false,
            link: None,
            wikilink: Some(target.to_string()),
            tag: None,
            image: None,
        }
    }

    fn tag_span(tag: &str) -> Span {
        Span {
            text: format!("#{tag}"),
            bold: false,
            italic: false,
            strikethrough: false,
            code: false,
            link: None,
            wikilink: None,
            tag: Some(tag.to_string()),
            image: None,
        }
    }

    fn preview_style(style: &TypesetStyle) -> PreviewStyle<'_> {
        PreviewStyle::new(&egui::Visuals::dark(), style, &[], &[], 1.0)
    }

    #[test]
    fn zoom_scales_body_heading_quote_and_code_fonts() {
        let styles = style::built_in_styles();
        let style = style::find(&styles, "manuscript").unwrap();
        let ps = PreviewStyle::new(&egui::Visuals::dark(), style, &[], &[], 2.0);

        assert_eq!(
            ps.body_font(style.body.size_pt as f32).size,
            style.body.size_pt as f32 * 2.0
        );
        assert_eq!(
            ps.heading_font(1).size,
            style.headings.sizes_pt[0] as f32 * 2.0
        );
        assert_eq!(ps.quote_font().size, style.blockquote.size_pt as f32 * 2.0);
        assert_eq!(ps.code_font().size, style.code.size_pt as f32 * 2.0);
        assert_eq!(ps.verse_font().size, style.verse.size_pt as f32 * 2.0);
    }

    fn wikilink_or_tag(click: PreviewClick) -> String {
        match click {
            PreviewClick::Wikilink(activation) => activation.target,
            PreviewClick::Tag(tag) => tag,
        }
    }

    /// Simulates a full move/press/release click at the center of the `char_index`-th
    /// character of `spans` rendered as one combined `render_spans_wrapped` run
    /// (`font_id` must be monospace so each character's on-screen x position is
    /// exactly predictable — `char_width` is that fixed advance, measured by the
    /// caller's own warm-up frame). Shared by the wikilink- and tag-click tests below.
    fn click_at_char(
        ctx: &egui::Context,
        style: &TypesetStyle,
        note_titles: &[String],
        spans: &[Span],
        font_id: &FontId,
        char_width: f32,
        char_index: f32,
    ) -> Option<PreviewClick> {
        let mut clicked = None;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 200.0),
            )),
            ..Default::default()
        };
        let mut start = egui::Pos2::ZERO;
        let _ = ctx.run_ui(input.clone(), |ui| {
            start = ui.cursor().min;
        });
        let pos = start
            + egui::vec2(
                char_width * char_index,
                ctx.fonts_mut(|f| f.row_height(font_id)) / 2.0,
            );
        let render = |ui: &mut egui::Ui| -> Option<PreviewClick> {
            let ps = PreviewStyle::new(ui.visuals(), style, &[], note_titles, 1.0);
            render_spans_wrapped(
                ui,
                &ps,
                spans,
                font_id.clone(),
                ps.text_color,
                NO_IMAGE_CONTEXT,
            )
        };
        // Move, press, and release each get their own frame — a single combined
        // frame left `is_pointer_button_down_on()` false, as if the press event
        // was never attributed to the widget at all (seemingly because
        // `hovered()`, which the press attribution depends on, isn't settled
        // from a `PointerMoved` delivered in that same frame).
        let move_input = egui::RawInput {
            events: vec![egui::Event::PointerMoved(pos)],
            ..input.clone()
        };
        let _ = ctx.run_ui(move_input, |ui| {
            render(ui);
        });
        let press_input = egui::RawInput {
            events: vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            ..input.clone()
        };
        let _ = ctx.run_ui(press_input, |ui| {
            render(ui);
        });
        let release_input = egui::RawInput {
            events: vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..input
        };
        let _ = ctx.run_ui(release_input, |ui| {
            clicked = render(ui);
        });
        clicked
    }

    /// Regression test for GitHub issue #66: clicking plain text that shares a
    /// wrapped run with a wikilink must not activate the link, and clicking the
    /// wikilink itself must — verifying `render_text_run`'s hit-testing picks the
    /// right byte range out of the single combined galley `render_spans_wrapped`
    /// now renders (previously two adjacent widgets, `ui.label()` + `ui.link()`,
    /// each responsible for its own hit-testing).
    #[test]
    fn clicking_plain_text_next_to_a_wikilink_does_not_activate_it_but_clicking_the_link_does() {
        let ctx = egui::Context::default();
        crate::editor_font::install(&ctx);
        let font_id = EditorFont::DejaVuSansMono.font_id(20.0);

        // Warm-up frame: fonts (and thus glyph measurement) aren't available
        // until the first real pass — same reason other tests in this module
        // call `editor_font::install` before measuring/rendering.
        let char_width = {
            let mut width = 0.0;
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                width = ui
                    .fonts_mut(|f| f.layout_no_wrap("A".into(), font_id.clone(), Color32::WHITE))
                    .size()
                    .x;
            });
            width
        };

        let styles = style::built_in_styles();
        let style = style::find(&styles, "manuscript").unwrap();
        let note_titles = vec!["BB".to_string()];
        // Spans render as one combined run "AABBCC": "AA" plain, "BB" a
        // (resolved) wikilink, "CC" plain — monospace so each character's
        // on-screen x position is exactly predictable, letting the test click
        // precise character centers without needing to inspect the galley.
        let spans = [plain_span("AA"), wikilink_span("BB"), plain_span("CC")];

        let clicked_on_plain = click_at_char(
            &ctx,
            style,
            &note_titles,
            &spans,
            &font_id,
            char_width,
            0.5, /* inside "AA" */
        );
        assert!(
            clicked_on_plain.is_none(),
            "clicking plain text next to a wikilink activated it: {:?}",
            clicked_on_plain.map(wikilink_or_tag)
        );

        let clicked_on_link = click_at_char(
            &ctx,
            style,
            &note_titles,
            &spans,
            &font_id,
            char_width,
            2.5, /* inside "BB" */
        );
        assert_eq!(
            clicked_on_link.map(wikilink_or_tag),
            Some("BB".to_string()),
            "clicking the wikilink itself should have activated it"
        );
    }

    /// A `#tag` shares the same single-combined-galley hit-testing as a wikilink
    /// (see the #66 regression test above) — clicking it must report
    /// `PreviewClick::Tag`, not `Wikilink`, and clicking plain text next to it must
    /// not activate it.
    #[test]
    fn clicking_a_tag_activates_it_as_a_tag_not_a_wikilink() {
        let ctx = egui::Context::default();
        crate::editor_font::install(&ctx);
        let font_id = EditorFont::DejaVuSansMono.font_id(20.0);

        let char_width = {
            let mut width = 0.0;
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                width = ui
                    .fonts_mut(|f| f.layout_no_wrap("A".into(), font_id.clone(), Color32::WHITE))
                    .size()
                    .x;
            });
            width
        };

        let styles = style::built_in_styles();
        let style = style::find(&styles, "manuscript").unwrap();
        // Spans render as one combined run "AA#tagCC": "AA" plain, "#tag" a tag,
        // "CC" plain.
        let spans = [plain_span("AA"), tag_span("tag"), plain_span("CC")];

        let clicked_on_plain = click_at_char(
            &ctx,
            style,
            &[],
            &spans,
            &font_id,
            char_width,
            0.5, /* inside "AA" */
        );
        assert!(
            clicked_on_plain.is_none(),
            "clicking plain text next to a tag activated it: {:?}",
            clicked_on_plain.map(wikilink_or_tag)
        );

        let clicked_on_tag = click_at_char(
            &ctx,
            style,
            &[],
            &spans,
            &font_id,
            char_width,
            3.5, /* inside "#tag" */
        );
        match clicked_on_tag {
            Some(PreviewClick::Tag(tag)) => assert_eq!(tag, "tag"),
            other => panic!("expected a tag click, got {:?}", other.map(wikilink_or_tag)),
        }
    }

    const NO_IMAGE_CONTEXT: ImageContext<'static> = ImageContext {
        dir: None,
        project_root: None,
    };

    #[test]
    fn wikilink_color_is_the_link_color_when_the_target_resolves() {
        let styles = style::built_in_styles();
        let style = style::find(&styles, "manuscript").unwrap();
        let note_titles = vec!["Chapter 1".to_string()];
        let visuals = egui::Visuals::dark();
        let ps = PreviewStyle::new(&visuals, style, &[], &note_titles, 1.0);

        assert_eq!(ps.wikilink_color("Chapter 1"), visuals.hyperlink_color);
        assert_eq!(ps.wikilink_color("chapter 1"), visuals.hyperlink_color);
    }

    #[test]
    fn wikilink_color_is_the_broken_color_when_the_target_does_not_resolve() {
        let styles = style::built_in_styles();
        let style = style::find(&styles, "manuscript").unwrap();
        let note_titles = vec!["Chapter 1".to_string()];
        let visuals = egui::Visuals::dark();
        let ps = PreviewStyle::new(&visuals, style, &[], &note_titles, 1.0);

        assert_eq!(ps.wikilink_color("Chapter 2"), visuals.error_fg_color);
    }

    #[test]
    fn append_span_gives_a_tag_the_link_color_and_a_background_pill() {
        let styles = style::built_in_styles();
        let style = style::find(&styles, "manuscript").unwrap();
        let ps = preview_style(style);
        let span = tag_span("worldbuilding");

        let mut job = LayoutJob::default();
        append_span(&mut job, &ps, &span, ps.body_font(12.0), ps.text_color);

        let format = &job.sections[0].format;
        assert_eq!(format.color, ps.link_color);
        assert_eq!(format.background, ps.tag_bg_color);
    }

    #[test]
    fn append_span_leaves_plain_text_without_a_background() {
        let styles = style::built_in_styles();
        let style = style::find(&styles, "manuscript").unwrap();
        let ps = preview_style(style);
        let span = plain_span("just prose");

        let mut job = LayoutJob::default();
        append_span(&mut job, &ps, &span, ps.body_font(12.0), ps.text_color);

        let format = &job.sections[0].format;
        assert_eq!(format.color, ps.text_color);
        assert_eq!(format.background, Color32::TRANSPARENT);
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
