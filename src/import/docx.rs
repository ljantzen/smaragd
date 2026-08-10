//! Parses a `.docx` file into [`super::ImportedNode`]s — the mirror image of
//! `export::docx`, which walks `Block`/`Span` into `docx_rs` paragraphs/runs;
//! this walks `docx_rs`'s own parsed paragraphs/runs back into markdown.
//!
//! No new dependency needed: `docx_rs` (already used for export) ships a real
//! reader (`docx_rs::read_docx`, confirmed in the vendored crate source under
//! `src/reader/`), not just the builder API `export::docx` uses to write.

use docx_rs::{
    DocumentChild, Docx, ParagraphChild, ParagraphProperty, RunChild, TableCellContent, TableChild,
    TableRowChild,
};

use super::{ImportError, ImportedNode};
use crate::export::sanitize_filename_component;

/// The paragraph style id `export::docx` (and, empirically, Word itself)
/// gives a top-level heading — the boundary a DOCX import splits chapters at
/// (see this module's doc comment and the plan this came from for why:
/// matches smaragd's own one-chapter-per-document convention rather than
/// dumping a whole manuscript into a single file). A DOCX with no Heading 1
/// paragraphs at all becomes a single document instead.
const HEADING_1_STYLE_ID: &str = "Heading1";

/// Parses `bytes` (a `.docx` file's contents) into one [`ImportedNode`] per
/// Heading-1-delimited chapter, titled from that heading's own text — or, if
/// the document has no Heading 1 at all, a single node named `fallback_title`.
pub fn parse(bytes: &[u8], fallback_title: &str) -> Result<Vec<ImportedNode>, ImportError> {
    let doc = docx_rs::read_docx(bytes)?;
    Ok(split_into_chapters(&doc, fallback_title))
}

/// One in-progress chapter: a title (from its Heading 1, or `fallback_title`
/// for content before the first one/when there's no Heading 1 at all) and the
/// markdown accumulated for it so far.
struct Chapter {
    title: String,
    markdown: String,
}

fn split_into_chapters(doc: &Docx, fallback_title: &str) -> Vec<ImportedNode> {
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut current = Chapter {
        title: fallback_title.to_string(),
        markdown: String::new(),
    };
    let mut seen_heading_1 = false;

    for child in &doc.document.children {
        match child {
            DocumentChild::Paragraph(paragraph) => {
                let text = paragraph_text(&paragraph.children);
                if is_heading_1(&paragraph.property) {
                    if seen_heading_1 || !current.markdown.trim().is_empty() {
                        chapters.push(std::mem::replace(
                            &mut current,
                            Chapter {
                                title: String::new(),
                                markdown: String::new(),
                            },
                        ));
                    }
                    current.title = if text.trim().is_empty() {
                        format!("Chapter {}", chapters.len() + 1)
                    } else {
                        text.trim().to_string()
                    };
                    seen_heading_1 = true;
                } else if !text.trim().is_empty() {
                    current.markdown.push_str(text.trim());
                    current.markdown.push_str("\n\n");
                }
            }
            DocumentChild::Table(table) => {
                append_table_as_paragraphs(&mut current.markdown, table);
            }
            _ => {}
        }
    }
    if !current.markdown.trim().is_empty() || chapters.is_empty() {
        chapters.push(current);
    }

    chapters
        .into_iter()
        .map(|chapter| ImportedNode {
            name: sanitize_filename_component(&chapter.title),
            kind: super::ImportedKind::Document {
                markdown: chapter.markdown.trim().to_string(),
            },
        })
        .collect()
}

fn is_heading_1(property: &ParagraphProperty) -> bool {
    property
        .style
        .as_ref()
        .is_some_and(|style| style.val == HEADING_1_STYLE_ID)
}

/// A paragraph's plain text, wrapped `**bold**`/`_italic_`/`~~strike~~` per
/// run — presence-based (`Option::is_some()`), not reading `Bold`/`Italic`'s
/// own `val` (which is a private field with no public accessor; the rare
/// explicit "style says bold, this run overrides it off" case is treated the
/// same as "not bold" — a deliberate v1 simplification, not an oversight).
fn paragraph_text(children: &[ParagraphChild]) -> String {
    let mut out = String::new();
    for child in children {
        if let ParagraphChild::Run(run) = child {
            let mut text = String::new();
            for run_child in &run.children {
                if let RunChild::Text(t) = run_child {
                    text.push_str(&t.text);
                }
            }
            if text.is_empty() {
                continue;
            }
            // Leading/trailing whitespace stays outside the emphasis markers
            // (`bold ` -> `**bold** `, not `**bold **`) — markdown emphasis
            // wrapped around whitespace either fails to parse as emphasis at
            // all or looks visibly wrong once rendered. A whitespace-only run
            // skips emphasis entirely rather than emitting empty `****`/`__`.
            let trimmed = text.trim();
            if trimmed.is_empty() {
                out.push_str(&text);
                continue;
            }
            let leading = &text[..text.len() - text.trim_start().len()];
            let trailing = &text[leading.len() + trimmed.len()..];
            let mut emphasized = trimmed.to_string();
            if run.run_property.bold.is_some() {
                emphasized = format!("**{emphasized}**");
            }
            if run.run_property.italic.is_some() {
                emphasized = format!("_{emphasized}_");
            }
            if run.run_property.strike.is_some() {
                emphasized = format!("~~{emphasized}~~");
            }
            out.push_str(leading);
            out.push_str(&emphasized);
            out.push_str(trailing);
        }
    }
    out
}

/// Flattens a table into one plain paragraph per cell, in row order — not a
/// markdown pipe table. A real DOCX table can nest arbitrary block content
/// per cell (lists, nested tables, multiple paragraphs); reconstructing that
/// as markdown table syntax isn't attempted for v1, so this trades tabular
/// layout for guaranteed-readable plain text instead.
fn append_table_as_paragraphs(out: &mut String, table: &docx_rs::Table) {
    for row in &table.rows {
        let TableChild::TableRow(row) = row;
        for cell in &row.cells {
            let TableRowChild::TableCell(cell) = cell;
            for content in &cell.children {
                if let TableCellContent::Paragraph(paragraph) = content {
                    let text = paragraph_text(&paragraph.children);
                    if !text.trim().is_empty() {
                        out.push_str(text.trim());
                        out.push_str("\n\n");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docx_rs::{Docx, Paragraph, Run};

    fn docx_bytes(doc: Docx) -> Vec<u8> {
        let mut buf = Vec::new();
        doc.build()
            .pack(&mut std::io::Cursor::new(&mut buf))
            .unwrap();
        buf
    }

    #[test]
    fn parse_with_no_heading_1_produces_a_single_document_named_from_the_fallback() {
        let doc =
            Docx::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text("Plain text.")));
        let bytes = docx_bytes(doc);

        let nodes = parse(&bytes, "Imported Document").unwrap();

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "Imported Document");
        let super::super::ImportedKind::Document { markdown } = &nodes[0].kind else {
            panic!("expected a document node");
        };
        assert_eq!(markdown, "Plain text.");
    }

    #[test]
    fn parse_splits_into_one_document_per_heading_1() {
        let doc = Docx::new()
            .add_paragraph(
                Paragraph::new()
                    .add_run(Run::new().add_text("Chapter One"))
                    .style(HEADING_1_STYLE_ID),
            )
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("First chapter body.")))
            .add_paragraph(
                Paragraph::new()
                    .add_run(Run::new().add_text("Chapter Two"))
                    .style(HEADING_1_STYLE_ID),
            )
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Second chapter body.")));
        let bytes = docx_bytes(doc);

        let nodes = parse(&bytes, "fallback").unwrap();

        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "Chapter One");
        assert_eq!(nodes[1].name, "Chapter Two");
        let super::super::ImportedKind::Document { markdown } = &nodes[1].kind else {
            panic!("expected a document node");
        };
        assert_eq!(markdown, "Second chapter body.");
    }

    #[test]
    fn parse_preserves_bold_and_italic_formatting() {
        let doc = Docx::new().add_paragraph(
            Paragraph::new()
                .add_run(Run::new().add_text("bold ").bold())
                .add_run(Run::new().add_text("italic").italic()),
        );
        let bytes = docx_bytes(doc);

        let nodes = parse(&bytes, "fallback").unwrap();

        let super::super::ImportedKind::Document { markdown } = &nodes[0].kind else {
            panic!("expected a document node");
        };
        assert_eq!(markdown, "**bold** _italic_");
    }

    #[test]
    fn parse_keeps_content_before_the_first_heading_1_under_the_fallback_title() {
        let doc = Docx::new()
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Front matter.")))
            .add_paragraph(
                Paragraph::new()
                    .add_run(Run::new().add_text("Chapter One"))
                    .style(HEADING_1_STYLE_ID),
            )
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Chapter body.")));
        let bytes = docx_bytes(doc);

        let nodes = parse(&bytes, "Front Matter").unwrap();

        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "Front Matter");
        assert_eq!(nodes[1].name, "Chapter One");
    }
}
