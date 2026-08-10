//! Parses a `.pdf` file into a single [`ImportedNode`] — the lowest-fidelity
//! of the four importers, by nature of the format: a plain PDF (not one
//! authored in Typst/LaTeX with embedded structure we could detect) has no
//! semantic markup at all, just glyphs positioned on a page. No chapter
//! splitting is attempted (no reliable per-chapter signal to key off in
//! extracted text), and no formatting survives — `pdf_extract` gives back
//! plain text only.
//!
//! New dependency: `pdf-extract` (pure Rust, built on `lopdf` — no native
//! library to bundle per platform, unlike e.g. `pdfium-render`, which would
//! complicate the existing 3-platform release pipeline,
//! `.github/workflows/release.yml`).

use super::{ImportError, ImportedKind, ImportedNode};
use crate::export::sanitize_filename_component;

/// Parses `bytes` (a `.pdf` file's contents) into a single [`ImportedNode`]
/// named from `fallback_title`. Paragraphs are reconstructed from blank-line-
/// separated runs in the extracted text — `pdf_extract` inserts a plain
/// newline per detected line of text, with no distinction between "new line,
/// same paragraph" and "new paragraph"; a blank line is the closest available
/// signal. A manuscript using first-line-indent instead of blank-line
/// paragraph spacing won't produce one — a known limitation of extracting
/// text from a format with no semantic structure to begin with, not
/// something worth fighting for v1 (see this module's doc comment).
pub fn parse(bytes: &[u8], fallback_title: &str) -> Result<Vec<ImportedNode>, ImportError> {
    let text = pdf_extract::extract_text_from_mem(bytes)?;
    Ok(vec![ImportedNode {
        name: sanitize_filename_component(fallback_title),
        kind: ImportedKind::Document {
            markdown: paragraphs_from_plain_text(&text),
        },
    }])
}

/// Groups `text`'s lines into paragraphs at blank lines, joining each
/// paragraph's own lines with a single space (a plain-text line-wrap isn't a
/// markdown hard break — see this module's doc comment).
fn paragraphs_from_plain_text(text: &str) -> String {
    let mut paragraphs = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }
    paragraphs.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_from_plain_text_joins_wrapped_lines_and_splits_on_blank_lines() {
        let text = "It was a dark\nand stormy night.\n\nThe rain fell\nin torrents.";

        let markdown = paragraphs_from_plain_text(text);

        assert_eq!(
            markdown,
            "It was a dark and stormy night.\n\nThe rain fell in torrents."
        );
    }

    #[test]
    fn paragraphs_from_plain_text_collapses_multiple_blank_lines() {
        let text = "First.\n\n\n\nSecond.";

        assert_eq!(paragraphs_from_plain_text(text), "First.\n\nSecond.");
    }

    #[test]
    fn paragraphs_from_plain_text_is_empty_for_blank_input() {
        assert_eq!(paragraphs_from_plain_text("\n\n   \n\n"), "");
    }
}
