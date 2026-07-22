use egui::{Color32, FontId, RichText, TextFormat, text::LayoutJob};

use crate::markdown::{self, Block, BlockKind, Span};

// Approximates the "dark"/dracula-family palette glow (charmbracelet/glow) renders
// markdown with in a terminal: pink/cyan/green/purple heading hierarchy, a muted
// blue-gray blockquote bar, and a dark panel for code.
const HEADING_COLORS: [Color32; 6] = [
    Color32::from_rgb(0xFF, 0x79, 0xC6), // h1 - pink
    Color32::from_rgb(0x8B, 0xE9, 0xFD), // h2 - cyan
    Color32::from_rgb(0x50, 0xFA, 0x7B), // h3 - green
    Color32::from_rgb(0xBD, 0x93, 0xF9), // h4 - purple
    Color32::from_rgb(0xF1, 0xFA, 0x8C), // h5 - yellow
    Color32::from_rgb(0xFF, 0xB8, 0x6C), // h6 - orange
];
const BODY_TEXT: Color32 = Color32::from_rgb(0xF8, 0xF8, 0xF2);
const QUOTE_BAR: Color32 = Color32::from_rgb(0x62, 0x72, 0xA4);
const QUOTE_TEXT: Color32 = Color32::from_rgb(0xA0, 0xA6, 0xC4);
const CODE_BG: Color32 = Color32::from_rgb(0x28, 0x2A, 0x36);
const CODE_INLINE_BG: Color32 = Color32::from_rgb(0x44, 0x47, 0x5A);
const LINK_COLOR: Color32 = Color32::from_rgb(0x8B, 0xE9, 0xFD);
const WIKILINK_COLOR: Color32 = Color32::from_rgb(0x50, 0xFA, 0x7B);

const BODY_SIZE: f32 = 15.0;
const BLOCK_SPACING: f32 = 10.0;
const INDENT_PER_DEPTH: f32 = 20.0;

/// Render `markdown` styled like the `glow` CLI's terminal preview: colored heading
/// hierarchy, a barred blockquote, and a boxed code block, laid out with egui widgets.
///
/// Returns `Some(target)` if the user clicked a `[[wikilink]]` during this frame, where
/// `target` is the note name to resolve — the caller is responsible for finding and
/// opening the matching document.
pub fn show(ui: &mut egui::Ui, markdown_text: &str) -> Option<String> {
    let blocks = markdown::parse(markdown_text);
    egui::ScrollArea::vertical()
        .id_salt("markdown_preview_scroll")
        .show(ui, |ui| {
            if blocks.is_empty() {
                ui.weak("Nothing to preview yet.");
                return None;
            }
            let mut clicked = None;
            for block in &blocks {
                if let Some(target) = render_block(ui, block) {
                    clicked = Some(target);
                }
                ui.add_space(BLOCK_SPACING);
            }
            clicked
        })
        .inner
}

fn render_block(ui: &mut egui::Ui, block: &Block) -> Option<String> {
    match &block.kind {
        BlockKind::Heading(level) => render_heading(ui, *level, &block.spans),
        BlockKind::Paragraph => {
            render_spans(ui, &block.spans, FontId::proportional(BODY_SIZE), BODY_TEXT)
        }
        BlockKind::CodeBlock { language } => {
            render_code_block(ui, language.as_deref(), &block.spans);
            None
        }
        BlockKind::BlockQuote => render_blockquote(ui, &block.spans),
        BlockKind::ListItem {
            ordered,
            index,
            depth,
        } => render_list_item(ui, *ordered, *index, *depth, &block.spans),
        BlockKind::Rule => {
            ui.add_space(4.0);
            ui.separator();
            None
        }
    }
}

fn render_heading(ui: &mut egui::Ui, level: u8, spans: &[Span]) -> Option<String> {
    let color = HEADING_COLORS[(level.saturating_sub(1).min(5)) as usize];
    let size = match level {
        1 => 28.0,
        2 => 24.0,
        3 => 20.0,
        4 => 18.0,
        5 => 16.5,
        _ => 15.5,
    };
    let clicked = render_spans(ui, spans, FontId::proportional(size), color);
    if level == 1 {
        ui.add_space(2.0);
        ui.separator();
    }
    clicked
}

fn render_code_block(ui: &mut egui::Ui, language: Option<&str>, spans: &[Span]) {
    egui::Frame::new()
        .fill(CODE_BG)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            if let Some(lang) = language {
                ui.weak(RichText::new(lang).size(11.0));
            }
            let text: String = spans.iter().map(|s| s.text.as_str()).collect();
            ui.add(egui::Label::new(
                RichText::new(text.trim_end_matches('\n'))
                    .font(FontId::monospace(BODY_SIZE))
                    .color(BODY_TEXT),
            ));
        });
}

fn render_blockquote(ui: &mut egui::Ui, spans: &[Span]) -> Option<String> {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(3.0, ui.spacing().interact_size.y),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(rect, 0.0, QUOTE_BAR);
        let mut italic_spans = spans.to_vec();
        for span in &mut italic_spans {
            span.italic = true;
        }
        render_spans(
            ui,
            &italic_spans,
            FontId::proportional(BODY_SIZE),
            QUOTE_TEXT,
        )
    })
    .inner
}

fn render_list_item(
    ui: &mut egui::Ui,
    ordered: bool,
    index: Option<u64>,
    depth: u8,
    spans: &[Span],
) -> Option<String> {
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * INDENT_PER_DEPTH);
        let bullet = if ordered {
            format!("{}.", index.unwrap_or(1))
        } else if depth == 0 {
            "•".to_string()
        } else {
            "◦".to_string()
        };
        ui.label(RichText::new(bullet).color(BODY_TEXT).strong());
        render_spans(ui, spans, FontId::proportional(BODY_SIZE), BODY_TEXT)
    })
    .inner
}

/// Render a run of spans, laying out plain-text runs as wrapped, styled text and each
/// `[[wikilink]]` span as a clickable link. Returns the clicked wikilink's target note
/// name, if any.
fn render_spans(
    ui: &mut egui::Ui,
    spans: &[Span],
    base_font: FontId,
    base_color: Color32,
) -> Option<String> {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut clicked = None;
        let mut buffer: Vec<Span> = Vec::new();
        for span in spans {
            if let Some(target) = &span.wikilink {
                if !buffer.is_empty() {
                    ui.label(build_layout_job(&buffer, base_font.clone(), base_color));
                    buffer.clear();
                }
                if ui
                    .link(RichText::new(&span.text).color(WIKILINK_COLOR))
                    .clicked()
                {
                    clicked = Some(target.clone());
                }
            } else {
                buffer.push(span.clone());
            }
        }
        if !buffer.is_empty() {
            ui.label(build_layout_job(&buffer, base_font, base_color));
        }
        clicked
    })
    .inner
}

/// Push `color` halfway toward white, approximating a "stronger" emphasis color
/// the way `RichText::strong()` does, since egui has no real bold font weight.
fn brighten(color: Color32) -> Color32 {
    Color32::from_rgb(
        color.r() + (255 - color.r()) / 2,
        color.g() + (255 - color.g()) / 2,
        color.b() + (255 - color.b()) / 2,
    )
}

fn build_layout_job(spans: &[Span], base_font: FontId, base_color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    for span in spans {
        let mut format = TextFormat {
            font_id: base_font.clone(),
            color: base_color,
            ..Default::default()
        };
        if span.bold {
            // egui has no bundled bold-weight font; like `RichText::strong()`, we
            // signal emphasis by brightening the color rather than switching fonts.
            format.color = brighten(base_color);
        }
        if span.italic {
            format.italics = true;
        }
        if span.strikethrough {
            format.strikethrough = egui::Stroke::new(1.0, format.color);
        }
        if span.code {
            format.font_id = FontId::monospace(base_font.size * 0.95);
            format.background = CODE_INLINE_BG;
        }
        if span.link.is_some() {
            format.color = LINK_COLOR;
            format.underline = egui::Stroke::new(1.0, LINK_COLOR);
        }
        job.append(&span.text, 0.0, format);
    }
    job
}
