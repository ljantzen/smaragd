//! EPUB rendering: walks `Block`/`Span` into XHTML chapters, styled from a
//! shared [`super::style::TypesetStyle`]'s generated `stylesheet.css` instead
//! of a single hardcoded font-family/size rule. Drop caps use the real CSS
//! `::first-letter` technique (well-supported by e-reader rendering engines)
//! — unlike DOCX, which skips drop caps entirely (see `docx.rs`'s doc comment).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};

use super::style::TypesetStyle;
use super::{BookMeta, ExportDoc, ExportError};
use crate::markdown::{Block, BlockKind, Span};
use crate::ui::markdown_preview::resolve_image_fs_path;

/// Renders `docs` to a single EPUB file at `out_path`. Each `ExportDoc` becomes
/// one XHTML chapter; a `[[wikilink]]` whose target matches another exported
/// document links to that document's chapter (case-insensitive, matching
/// `BinderNode::find_document_by_stem`), otherwise it renders as plain text.
pub fn export_epub(
    docs: &[ExportDoc],
    meta: &BookMeta,
    style: &TypesetStyle,
    project_root: &Path,
    out_path: &Path,
) -> Result<(), ExportError> {
    let chapter_files: HashMap<String, String> = docs
        .iter()
        .enumerate()
        .map(|(i, doc)| (doc.title.to_lowercase(), format!("chapter_{i}.xhtml")))
        .collect();

    let mut builder = EpubBuilder::new(ZipLibrary::new()?)?;
    let opf_title = combined_title(meta);
    if !opf_title.is_empty() {
        builder.metadata("title", opf_title)?;
    }
    if !meta.author.is_empty() {
        builder.metadata("author", meta.author.clone())?;
    }
    builder.stylesheet(stylesheet(style).as_bytes())?;

    let mut image_resources: HashMap<PathBuf, String> = HashMap::new();
    let mut image_counter = 0usize;

    for (i, doc) in docs.iter().enumerate() {
        let doc_dir = doc.source_path.parent().unwrap_or(project_root);
        let mut body = String::new();
        let mut first_paragraph_seen = false;
        for block in &doc.blocks {
            let drop_cap_here = style.drop_cap.is_some()
                && !first_paragraph_seen
                && matches!(block.kind, BlockKind::Paragraph);
            if matches!(block.kind, BlockKind::Paragraph) {
                first_paragraph_seen = true;
            }
            append_epub_block(
                &mut body,
                block,
                drop_cap_here,
                &chapter_files,
                doc_dir,
                project_root,
                &mut builder,
                &mut image_resources,
                &mut image_counter,
            );
        }
        let xhtml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{}</title><link rel=\"stylesheet\" type=\"text/css\" href=\"stylesheet.css\"/></head><body><h1>{}</h1>\n{}\n</body></html>",
            escape_html(&doc.title),
            escape_html(&doc.title),
            body
        );
        let filename = format!("chapter_{i}.xhtml");
        builder
            .add_content(EpubContent::new(filename, xhtml.as_bytes()).title(doc.title.clone()))?;
    }

    let file = fs::File::create(out_path)?;
    builder.generate(file)?;
    Ok(())
}

/// The EPUB `dc:title` value: `"{title}: {subtitle}"` when both are set, or
/// whichever one is. `epub_builder`'s `content.opf` template renders `<dc:
/// title>` without an `id` attribute (see its `templates/v2|v3/content.opf`),
/// so there's no element to `refines="#..."` a proper EPUB3 `title-type:
/// subtitle` `<meta>` against from outside the crate — the colon-joined
/// single title is what actually shows up in a reader's title display,
/// unlike a second, unlinked `<dc:title>` or an inert custom `<meta>` tag
/// that no reader is looking for.
fn combined_title(meta: &BookMeta) -> String {
    match (meta.title.is_empty(), meta.subtitle.is_empty()) {
        (true, true) => String::new(),
        (true, false) => meta.subtitle.clone(),
        (false, true) => meta.title.clone(),
        (false, false) => format!("{}: {}", meta.title, meta.subtitle),
    }
}

fn stylesheet(style: &TypesetStyle) -> String {
    let mut css = format!(
        "body {{ font-family: \"{}\", serif; font-size: {}pt; line-height: {}; text-align: {}; }}\n",
        style.body.font.replace('"', ""),
        style.body.size_pt.max(1),
        style.body.line_height,
        if style.body.justify {
            "justify"
        } else {
            "left"
        },
    );
    for (i, size) in style.headings.sizes_pt.iter().enumerate() {
        css.push_str(&format!(
            "h{} {{ font-family: \"{}\", serif; font-size: {}pt; }}\n",
            i + 1,
            style.headings.font.replace('"', ""),
            (*size).max(1)
        ));
    }
    css.push_str(&format!(
        "blockquote {{ font-family: \"{}\", serif; font-size: {}pt; font-style: {}; }}\n",
        style.blockquote.font.replace('"', ""),
        style.blockquote.size_pt.max(1),
        if style.blockquote.italic {
            "italic"
        } else {
            "normal"
        },
    ));
    css.push_str(&format!(
        "pre, code {{ font-family: \"{}\", monospace; font-size: {}pt; }}\n",
        style.code.font.replace('"', ""),
        style.code.size_pt.max(1)
    ));
    if let Some(drop_cap) = &style.drop_cap {
        css.push_str(&format!(
            "p.drop-cap::first-letter {{ font-size: {}em; float: left; line-height: 1; padding-right: 0.1em; }}\n",
            drop_cap.scale
        ));
    }
    css
}

#[allow(clippy::too_many_arguments)]
fn append_epub_block(
    out: &mut String,
    block: &Block,
    drop_cap_here: bool,
    chapter_files: &HashMap<String, String>,
    doc_dir: &Path,
    project_root: &Path,
    builder: &mut EpubBuilder<ZipLibrary>,
    image_resources: &mut HashMap<PathBuf, String>,
    image_counter: &mut usize,
) {
    match &block.kind {
        BlockKind::Heading(level) => {
            let level = (*level).clamp(1, 6);
            out.push_str(&format!("<h{level}>"));
            append_epub_spans(
                out,
                &block.spans,
                chapter_files,
                doc_dir,
                project_root,
                builder,
                image_resources,
                image_counter,
            );
            out.push_str(&format!("</h{level}>\n"));
        }
        BlockKind::Paragraph => {
            if drop_cap_here {
                out.push_str("<p class=\"drop-cap\">");
            } else {
                out.push_str("<p>");
            }
            append_epub_spans(
                out,
                &block.spans,
                chapter_files,
                doc_dir,
                project_root,
                builder,
                image_resources,
                image_counter,
            );
            out.push_str("</p>\n");
        }
        BlockKind::CodeBlock { .. } => {
            let text: String = block.spans.iter().map(|s| s.text.as_str()).collect();
            out.push_str(&format!("<pre><code>{}</code></pre>\n", escape_html(&text)));
        }
        BlockKind::BlockQuote => {
            out.push_str("<blockquote>");
            append_epub_spans(
                out,
                &block.spans,
                chapter_files,
                doc_dir,
                project_root,
                builder,
                image_resources,
                image_counter,
            );
            out.push_str("</blockquote>\n");
        }
        BlockKind::ListItem { ordered, .. } => {
            let tag = if *ordered { "ol" } else { "ul" };
            out.push_str(&format!("<{tag}><li>"));
            append_epub_spans(
                out,
                &block.spans,
                chapter_files,
                doc_dir,
                project_root,
                builder,
                image_resources,
                image_counter,
            );
            out.push_str(&format!("</li></{tag}>\n"));
        }
        BlockKind::Rule => out.push_str("<hr/>\n"),
        BlockKind::Table { header, rows, .. } => {
            out.push_str("<table><thead><tr>");
            for cell in header {
                out.push_str("<th>");
                append_epub_spans(
                    out,
                    cell,
                    chapter_files,
                    doc_dir,
                    project_root,
                    builder,
                    image_resources,
                    image_counter,
                );
                out.push_str("</th>");
            }
            out.push_str("</tr></thead><tbody>");
            for row in rows {
                out.push_str("<tr>");
                for cell in row {
                    out.push_str("<td>");
                    append_epub_spans(
                        out,
                        cell,
                        chapter_files,
                        doc_dir,
                        project_root,
                        builder,
                        image_resources,
                        image_counter,
                    );
                    out.push_str("</td>");
                }
                out.push_str("</tr>");
            }
            out.push_str("</tbody></table>\n");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn append_epub_spans(
    out: &mut String,
    spans: &[Span],
    chapter_files: &HashMap<String, String>,
    doc_dir: &Path,
    project_root: &Path,
    builder: &mut EpubBuilder<ZipLibrary>,
    image_resources: &mut HashMap<PathBuf, String>,
    image_counter: &mut usize,
) {
    for span in spans {
        if let Some(image) = &span.image
            && let Some(resource) = embed_epub_image(
                &image.src,
                doc_dir,
                project_root,
                builder,
                image_resources,
                image_counter,
            )
        {
            out.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\"/>",
                resource,
                escape_html(&span.text)
            ));
            continue;
        }
        let mut text = escape_html(&span.text);
        if span.bold {
            text = format!("<strong>{text}</strong>");
        }
        if span.italic {
            text = format!("<em>{text}</em>");
        }
        if span.strikethrough {
            text = format!("<s>{text}</s>");
        }
        if span.code {
            text = format!("<code>{text}</code>");
        }
        if let Some(target) = &span.wikilink
            && let Some(href) = chapter_files.get(&target.to_lowercase())
        {
            text = format!("<a href=\"{href}\">{text}</a>");
        }
        out.push_str(&text);
    }
}

fn embed_epub_image(
    src: &str,
    doc_dir: &Path,
    project_root: &Path,
    builder: &mut EpubBuilder<ZipLibrary>,
    image_resources: &mut HashMap<PathBuf, String>,
    image_counter: &mut usize,
) -> Option<String> {
    let path = resolve_image_fs_path(src, doc_dir, project_root)?;
    if let Some(existing) = image_resources.get(&path) {
        return Some(existing.clone());
    }
    let bytes = fs::read(&path).ok()?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => return None,
    };
    let resource_name = format!("images/img_{image_counter}.{ext}");
    *image_counter += 1;
    builder
        .add_resource(&resource_name, bytes.as_slice(), mime)
        .ok()?;
    image_resources.insert(path, resource_name.clone());
    Some(resource_name)
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
    fn combined_title_joins_title_and_subtitle_with_a_colon() {
        let meta = BookMeta {
            title: "My Book".to_string(),
            subtitle: "A Subtitle".to_string(),
            author: "Jane Doe".to_string(),
        };
        assert_eq!(combined_title(&meta), "My Book: A Subtitle");
    }

    #[test]
    fn combined_title_falls_back_to_whichever_of_title_or_subtitle_is_set() {
        let title_only = BookMeta {
            title: "My Book".to_string(),
            ..BookMeta::default()
        };
        assert_eq!(combined_title(&title_only), "My Book");

        let subtitle_only = BookMeta {
            subtitle: "A Subtitle".to_string(),
            ..BookMeta::default()
        };
        assert_eq!(combined_title(&subtitle_only), "A Subtitle");

        assert_eq!(combined_title(&BookMeta::default()), "");
    }

    #[test]
    fn export_epub_does_not_panic_on_every_block_kind() {
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
        let out = dir.path().join("out.epub");
        export_epub(&docs, &meta, &manuscript_style(), dir.path(), &out).unwrap();
        assert!(out.exists());
    }

    #[test]
    fn export_epub_links_a_wikilink_to_another_exported_chapter() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![
            ExportDoc {
                title: "Chapter One".to_string(),
                blocks: markdown::parse("See [[Chapter Two]]."),
                source_path: dir.path().join("one.md"),
            },
            ExportDoc {
                title: "Chapter Two".to_string(),
                blocks: markdown::parse("The end."),
                source_path: dir.path().join("two.md"),
            },
        ];
        let out = dir.path().join("linked.epub");
        export_epub(
            &docs,
            &BookMeta::default(),
            &manuscript_style(),
            dir.path(),
            &out,
        )
        .unwrap();
        assert!(out.exists());
    }

    #[test]
    fn export_epub_stylesheet_uses_the_style_body_font_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![ExportDoc {
            title: "Chapter One".to_string(),
            blocks: markdown::parse("Some body text."),
            source_path: dir.path().join("chapter1.md"),
        }];
        let mut style = manuscript_style();
        style.body.font = "Garamond".to_string();
        style.body.size_pt = 11;
        let out = dir.path().join("font.epub");
        export_epub(&docs, &BookMeta::default(), &style, dir.path(), &out).unwrap();

        let file = fs::File::open(&out).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut css = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("OEBPS/stylesheet.css").unwrap(),
            &mut css,
        )
        .unwrap();
        assert!(css.contains("Garamond"));
        assert!(css.contains("11pt"));
    }

    #[test]
    fn export_epub_with_a_drop_cap_style_marks_the_first_paragraph_and_defines_first_letter_css() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![ExportDoc {
            title: "Chapter One".to_string(),
            blocks: markdown::parse("# Heading\n\nFirst paragraph.\n\nSecond paragraph."),
            source_path: dir.path().join("chapter1.md"),
        }];
        let out = dir.path().join("dropcap.epub");
        export_epub(
            &docs,
            &BookMeta::default(),
            &trade_paperback_style(),
            dir.path(),
            &out,
        )
        .unwrap();

        let file = fs::File::open(&out).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut css = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("OEBPS/stylesheet.css").unwrap(),
            &mut css,
        )
        .unwrap();
        assert!(css.contains("::first-letter"));

        let mut chapter = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("OEBPS/chapter_0.xhtml").unwrap(),
            &mut chapter,
        )
        .unwrap();
        assert_eq!(chapter.matches("class=\"drop-cap\"").count(), 1);
        assert!(chapter.contains("First paragraph."));
    }

    #[test]
    fn empty_book_meta_is_handled_without_a_title_or_author() {
        let dir = tempfile::tempdir().unwrap();
        let docs = vec![ExportDoc {
            title: "Solo".to_string(),
            blocks: markdown::parse("Just text."),
            source_path: dir.path().join("solo.md"),
        }];
        let out = dir.path().join("solo.epub");
        export_epub(
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
