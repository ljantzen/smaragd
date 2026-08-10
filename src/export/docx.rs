//! DOCX rendering: walks `Block`/`Span` into `docx_rs` paragraphs/runs, styled
//! from a shared [`super::style::TypesetStyle`] instead of hardcoded literals.
//! Drop caps are **not attempted here**  
//! Real running headers/page numbers and named heading styles are
//! attempted, via `docx-rs`'s `Header`/`Style` support.

use std::fs;
use std::path::Path;

use docx_rs::{
    AlignmentType, Docx, DocxError, Header, PageNum, Paragraph, Pic, Run, RunFonts, Style,
    StyleType, Table, TableCell, TableRow,
};

use super::style::TypesetStyle;
use super::{BookMeta, ExportDoc, ExportError};
use crate::markdown::{Block, BlockKind, Span};
use crate::ui::markdown_preview::resolve_image_fs_path;

const HEADING_STYLE_IDS: [&str; 6] = [
    "Heading1", "Heading2", "Heading3", "Heading4", "Heading5", "Heading6",
];

/// Renders `docs` to a single DOCX file at `out_path`: each `ExportDoc` becomes
/// a title heading followed by its body. Wikilinks render as plain
/// (non-linked) text and list items as a `"• "`/`"N. "` text prefix rather
/// than real DOCX list numbering — deliberate v1 simplifications, upgradable
/// later without changing this walk.
pub fn export_docx(
    docs: &[ExportDoc],
    meta: &BookMeta,
    style: &TypesetStyle,
    project_root: &Path,
    out_path: &Path,
) -> Result<(), ExportError> {
    let body_size = pt_to_half_points(style.body.size_pt);
    let mut docx = Docx::new().page_size(
        mm_to_twips(style.page.width_mm),
        mm_to_twips(style.page.height_mm),
    );

    for (i, size) in style.headings.sizes_pt.iter().enumerate() {
        docx = docx.add_style(
            Style::new(HEADING_STYLE_IDS[i], StyleType::Paragraph)
                .name(format!("Heading {}", i + 1))
                .bold()
                .size(pt_to_half_points(*size)),
        );
    }

    if let Some(running_header) = &style.running_header {
        let left = fill_running_header_template(&running_header.left, meta);
        let mut header_paragraph = Paragraph::new();
        if !left.is_empty() {
            header_paragraph = header_paragraph.add_run(Run::new().add_text(left));
        }
        header_paragraph = header_paragraph
            .add_run(Run::new().add_tab())
            .add_run(Run::new().add_tab())
            .add_page_num(PageNum::new());
        docx = docx.header(Header::new().add_paragraph(header_paragraph));
    }

    if !meta.title.is_empty() {
        docx = docx.add_paragraph(
            Paragraph::new()
                .add_run(
                    Run::new()
                        .add_text(meta.title.clone())
                        .bold()
                        .size(56)
                        .fonts(RunFonts::new().ascii(&style.body.font)),
                )
                .align(AlignmentType::Center),
        );
    }
    if !meta.subtitle.is_empty() {
        docx = docx.add_paragraph(
            Paragraph::new()
                .add_run(
                    Run::new()
                        .add_text(meta.subtitle.clone())
                        .italic()
                        .size(36)
                        .fonts(RunFonts::new().ascii(&style.body.font)),
                )
                .align(AlignmentType::Center),
        );
    }
    if !meta.author.is_empty() {
        docx = docx.add_paragraph(
            Paragraph::new()
                .add_run(
                    Run::new()
                        .add_text(meta.author.clone())
                        .size(28)
                        .fonts(RunFonts::new().ascii(&style.body.font)),
                )
                .align(AlignmentType::Center),
        );
    }

    for doc in docs {
        let mut title_paragraph = Paragraph::new().add_run(
            Run::new()
                .add_text(doc.title.clone())
                .bold()
                .size(36)
                .fonts(RunFonts::new().ascii(&style.headings.font)),
        );
        if style.headings.page_break_before {
            title_paragraph = title_paragraph.page_break_before(true);
        }
        docx = docx.add_paragraph(title_paragraph);
        let doc_dir = doc.source_path.parent().unwrap_or(project_root);
        for block in &doc.blocks {
            docx = append_docx_block(docx, block, doc_dir, project_root, style, body_size);
        }
    }

    let file = fs::File::create(out_path)?;
    docx.build().pack(file).map_err(DocxError::from)?;
    Ok(())
}

fn pt_to_half_points(pt: u32) -> usize {
    (pt.max(1) * 2) as usize
}

/// `docx-rs`'s `page_size` takes twentieths-of-a-point (twips); 1mm ≈ 56.7 twips.
fn mm_to_twips(mm: f32) -> u32 {
    (mm * 56.7).round() as u32
}

fn fill_running_header_template(template: &str, meta: &BookMeta) -> String {
    template
        .replace("{title}", &meta.title)
        .replace("{subtitle}", &meta.subtitle)
        .replace("{author}", &meta.author)
        // DOCX has no per-page "current chapter" query the way Typst does
        // (that needs a field/REF mechanism far more involved than this v1
        // warrants) — a running header that asks for `{chapter}` just omits
        // it here rather than showing a literal unfilled token.
        .replace("{chapter}", "")
        .trim()
        .to_string()
}

fn append_docx_block(
    docx: Docx,
    block: &Block,
    doc_dir: &Path,
    project_root: &Path,
    style: &TypesetStyle,
    body_size: usize,
) -> Docx {
    match &block.kind {
        BlockKind::Heading(level) => {
            let style_id = HEADING_STYLE_IDS[(*level as usize).saturating_sub(1).min(5)];
            let size = pt_to_half_points(
                style.headings.sizes_pt[(*level as usize).saturating_sub(1).min(5)],
            );
            let mut p = Paragraph::new().style(style_id);
            for span in &block.spans {
                p = p.add_run(docx_run(span, size, true, &style.headings.font));
            }
            docx.add_paragraph(p)
        }
        BlockKind::Paragraph => {
            let mut p = Paragraph::new();
            for span in &block.spans {
                if let Some(image) = &span.image {
                    if let Some(pic) = load_docx_pic(&image.src, doc_dir, project_root) {
                        p = p.add_run(Run::new().add_image(pic));
                        continue;
                    }
                    p = p.add_run(Run::new().add_text(span.text.clone()).italic());
                    continue;
                }
                p = p.add_run(docx_run(span, body_size, false, &style.body.font));
            }
            docx.add_paragraph(p)
        }
        BlockKind::CodeBlock { .. } => {
            let text: String = block.spans.iter().map(|s| s.text.as_str()).collect();
            let code_size = pt_to_half_points(style.code.size_pt);
            let mut d = docx;
            for line in text.lines() {
                d = d.add_paragraph(
                    Paragraph::new().add_run(
                        Run::new()
                            .add_text(line.to_string())
                            .size(code_size)
                            .fonts(RunFonts::new().ascii(&style.code.font)),
                    ),
                );
            }
            d
        }
        BlockKind::BlockQuote => {
            let quote_size = pt_to_half_points(style.blockquote.size_pt);
            let mut p = Paragraph::new().indent(Some(720), None, None, None);
            for span in &block.spans {
                let mut run = docx_run(span, quote_size, false, &style.blockquote.font);
                if style.blockquote.italic {
                    run = run.italic();
                }
                p = p.add_run(run);
            }
            docx.add_paragraph(p)
        }
        BlockKind::ListItem {
            ordered,
            index,
            depth,
        } => {
            let prefix = if *ordered {
                format!("{}. ", index.unwrap_or(1))
            } else {
                "• ".to_string()
            };
            let indent = (*depth as i32 + 1) * 480;
            let mut p = Paragraph::new()
                .indent(Some(indent), None, None, None)
                .add_run(
                    Run::new()
                        .add_text(prefix)
                        .size(body_size)
                        .fonts(RunFonts::new().ascii(&style.body.font)),
                );
            for span in &block.spans {
                p = p.add_run(docx_run(span, body_size, false, &style.body.font));
            }
            docx.add_paragraph(p)
        }
        BlockKind::Rule => docx.add_paragraph(
            Paragraph::new()
                .add_run(
                    Run::new()
                        .add_text("* * *")
                        .size(body_size)
                        .fonts(RunFonts::new().ascii(&style.body.font)),
                )
                .align(AlignmentType::Center),
        ),
        BlockKind::Table {
            header,
            rows,
            alignments: _,
        } => {
            let header_row = TableRow::new(
                header
                    .iter()
                    .map(|cell| {
                        let mut p = Paragraph::new();
                        for span in cell {
                            p = p
                                .add_run(docx_run(span, body_size, false, &style.body.font).bold());
                        }
                        TableCell::new().add_paragraph(p)
                    })
                    .collect(),
            );
            let mut table_rows = vec![header_row];
            for row in rows {
                table_rows.push(TableRow::new(
                    row.iter()
                        .map(|cell| {
                            let mut p = Paragraph::new();
                            for span in cell {
                                p = p.add_run(docx_run(span, body_size, false, &style.body.font));
                            }
                            TableCell::new().add_paragraph(p)
                        })
                        .collect(),
                ));
            }
            docx.add_table(Table::new(table_rows))
        }
    }
}

fn docx_run(span: &Span, size: usize, bold_heading: bool, font: &str) -> Run {
    let mut run = Run::new()
        .add_text(span.text.clone())
        .size(size)
        .fonts(RunFonts::new().ascii(font));
    if span.bold || bold_heading {
        run = run.bold();
    }
    if span.italic {
        run = run.italic();
    }
    if span.strikethrough {
        run = run.strike();
    }
    if span.code {
        run = run.fonts(RunFonts::new().ascii("Courier New"));
    }
    run
}

/// Resolves and reads an image span's bytes and builds a `Pic`, or `None` if the
/// path can't be resolved/read/decoded. `Pic::new` (via the `image` crate) panics
/// on malformed image data, so that call is wrapped in `catch_unwind` — a
/// corrupt image should degrade to alt text, not abort the whole export.
fn load_docx_pic(src: &str, doc_dir: &Path, project_root: &Path) -> Option<Pic> {
    let path = resolve_image_fs_path(src, doc_dir, project_root)?;
    let bytes = fs::read(path).ok()?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Pic::new(&bytes))).ok()
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
    fn fill_running_header_template_substitutes_the_subtitle_token() {
        let meta = BookMeta {
            title: "My Book".to_string(),
            subtitle: "A Subtitle".to_string(),
            author: "Jane Doe".to_string(),
        };
        assert_eq!(
            fill_running_header_template("{subtitle}", &meta),
            "A Subtitle"
        );
    }

    #[test]
    fn export_docx_does_not_panic_on_every_block_kind() {
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
        let out = dir.path().join("out.docx");
        export_docx(&docs, &meta, &manuscript_style(), dir.path(), &out).unwrap();
        assert!(out.exists());
    }

    #[test]
    fn export_docx_with_a_running_header_style_does_not_panic() {
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
        let out = dir.path().join("out.docx");
        export_docx(&docs, &meta, &trade_paperback_style(), dir.path(), &out).unwrap();
        assert!(out.exists());
    }

    #[test]
    fn export_docx_uses_the_style_body_font_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![ExportDoc {
            title: "Chapter One".to_string(),
            blocks: markdown::parse("Some body text."),
            source_path: dir.path().join("chapter1.md"),
        }];
        let mut style = manuscript_style();
        style.body.font = "Garamond".to_string();
        style.body.size_pt = 11;
        let out = dir.path().join("font.docx");
        export_docx(&docs, &BookMeta::default(), &style, dir.path(), &out).unwrap();

        let file = fs::File::open(&out).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        assert!(xml.contains("Garamond"));
        assert!(xml.contains("w:val=\"22\""));
    }

    #[test]
    fn empty_book_meta_is_handled_without_a_title_or_author() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![ExportDoc {
            title: "Solo".to_string(),
            blocks: markdown::parse("Just text."),
            source_path: dir.path().join("solo.md"),
        }];
        let out = dir.path().join("solo.docx");
        export_docx(
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
