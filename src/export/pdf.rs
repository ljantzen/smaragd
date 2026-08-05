//! Print-PDF rendering, backed by [Typst](https://typst.app) embedded as a
//! library via `typst-as-lib` — a real typesetting engine (page layout,
//! automatic widow/orphan avoidance, running headers) rather than placing
//! text on a page by hand. `Block`/`Span` is serialized into a single
//! generated `.typ` source string (a preamble from
//! [`super::style::TypesetStyle`], plus one section per `ExportDoc`), which
//! Typst then compiles and paginates itself — the same "generate markup, let
//! a real renderer lay it out" approach `export::epub` already takes with
//! HTML, just targeting Typst markup instead.
//!
//! Fonts: `typst-kit`'s embedded fonts (bundled from `typst-assets` at compile
//! time — no network, no separate install) always include "Libertinus Serif"
//! and "DejaVu Sans Mono", which is why the built-in styles
//! (`style::built_in_styles`) default to those names — every built-in style
//! is guaranteed to render with the exact requested font. System font search
//! is layered on top (`include_system_fonts(true)`) so a custom style naming
//! a locally-installed font still works; if neither has the requested name,
//! Typst falls back to *some* available font rather than erroring — a custom
//! style with a typo'd font name degrades rather than failing the export.
//!
//! No remote package resolution (`with_package_file_resolver`) is used
//! anywhere here — this app is otherwise fully offline-capable, and a print
//! export failing for "no internet" would be a bad surprise.

use std::fs;
use std::path::Path;

use typst_as_lib::TypstEngine;
use typst_as_lib::typst_kit_options::TypstKitFontOptions;

use super::style::{DropCapStyle, RunningHeaderStyle, TypesetStyle};
use super::{BookMeta, ExportDoc, ExportError};
use crate::markdown::{Block, BlockKind, Span};
use crate::ui::markdown_preview::resolve_image_fs_path;

/// Renders `docs` to a single print-ready PDF file at `out_path`, and returns
/// an estimated spine width in inches for the resulting page count (see
/// [`spine_width_inches`]) — informational only, not written into the PDF.
pub fn export_pdf(
    docs: &[ExportDoc],
    meta: &BookMeta,
    style: &TypesetStyle,
    project_root: &Path,
    out_path: &Path,
) -> Result<f32, ExportError> {
    let mut source = generate_preamble(meta, style);
    source.push_str(&title_page_typst(meta, style));
    for doc in docs {
        source.push_str("#pagebreak()\n");
        let doc_dir = doc.source_path.parent().unwrap_or(project_root);
        source.push_str(&blocks_to_typst(doc, doc_dir, project_root, style));
    }

    let template = TypstEngine::builder()
        .main_file(source)
        .search_fonts_with(
            TypstKitFontOptions::default()
                .include_system_fonts(true)
                .include_embedded_fonts(true),
        )
        .with_file_system_resolver(project_root)
        .build();

    let warned = template.compile();
    let doc = warned
        .output
        .map_err(|err| ExportError::Pdf(err.to_string()))?;

    let pdf_bytes = typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default())
        .map_err(|diags| ExportError::Pdf(format_diagnostics(&diags)))?;
    let page_count = doc.pages().len();
    fs::write(out_path, pdf_bytes)?;

    Ok(spine_width_inches(page_count))
}

/// `typst_pdf::pdf`'s error is a list of Typst source diagnostics with no
/// `Display` impl of its own (unlike `TypstAsLibError`, used above) — format
/// via `Debug` rather than pulling in `typst` as a direct dependency just to
/// name `SourceDiagnostic`.
fn format_diagnostics<T: std::fmt::Debug>(diags: &T) -> String {
    format!("{diags:?}")
}

/// A rough estimate, not a print-broker-grade figure — confirm against your
/// printer's own spine-width calculator (e.g. KDP's) before sending a cover
/// to print. Uses KDP's published white-paper thickness constant
/// (~0.002252in/page) as a reasonable default assumption.
const PAPER_THICKNESS_IN: f32 = 0.002252;

fn spine_width_inches(page_count: usize) -> f32 {
    page_count as f32 * PAPER_THICKNESS_IN
}

fn generate_preamble(meta: &BookMeta, style: &TypesetStyle) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "#set page(width: {}mm, height: {}mm, margin: {}mm{})\n",
        style.page.width_mm,
        style.page.height_mm,
        style.page.margin_mm,
        style
            .running_header
            .as_ref()
            .map(|rh| format!(
                ", header: context {{\n{}\n}}, footer: context [#align(center)[#counter(page).display()]]",
                running_header_body(rh, meta)
            ))
            .unwrap_or_default()
    ));
    s.push_str(&format!(
        "#set text(font: \"{}\", size: {}pt)\n",
        escape_typst(&style.body.font),
        style.body.size_pt
    ));
    // Typst's `leading` is *extra* space between lines on top of the font's
    // own natural gap, not a CSS-style total-height multiplier — this is an
    // approximation of `line_height` (a multiplier) translated into that
    // shape, not an exact conversion.
    let extra_leading = ((style.body.line_height - 1.0) * style.body.size_pt as f32).max(0.0);
    s.push_str(&format!(
        "#set par(justify: {}, leading: {extra_leading}pt + 0.65em)\n",
        style.body.justify
    ));
    for (level, size) in style.headings.sizes_pt.iter().enumerate() {
        s.push_str(&format!(
            "#show heading.where(level: {}): set text(font: \"{}\", size: {}pt, weight: \"bold\")\n",
            level + 1,
            escape_typst(&style.headings.font),
            size
        ));
    }
    if style.headings.page_break_before {
        s.push_str("#show heading.where(level: 1): it => { pagebreak(weak: true); it }\n");
    }
    s.push_str(&format!(
        "#set quote(block: true)\n#show quote: set text(font: \"{}\", size: {}pt, style: \"{}\")\n",
        escape_typst(&style.blockquote.font),
        style.blockquote.size_pt,
        if style.blockquote.italic {
            "italic"
        } else {
            "normal"
        }
    ));
    s.push_str(&format!(
        "#show raw: set text(font: \"{}\", size: {}pt)\n",
        escape_typst(&style.code.font),
        style.code.size_pt
    ));
    s
}

/// A centered Title/Subtitle/Author page rendered before the manuscript —
/// mirrors `docx::export_docx`'s equivalent title-page paragraphs. Empty when
/// none of the three fields are set, so a never-filled-in `BookMeta` (the
/// common case before a project's Export dialog has been used even once)
/// changes nothing about the resulting PDF versus before this existed; the
/// loop in `export_pdf` supplies the `#pagebreak()` that separates it from
/// the first chapter, same as it already does between every other pair of
/// chapters, so this doesn't add one of its own.
fn title_page_typst(meta: &BookMeta, style: &TypesetStyle) -> String {
    if meta.title.is_empty() && meta.subtitle.is_empty() && meta.author.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    s.push_str("#align(center + horizon)[\n");
    if !meta.title.is_empty() {
        s.push_str(&format!(
            "#text(font: \"{}\", size: 28pt, weight: \"bold\")[{}]\n\n",
            escape_typst(&style.headings.font),
            escape_typst(&meta.title)
        ));
    }
    if !meta.subtitle.is_empty() {
        s.push_str(&format!(
            "#v(0.8em)\n#text(font: \"{}\", size: 16pt, style: \"italic\")[{}]\n\n",
            escape_typst(&style.body.font),
            escape_typst(&meta.subtitle)
        ));
    }
    if !meta.author.is_empty() {
        s.push_str(&format!(
            "#v(2em)\n#text(font: \"{}\", size: 14pt)[{}]\n",
            escape_typst(&style.body.font),
            escape_typst(&meta.author)
        ));
    }
    s.push_str("]\n");
    s
}

fn running_header_body(rh: &RunningHeaderStyle, meta: &BookMeta) -> String {
    format!(
        "  let elems = query(selector(heading.where(level: 1)).before(here()))\n  \
         if elems.len() == 0 {{ return }}\n  \
         let heading_elem = elems.last()\n  \
         [{} #h(1fr) {}]",
        header_side(&rh.left, meta),
        header_side(&rh.right, meta),
    )
}

/// A running-header slot's generated content-mode snippet: `{chapter}` (and
/// only exactly that, not mixed with other text — a v1 simplification) uses
/// the current-chapter lookback; anything else is `{title}`/`{author}`-
/// substituted literal text.
fn header_side(template: &str, meta: &BookMeta) -> String {
    if template.trim() == "{chapter}" {
        "#heading_elem.body".to_string()
    } else {
        escape_typst(
            &template
                .replace("{title}", &meta.title)
                .replace("{subtitle}", &meta.subtitle)
                .replace("{author}", &meta.author),
        )
    }
}

fn blocks_to_typst(
    doc: &ExportDoc,
    doc_dir: &Path,
    project_root: &Path,
    style: &TypesetStyle,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("= {}\n\n", escape_typst(&doc.title)));
    let mut first_paragraph_seen = false;
    for block in &doc.blocks {
        let drop_cap_here = style.drop_cap.is_some()
            && !first_paragraph_seen
            && matches!(block.kind, BlockKind::Paragraph);
        if matches!(block.kind, BlockKind::Paragraph) {
            first_paragraph_seen = true;
        }
        append_typst_block(
            &mut out,
            block,
            drop_cap_here,
            style.drop_cap,
            doc_dir,
            project_root,
        );
    }
    out
}

fn append_typst_block(
    out: &mut String,
    block: &Block,
    drop_cap_here: bool,
    drop_cap: Option<DropCapStyle>,
    doc_dir: &Path,
    project_root: &Path,
) {
    match &block.kind {
        BlockKind::Heading(level) => {
            // Shifted one level deeper than the source markdown's own level
            // (clamped at Typst's max of 6): level 1 is reserved for the
            // synthetic chapter-title heading each document gets in
            // `blocks_to_typst`, so the running header's "most recent level-1
            // heading" lookback always finds the chapter title, never a
            // document's own `# H1`.
            let level = ((*level as usize) + 1).min(6);
            out.push_str(&"=".repeat(level));
            out.push(' ');
            spans_to_typst(out, &block.spans, doc_dir, project_root);
            out.push_str("\n\n");
        }
        BlockKind::Paragraph => {
            if drop_cap_here && let Some(drop_cap) = drop_cap {
                append_paragraph_with_drop_cap(
                    out,
                    &block.spans,
                    drop_cap.scale,
                    doc_dir,
                    project_root,
                );
            } else {
                spans_to_typst(out, &block.spans, doc_dir, project_root);
            }
            out.push_str("\n\n");
        }
        BlockKind::CodeBlock { .. } => {
            let text: String = block.spans.iter().map(|s| s.text.as_str()).collect();
            out.push_str("```\n");
            out.push_str(&text.replace("```", "` ` `"));
            out.push_str("\n```\n\n");
        }
        BlockKind::BlockQuote => {
            out.push_str("#quote[");
            spans_to_typst(out, &block.spans, doc_dir, project_root);
            out.push_str("]\n\n");
        }
        BlockKind::ListItem {
            ordered,
            index,
            depth,
        } => {
            out.push_str(&"  ".repeat(*depth as usize));
            if *ordered {
                out.push_str(&format!("{}. ", index.unwrap_or(1)));
            } else {
                out.push_str("- ");
            }
            spans_to_typst(out, &block.spans, doc_dir, project_root);
            out.push('\n');
        }
        BlockKind::Rule => out.push_str("#align(center)[\\* \\* \\*]\n\n"),
        BlockKind::Table { header, rows, .. } => {
            let columns = header.len().max(1);
            out.push_str(&format!("#table(columns: {columns},\n"));
            for cell in header {
                out.push_str("  [*");
                spans_to_typst(out, cell, doc_dir, project_root);
                out.push_str("*],\n");
            }
            for row in rows {
                for cell in row {
                    out.push_str("  [");
                    spans_to_typst(out, cell, doc_dir, project_root);
                    out.push_str("],\n");
                }
            }
            out.push_str(")\n\n");
        }
    }
}

/// Pulls the first character off `spans` and renders it oversized inline —
/// a *raised* cap, not a true multi-line-wrapping sunk drop cap (see
/// `DropCapStyle`'s doc comment for why).
fn append_paragraph_with_drop_cap(
    out: &mut String,
    spans: &[Span],
    scale: f32,
    doc_dir: &Path,
    project_root: &Path,
) {
    let Some(first_span) = spans
        .iter()
        .find(|s| s.image.is_none() && !s.text.is_empty())
    else {
        spans_to_typst(out, spans, doc_dir, project_root);
        return;
    };
    let mut chars = first_span.text.chars();
    let Some(first_char) = chars.next() else {
        spans_to_typst(out, spans, doc_dir, project_root);
        return;
    };
    let rest_of_span = chars.as_str().to_string();
    out.push_str(&format!(
        "#text(size: {scale}em)[{}]",
        escape_typst(&first_char.to_string())
    ));
    out.push_str(&escape_typst(&rest_of_span));
    let first_span_ptr = first_span as *const Span;
    for span in spans {
        if std::ptr::eq(span, first_span_ptr) {
            continue;
        }
        append_span_typst(out, span, doc_dir, project_root);
    }
}

fn spans_to_typst(out: &mut String, spans: &[Span], doc_dir: &Path, project_root: &Path) {
    for span in spans {
        append_span_typst(out, span, doc_dir, project_root);
    }
}

fn append_span_typst(out: &mut String, span: &Span, doc_dir: &Path, project_root: &Path) {
    if let Some(image) = &span.image {
        if let Some(reference) = typst_image_path(&image.src, doc_dir, project_root) {
            out.push_str(&format!("#image(\"{reference}\")\n"));
            return;
        }
        out.push_str(&escape_typst(&span.text));
        return;
    }
    let mut text = escape_typst(&span.text);
    if span.code {
        text = format!("`{}`", span.text.replace('`', "'"));
    } else {
        if span.bold {
            text = format!("*{text}*");
        }
        if span.italic {
            text = format!("_{text}_");
        }
        if span.strikethrough {
            text = format!("#strike[{text}]");
        }
    }
    out.push_str(&text);
    out.push(' ');
}

/// A `#image(...)` path resolvable against the `FileSystemResolver` rooted at
/// `project_root` — root-relative with a leading `/`, since the generated
/// source is an in-memory main file with no real location of its own to
/// resolve a same-directory-relative path against.
fn typst_image_path(src: &str, doc_dir: &Path, project_root: &Path) -> Option<String> {
    let path = resolve_image_fs_path(src, doc_dir, project_root)?;
    let relative = path.strip_prefix(project_root).ok()?;
    Some(format!(
        "/{}",
        relative.to_string_lossy().replace('\\', "/")
    ))
}

fn escape_typst(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(
            ch,
            '\\' | '*' | '_' | '`' | '#' | '$' | '[' | ']' | '<' | '>' | '@'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::style::built_in_styles;
    use crate::markdown;

    fn sample_blocks() -> Vec<Block> {
        markdown::parse(
            "# Heading\n\nA *paragraph* with **bold**, `code`, and a [[Wikilink]].\n\n\
             > A quote\n\n- one\n- two\n\n1. first\n2. second\n\n---\n\n\
             ```\ncode line\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
        )
    }

    fn manuscript_style() -> TypesetStyle {
        built_in_styles().remove(0)
    }

    fn trade_paperback_style() -> TypesetStyle {
        built_in_styles().remove(1)
    }

    #[test]
    fn title_page_typst_is_empty_for_a_default_book_meta() {
        assert_eq!(
            title_page_typst(&BookMeta::default(), &manuscript_style()),
            ""
        );
    }

    #[test]
    fn title_page_typst_includes_title_subtitle_and_author_when_set() {
        let meta = BookMeta {
            title: "My Book".to_string(),
            subtitle: "A Subtitle".to_string(),
            author: "Jane Doe".to_string(),
        };
        let page = title_page_typst(&meta, &manuscript_style());
        assert!(page.contains("My Book"));
        assert!(page.contains("A Subtitle"));
        assert!(page.contains("Jane Doe"));
    }

    #[test]
    fn header_side_substitutes_the_subtitle_token() {
        let meta = BookMeta {
            title: "My Book".to_string(),
            subtitle: "A Subtitle".to_string(),
            author: "Jane Doe".to_string(),
        };
        assert_eq!(header_side("{subtitle}", &meta), "A Subtitle");
    }

    #[test]
    fn escape_typst_escapes_markup_trigger_characters() {
        assert_eq!(
            escape_typst("a*b_c`d#e$f[g]h<i>j@k"),
            "a\\*b\\_c\\`d\\#e\\$f\\[g\\]h\\<i\\>j\\@k"
        );
        assert_eq!(escape_typst("plain text"), "plain text");
    }

    #[test]
    fn export_pdf_does_not_panic_on_every_block_kind() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![ExportDoc {
            title: "Chapter One".to_string(),
            blocks: sample_blocks(),
            source_path: dir.path().join("chapter1.md"),
        }];
        let meta = BookMeta {
            title: "My Book".to_string(),
            subtitle: "A Subtitle".to_string(),
            author: "Jane Doe".to_string(),
        };
        let out = dir.path().join("out.pdf");
        let spine = export_pdf(&docs, &meta, &manuscript_style(), dir.path(), &out).unwrap();
        assert!(out.exists());
        assert!(spine > 0.0);
    }

    #[test]
    fn export_pdf_with_drop_cap_and_running_header_style_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![
            ExportDoc {
                title: "Chapter One".to_string(),
                blocks: markdown::parse("First paragraph of chapter one.\n\nSecond paragraph."),
                source_path: dir.path().join("one.md"),
            },
            ExportDoc {
                title: "Chapter Two".to_string(),
                blocks: markdown::parse("First paragraph of chapter two."),
                source_path: dir.path().join("two.md"),
            },
        ];
        let meta = BookMeta {
            title: "My Book".to_string(),
            subtitle: "A Subtitle".to_string(),
            author: "Jane Doe".to_string(),
        };
        let out = dir.path().join("out.pdf");
        export_pdf(&docs, &meta, &trade_paperback_style(), dir.path(), &out).unwrap();
        assert!(out.exists());
    }

    #[test]
    fn spine_width_scales_with_page_count() {
        assert!(spine_width_inches(200) > spine_width_inches(100));
        assert_eq!(spine_width_inches(0), 0.0);
    }

    #[test]
    fn empty_book_meta_is_handled_without_a_title_or_author() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![ExportDoc {
            title: "Solo".to_string(),
            blocks: markdown::parse("Just text."),
            source_path: dir.path().join("solo.md"),
        }];
        let out = dir.path().join("solo.pdf");
        export_pdf(
            &docs,
            &BookMeta::default(),
            &manuscript_style(),
            dir.path(),
            &out,
        )
        .unwrap();
        assert!(out.exists());
    }
}
