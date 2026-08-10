//! Parses an `.epub` file into [`super::ImportedNode`]s — the mirror image
//! of `export::epub`, which walks `Block`/`Span` into XHTML chapters; this
//! walks XHTML chapters back into markdown.
//!
//! New dependencies: `zip` (already used read-only in `export::docx`/
//! `export::epub`'s own tests — an EPUB is a zip of XHTML/CSS/OPF/NCX, same
//! as a DOCX is a zip of XML) and `quick-xml` (already present transitively
//! via `docx-rs`, promoted to a direct dependency here).

use std::collections::HashMap;
use std::fmt;
use std::io::{self, Cursor, Read, Seek};

use quick_xml::Reader;
use quick_xml::events::Event;

use super::ImportedNode;
use crate::export::sanitize_filename_component;

#[derive(Debug)]
pub enum EpubImportError {
    Io(io::Error),
    Zip(zip::result::ZipError),
    /// `META-INF/container.xml` didn't contain a `<rootfile full-path="...">`
    /// pointing at the package document — not a well-formed EPUB.
    MissingOpfRootfile,
}

impl fmt::Display for EpubImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EpubImportError::Io(err) => write!(f, "{err}"),
            EpubImportError::Zip(err) => write!(f, "{err}"),
            EpubImportError::MissingOpfRootfile => {
                write!(
                    f,
                    "couldn't find the package document referenced by META-INF/container.xml"
                )
            }
        }
    }
}

impl From<io::Error> for EpubImportError {
    fn from(err: io::Error) -> Self {
        EpubImportError::Io(err)
    }
}

impl From<zip::result::ZipError> for EpubImportError {
    fn from(err: zip::result::ZipError) -> Self {
        EpubImportError::Zip(err)
    }
}

/// Parses `bytes` (an `.epub` file's contents) into one [`ImportedNode`] per
/// spine item, in reading order, titled from that chapter's own first
/// heading (or `Chapter N` if it has none) — EPUB's spine is already a
/// well-defined chapter order, unlike DOCX's heading-style heuristic.
pub fn parse(bytes: &[u8]) -> Result<Vec<ImportedNode>, super::ImportError> {
    parse_inner(bytes).map_err(super::ImportError::from)
}

fn parse_inner(bytes: &[u8]) -> Result<Vec<ImportedNode>, EpubImportError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let container_xml = read_zip_entry(&mut archive, "META-INF/container.xml")?;
    let opf_path = extract_opf_path(&container_xml).ok_or(EpubImportError::MissingOpfRootfile)?;
    let opf_dir = opf_dir(&opf_path);
    let opf_xml = read_zip_entry(&mut archive, &opf_path)?;
    let (manifest, spine) = parse_opf(&opf_xml);

    let mut nodes = Vec::new();
    for (index, idref) in spine.iter().enumerate() {
        let Some(href) = manifest.get(idref) else {
            continue;
        };
        let full_path = format!("{opf_dir}{href}");
        let Ok(xhtml) = read_zip_entry(&mut archive, &full_path) else {
            continue;
        };
        let (title, markdown) = xhtml_to_markdown(&xhtml, index + 1);
        nodes.push(ImportedNode {
            name: sanitize_filename_component(&title),
            kind: super::ImportedKind::Document { markdown },
        });
    }
    Ok(nodes)
}

fn read_zip_entry<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<String, EpubImportError> {
    let mut file = archive.by_name(name)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

/// An attribute's value, decoded as UTF-8 — not entity-unescaped (`&...;`),
/// unlike `text_content` below. Every attribute this module reads (`full-path`,
/// `id`, `href`, `idref`) is a plain file path or identifier, never prose, so
/// this is a deliberate simplification, not an oversight: it sidesteps
/// `Attribute::unescape_value` being `cfg`'d out under quick-xml's `encoding`
/// feature (enabled transitively in this build via `docx-rs`'s own quick-xml
/// dependency — cargo unifies features crate-wide, so it's on for us too).
fn attr_value(attr: &quick_xml::events::attributes::Attribute) -> Option<String> {
    std::str::from_utf8(&attr.value).ok().map(str::to_string)
}

/// The package document's path, from `META-INF/container.xml`'s
/// `<rootfile full-path="...">` — not hardcoded to `OEBPS/content.opf`
/// (what `export::epub`'s own output always uses) since a third-party EPUB
/// is free to put it anywhere (`EPUB/package.opf` is another common choice).
fn extract_opf_path(container_xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(container_xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"rootfile" => {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"full-path"
                        && let Some(value) = attr_value(&attr)
                    {
                        return Some(value);
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

/// The package document's own directory, as a `/`-terminated prefix (empty
/// string if it's at the zip's root) — every `href` in its manifest is
/// relative to this, not to the zip root.
fn opf_dir(opf_path: &str) -> String {
    match opf_path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/"),
        None => String::new(),
    }
}

/// The OPF's manifest (`id` -> `href`) and spine (`idref`s, in reading order).
fn parse_opf(opf_xml: &str) -> (HashMap<String, String>, Vec<String>) {
    let mut manifest = HashMap::new();
    let mut spine = Vec::new();
    let mut reader = Reader::from_str(opf_xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"item" => {
                    let mut id = None;
                    let mut href = None;
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"id" => id = attr_value(&attr),
                            b"href" => href = attr_value(&attr),
                            _ => {}
                        }
                    }
                    if let (Some(id), Some(href)) = (id, href) {
                        manifest.insert(id, href);
                    }
                }
                b"itemref" => {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"idref"
                            && let Some(value) = attr_value(&attr)
                        {
                            spine.push(value);
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (manifest, spine)
}

/// One chapter XHTML file's own text, decoded — `Event::Text` never contains
/// a literal `&...;` sequence itself (an entity reference tokenizes as its
/// own separate `Event::GeneralRef`, handled where that event is matched
/// below), so only byte-decoding is needed here, not entity-unescaping.
fn text_content(e: &quick_xml::events::BytesText) -> String {
    e.decode().unwrap_or_default().into_owned()
}

/// Wraps `text` in markdown emphasis markers for whichever of bold/italic/
/// strike/code is currently "open" (depth > 0) — shared by `Event::Text` and
/// `Event::GeneralRef` (an entity reference still needs the surrounding
/// run's formatting applied, same as plain text).
fn apply_emphasis(text: &str, bold: u32, italic: u32, strike: u32, code: u32) -> String {
    if code > 0 {
        return format!("`{}`", text.replace('`', "'"));
    }
    let mut text = text.to_string();
    if bold > 0 {
        text = format!("**{text}**");
    }
    if italic > 0 {
        text = format!("_{text}_");
    }
    if strike > 0 {
        text = format!("~~{text}~~");
    }
    text
}

/// Walks one chapter's XHTML into `(title, markdown)`: `<strong>`/`<em>`/
/// `<s>`/`<code>` become `**..**`/`_.._`/`~~..~~`/`` `..` `` per the exact
/// inverse of `export::epub::append_epub_spans`; `<p>`/`<h1>`-`<h6>`/
/// `<blockquote>`/`<li>` become markdown blocks. The chapter's first heading
/// (any level) becomes `title` and is *not* included in `markdown` — it
/// becomes the binder document's own name instead, the same way a DOCX
/// import's Heading-1-styled paragraph names its chapter rather than
/// appearing twice. A chapter with no heading at all falls back to
/// `Chapter {chapter_index}`.
fn xhtml_to_markdown(xhtml: &str, chapter_index: usize) -> (String, String) {
    let mut reader = Reader::from_str(xhtml);
    let mut out = String::new();
    let mut block = String::new();
    let mut title: Option<String> = None;

    let mut bold = 0u32;
    let mut italic = 0u32;
    let mut strike = 0u32;
    let mut code = 0u32;
    let mut heading_level: Option<u8> = None;
    let mut ordered_list = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"strong" | b"b" => bold += 1,
                b"em" | b"i" => italic += 1,
                b"s" | b"strike" | b"del" => strike += 1,
                b"code" => code += 1,
                b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => {
                    heading_level = Some(e.name().as_ref()[1] - b'0');
                    block.clear();
                }
                b"p" | b"blockquote" | b"li" => block.clear(),
                b"ol" => ordered_list = true,
                b"ul" => ordered_list = false,
                _ => {}
            },
            Ok(Event::Empty(e)) if e.name().as_ref() == b"br" => {
                block.push_str("  \n");
            }
            Ok(Event::Empty(e)) if e.name().as_ref() == b"hr" => {
                out.push_str("---\n\n");
            }
            Ok(Event::Text(e)) => {
                block.push_str(&apply_emphasis(
                    &text_content(&e),
                    bold,
                    italic,
                    strike,
                    code,
                ));
            }
            // A named (`&amp;`) or numeric (`&#8212;`/`&#x2014;`) entity — its
            // own event, not embedded in the surrounding `Event::Text` (see
            // `text_content`'s doc comment for the two-step decode this
            // mirrors for plain text). `BytesRef::resolve_char_ref` only
            // handles the numeric form; a named entity falls back to
            // `resolve_predefined_entity` (XML's 5 built-ins — `amp`/`lt`/
            // `gt`/`quot`/`apos` — the only ones well-formed XHTML can use
            // without its own DTD declaring more), and an entity neither
            // resolves is re-emitted literally (`&name;`) rather than
            // silently dropped.
            Ok(Event::GeneralRef(e)) => {
                let resolved = match e.resolve_char_ref() {
                    Ok(Some(ch)) => ch.to_string(),
                    _ => {
                        let name = e.decode().unwrap_or_default();
                        match quick_xml::escape::resolve_predefined_entity(&name) {
                            Some(resolved) => resolved.to_string(),
                            None => format!("&{name};"),
                        }
                    }
                };
                block.push_str(&apply_emphasis(&resolved, bold, italic, strike, code));
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"strong" | b"b" => bold = bold.saturating_sub(1),
                b"em" | b"i" => italic = italic.saturating_sub(1),
                b"s" | b"strike" | b"del" => strike = strike.saturating_sub(1),
                b"code" => code = code.saturating_sub(1),
                b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => {
                    let level = heading_level.take().unwrap_or(1);
                    let text = block.trim().to_string();
                    block.clear();
                    if title.is_none() {
                        title = Some(text);
                    } else if !text.is_empty() {
                        out.push_str(&"#".repeat(level as usize));
                        out.push(' ');
                        out.push_str(&text);
                        out.push_str("\n\n");
                    }
                }
                b"p" => {
                    if !block.trim().is_empty() {
                        out.push_str(block.trim());
                        out.push_str("\n\n");
                    }
                    block.clear();
                }
                b"blockquote" => {
                    if !block.trim().is_empty() {
                        out.push_str("> ");
                        out.push_str(block.trim());
                        out.push_str("\n\n");
                    }
                    block.clear();
                }
                b"li" => {
                    if !block.trim().is_empty() {
                        out.push_str(if ordered_list { "1. " } else { "- " });
                        out.push_str(block.trim());
                        out.push('\n');
                    }
                    block.clear();
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    let title = title
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| format!("Chapter {chapter_index}"));
    (title, out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_opf_path_reads_the_rootfile_full_path_attribute() {
        let container = r#"<?xml version="1.0"?>
<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;

        assert_eq!(
            extract_opf_path(container),
            Some("OEBPS/content.opf".to_string())
        );
    }

    #[test]
    fn opf_dir_strips_the_filename() {
        assert_eq!(opf_dir("OEBPS/content.opf"), "OEBPS/");
        assert_eq!(opf_dir("content.opf"), "");
    }

    #[test]
    fn parse_opf_reads_manifest_and_spine_in_order() {
        let opf = r#"<?xml version="1.0"?>
<package><manifest>
<item id="c1" href="chapter_0.xhtml" media-type="application/xhtml+xml"/>
<item id="c2" href="chapter_1.xhtml" media-type="application/xhtml+xml"/>
</manifest><spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#;

        let (manifest, spine) = parse_opf(opf);

        assert_eq!(spine, vec!["c1".to_string(), "c2".to_string()]);
        assert_eq!(manifest.get("c1"), Some(&"chapter_0.xhtml".to_string()));
        assert_eq!(manifest.get("c2"), Some(&"chapter_1.xhtml".to_string()));
    }

    #[test]
    fn xhtml_to_markdown_extracts_the_first_heading_as_title_and_excludes_it_from_the_body() {
        let xhtml = r#"<html><head><title>Chapter One</title></head><body><h1>Chapter One</h1>
<p>It was a <em>dark</em> and <strong>stormy</strong> night.</p>
</body></html>"#;

        let (title, markdown) = xhtml_to_markdown(xhtml, 1);

        assert_eq!(title, "Chapter One");
        assert_eq!(markdown, "It was a _dark_ and **stormy** night.");
    }

    #[test]
    fn xhtml_to_markdown_falls_back_to_a_generic_title_when_there_is_no_heading() {
        let xhtml = "<html><body><p>Just text.</p></body></html>";

        let (title, markdown) = xhtml_to_markdown(xhtml, 3);

        assert_eq!(title, "Chapter 3");
        assert_eq!(markdown, "Just text.");
    }

    #[test]
    fn xhtml_to_markdown_converts_blockquotes_and_list_items() {
        let xhtml = "<html><body><blockquote>A quote.</blockquote>\
                     <ul><li>one</li><li>two</li></ul></body></html>";

        let (_, markdown) = xhtml_to_markdown(xhtml, 1);

        assert!(markdown.contains("> A quote."));
        assert!(markdown.contains("- one"));
        assert!(markdown.contains("- two"));
    }

    #[test]
    fn xhtml_to_markdown_unescapes_xml_entities() {
        let xhtml = "<html><body><p>Rock &amp; Roll &#8212; forever</p></body></html>";

        let (_, markdown) = xhtml_to_markdown(xhtml, 1);

        assert_eq!(markdown, "Rock & Roll \u{2014} forever");
    }
}
