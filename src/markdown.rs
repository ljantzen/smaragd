use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// A block-level markdown element, in document order.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    pub spans: Vec<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    Heading(u8),
    Paragraph,
    CodeBlock {
        language: Option<String>,
    },
    BlockQuote,
    ListItem {
        ordered: bool,
        index: Option<u64>,
        depth: u8,
    },
    Rule,
    /// A GFM table. `header` and each entry of `rows` hold one `Vec<Span>` per column;
    /// row/cell content is inline-only, per the GFM table spec.
    Table {
        alignments: Vec<ColumnAlignment>,
        header: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnAlignment {
    None,
    Left,
    Center,
    Right,
}

/// A run of inline text sharing the same formatting.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
    pub link: Option<String>,
    /// Set for an Obsidian-style `[[Topic]]`/`[[Topic|Alias]]` span: the target note
    /// name to resolve, separate from `link` (which holds a real URL destination).
    pub wikilink: Option<String>,
    /// Set for an inline `![alt](src "title")` image. `text` holds the alt text, kept
    /// as the fallback if the image can't be loaded/rendered.
    pub image: Option<ImageRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageRef {
    pub src: String,
    pub title: String,
}

struct ListLevel {
    ordered: bool,
    next_index: Option<u64>,
}

/// Accumulates a table's rows while its events are being parsed — the flat
/// `current_kind` + `spans` model the rest of `parse` uses doesn't fit a table's
/// grid-of-cells shape.
struct TableBuilder {
    alignments: Vec<ColumnAlignment>,
    header: Vec<Vec<Span>>,
    rows: Vec<Vec<Vec<Span>>>,
    current_row: Vec<Vec<Span>>,
    current_cell: Vec<Span>,
}

/// Accumulates an image's alt text while its (possibly multi-event, e.g. `*em*`)
/// inline content is being parsed, between `Tag::Image` and `TagEnd::Image`.
struct PendingImage {
    src: String,
    title: String,
    alt: String,
}

fn convert_alignment(alignment: Alignment) -> ColumnAlignment {
    match alignment {
        Alignment::None => ColumnAlignment::None,
        Alignment::Left => ColumnAlignment::Left,
        Alignment::Center => ColumnAlignment::Center,
        Alignment::Right => ColumnAlignment::Right,
    }
}

/// Where inline content (text, images, wikilinks) currently being parsed should be
/// pushed: a table cell's own span list if we're inside one, otherwise the enclosing
/// block's `spans`.
fn current_sink<'a>(
    table: &'a mut Option<TableBuilder>,
    spans: &'a mut Vec<Span>,
) -> &'a mut Vec<Span> {
    match table {
        Some(table) => &mut table.current_cell,
        None => spans,
    }
}

/// Parse `markdown` into a flat sequence of blocks. Deliberately does not model
/// arbitrarily deep nesting (e.g. a blockquote inside a list inside a blockquote) or
/// footnotes — a flat block list with single-level list nesting covers what an author
/// actually writes, and matches what the glow-style preview renders.
pub fn parse(markdown: &str) -> Vec<Block> {
    let (markdown, wikilinks) = extract_wikilinks(markdown);
    let markdown = markdown.as_str();

    let mut blocks = Vec::new();
    let mut current_kind: Option<BlockKind> = None;
    let mut spans: Vec<Span> = Vec::new();

    let mut bold_depth = 0u32;
    let mut italic_depth = 0u32;
    let mut strike_depth = 0u32;
    let mut in_code_block = false;
    let mut link_stack: Vec<String> = Vec::new();
    let mut list_stack: Vec<ListLevel> = Vec::new();
    let mut current_table: Option<TableBuilder> = None;
    let mut pending_image: Option<PendingImage> = None;

    macro_rules! flush {
        () => {
            if let Some(kind) = current_kind.take() {
                if !spans.is_empty() {
                    blocks.push(Block {
                        kind,
                        spans: std::mem::take(&mut spans),
                    });
                } else {
                    spans.clear();
                }
            }
        };
    }

    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush!();
                current_kind = Some(BlockKind::Heading(heading_level_to_u8(level)));
            }
            Event::End(TagEnd::Heading(_)) => flush!(),

            Event::Start(Tag::Paragraph) => {
                if current_kind.is_none() {
                    current_kind = Some(BlockKind::Paragraph);
                } else if !spans.is_empty() {
                    // A later paragraph inside the same enclosing container (list item,
                    // blockquote): keep it in the same block, separated visually.
                    spans.push(Span {
                        text: "\n\n".to_string(),
                        ..Default::default()
                    });
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if matches!(current_kind, Some(BlockKind::Paragraph)) {
                    flush!();
                }
            }

            Event::Start(Tag::BlockQuote(_)) => {
                flush!();
                current_kind = Some(BlockKind::BlockQuote);
            }
            Event::End(TagEnd::BlockQuote(_)) => flush!(),

            Event::Start(Tag::CodeBlock(kind)) => {
                flush!();
                in_code_block = true;
                let language = match kind {
                    CodeBlockKind::Fenced(lang) if !lang.is_empty() => Some(lang.to_string()),
                    _ => None,
                };
                current_kind = Some(BlockKind::CodeBlock { language });
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                flush!();
            }

            Event::Start(Tag::List(start)) => {
                flush!();
                list_stack.push(ListLevel {
                    ordered: start.is_some(),
                    next_index: start,
                });
            }
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }
            Event::Start(Tag::Item) => {
                flush!();
                let depth = list_stack.len().saturating_sub(1) as u8;
                let (ordered, index) = match list_stack.last_mut() {
                    Some(level) => {
                        let index = level.next_index;
                        if let Some(next) = level.next_index.as_mut() {
                            *next += 1;
                        }
                        (level.ordered, index)
                    }
                    None => (false, None),
                };
                current_kind = Some(BlockKind::ListItem {
                    ordered,
                    index,
                    depth,
                });
            }
            Event::End(TagEnd::Item) => flush!(),

            Event::Start(Tag::Emphasis) => italic_depth += 1,
            Event::End(TagEnd::Emphasis) => italic_depth = italic_depth.saturating_sub(1),
            Event::Start(Tag::Strong) => bold_depth += 1,
            Event::End(TagEnd::Strong) => bold_depth = bold_depth.saturating_sub(1),
            Event::Start(Tag::Strikethrough) => strike_depth += 1,
            Event::End(TagEnd::Strikethrough) => strike_depth = strike_depth.saturating_sub(1),
            Event::Start(Tag::Link { dest_url, .. }) => link_stack.push(dest_url.to_string()),
            Event::End(TagEnd::Link) => {
                link_stack.pop();
            }

            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                pending_image = Some(PendingImage {
                    src: dest_url.to_string(),
                    title: title.to_string(),
                    alt: String::new(),
                });
            }
            Event::End(TagEnd::Image) => {
                if let Some(image) = pending_image.take()
                    && (current_kind.is_some() || current_table.is_some())
                {
                    current_sink(&mut current_table, &mut spans).push(Span {
                        text: image.alt,
                        image: Some(ImageRef {
                            src: image.src,
                            title: image.title,
                        }),
                        ..Default::default()
                    });
                }
            }

            Event::Start(Tag::Table(alignments)) => {
                flush!();
                current_table = Some(TableBuilder {
                    alignments: alignments.into_iter().map(convert_alignment).collect(),
                    header: Vec::new(),
                    rows: Vec::new(),
                    current_row: Vec::new(),
                    current_cell: Vec::new(),
                });
            }
            Event::End(TagEnd::Table) => {
                if let Some(table) = current_table.take() {
                    blocks.push(Block {
                        kind: BlockKind::Table {
                            alignments: table.alignments,
                            header: table.header,
                            rows: table.rows,
                        },
                        spans: Vec::new(),
                    });
                }
            }
            // `TableHead` wraps the header's cells directly (pulldown-cmark doesn't
            // nest a `TableRow` inside it the way body rows get one), so it both
            // starts and finalizes `current_row` itself.
            Event::Start(Tag::TableHead) => {
                if let Some(table) = current_table.as_mut() {
                    table.current_row = Vec::new();
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(table) = current_table.as_mut() {
                    table.header = std::mem::take(&mut table.current_row);
                }
            }
            Event::Start(Tag::TableRow) => {
                if let Some(table) = current_table.as_mut() {
                    table.current_row = Vec::new();
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(table) = current_table.as_mut() {
                    let row = std::mem::take(&mut table.current_row);
                    table.rows.push(row);
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(table) = current_table.as_mut() {
                    table.current_cell = Vec::new();
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(table) = current_table.as_mut() {
                    let cell = std::mem::take(&mut table.current_cell);
                    table.current_row.push(cell);
                }
            }

            Event::Text(text) => {
                if let Some(image) = pending_image.as_mut() {
                    image.alt.push_str(&text);
                } else if current_kind.is_some() || current_table.is_some() {
                    let template = Span {
                        bold: bold_depth > 0,
                        italic: italic_depth > 0,
                        code: in_code_block,
                        strikethrough: strike_depth > 0,
                        link: link_stack.last().cloned(),
                        ..Default::default()
                    };
                    let target = current_sink(&mut current_table, &mut spans);
                    if in_code_block {
                        target.push(Span {
                            text: text.to_string(),
                            ..template
                        });
                    } else {
                        expand_placeholders(target, &text, &template, &wikilinks);
                    }
                }
            }
            Event::Code(text) => {
                if let Some(image) = pending_image.as_mut() {
                    image.alt.push_str(&text);
                } else if current_kind.is_some() || current_table.is_some() {
                    current_sink(&mut current_table, &mut spans).push(Span {
                        text: text.to_string(),
                        bold: bold_depth > 0,
                        italic: italic_depth > 0,
                        code: true,
                        strikethrough: strike_depth > 0,
                        link: link_stack.last().cloned(),
                        wikilink: None,
                        image: None,
                    });
                }
            }
            Event::SoftBreak => {
                if let Some(image) = pending_image.as_mut() {
                    image.alt.push(' ');
                } else {
                    let target = current_sink(&mut current_table, &mut spans);
                    if let Some(last) = target.last_mut() {
                        last.text.push(' ');
                    } else {
                        target.push(Span {
                            text: " ".to_string(),
                            ..Default::default()
                        });
                    }
                }
            }
            Event::HardBreak => {
                if let Some(image) = pending_image.as_mut() {
                    image.alt.push('\n');
                } else {
                    current_sink(&mut current_table, &mut spans).push(Span {
                        text: "\n".to_string(),
                        ..Default::default()
                    });
                }
            }
            Event::Rule => {
                flush!();
                blocks.push(Block {
                    kind: BlockKind::Rule,
                    spans: vec![],
                });
            }
            _ => {}
        }
    }
    flush!();

    blocks
}

/// A private-use-area character marking the start/end of a wikilink placeholder.
/// Not valid in ordinary markdown input, so it can't collide with real content, and
/// isn't treated specially by pulldown-cmark's inline tokenizer — the placeholder
/// (this char, a decimal index, this char again) survives inline parsing (emphasis,
/// links, etc.) as ordinary text, unlike the literal `[[...]]` source, which
/// pulldown-cmark's bracket-matching splits into several separate text events.
const WIKILINK_MARK: char = '\u{E000}';

/// A `[[Target]]` / `[[Target|Alias]]` (optionally `!`-prefixed) found by
/// [`extract_wikilinks`], recorded in its side table.
struct WikilinkPlaceholder {
    target: String,
    display: String,
    /// Set for Obsidian-style `![[Target]]` embeds, as opposed to a plain
    /// `[[Target]]` link. Only image-extension targets actually embed as images
    /// (see [`has_image_extension`]) — an embed of anything else (e.g. another note)
    /// falls back to behaving like a plain link, since full note transclusion isn't
    /// implemented.
    is_embed: bool,
}

/// Image filename extensions recognized for `![[Target]]` embeds.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "svg", "tif", "tiff", "ico",
];

fn has_image_extension(name: &str) -> bool {
    name.rsplit('.').next().is_some_and(|ext| {
        IMAGE_EXTENSIONS
            .iter()
            .any(|img| ext.eq_ignore_ascii_case(img))
    })
}

/// Replace `[[Topic]]` / `[[Topic|Alias]]` wikilinks (and `![[Topic]]` embeds) in
/// `markdown` with placeholders pulldown-cmark's inline parser won't fragment,
/// skipping fenced code blocks (whose content must stay literal). Returns the
/// rewritten markdown plus a side table indexed by the number embedded in each
/// placeholder.
fn extract_wikilinks(markdown: &str) -> (String, Vec<WikilinkPlaceholder>) {
    let mut output = String::with_capacity(markdown.len());
    let mut table = Vec::new();
    let mut in_fence = false;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            output.push_str(line);
        } else if in_fence {
            output.push_str(line);
        } else {
            replace_wikilinks_in_line(line, &mut table, &mut output);
        }
    }
    (output, table)
}

fn replace_wikilinks_in_line(
    line: &str,
    table: &mut Vec<WikilinkPlaceholder>,
    output: &mut String,
) {
    let mut rest = line;
    while let Some(start) = rest.find("[[") {
        // A "!" immediately before "[[" marks an embed; it's part of the marker, not
        // literal text, so it isn't copied to `output`.
        let is_embed = start > 0 && rest.as_bytes()[start - 1] == b'!';
        let text_before_end = if is_embed { start - 1 } else { start };
        output.push_str(&rest[..text_before_end]);
        let after_open = &rest[start + 2..];
        match after_open.find("]]") {
            Some(end) => {
                let inner = &after_open[..end];
                let (target, display) = match inner.split_once('|') {
                    Some((target, alias)) => (target.trim().to_string(), alias.trim().to_string()),
                    None => (inner.trim().to_string(), inner.trim().to_string()),
                };
                table.push(WikilinkPlaceholder {
                    target,
                    display,
                    is_embed,
                });
                output.push(WIKILINK_MARK);
                output.push_str(&(table.len() - 1).to_string());
                output.push(WIKILINK_MARK);
                rest = &after_open[end + 2..];
            }
            None => {
                // Unterminated "[[": no wikilink here, keep the rest as plain text.
                output.push_str(&rest[text_before_end..]);
                return;
            }
        }
    }
    output.push_str(rest);
}

/// Split `text` (already through pulldown-cmark) on wikilink placeholders inserted by
/// [`extract_wikilinks`], pushing plain-text spans, wikilink spans, and image spans
/// (for `![[image.ext]]` embeds) in order, all inheriting `template`'s formatting.
fn expand_placeholders(
    spans: &mut Vec<Span>,
    text: &str,
    template: &Span,
    table: &[WikilinkPlaceholder],
) {
    let mut rest = text;
    while let Some(start) = rest.find(WIKILINK_MARK) {
        if start > 0 {
            spans.push(Span {
                text: rest[..start].to_string(),
                ..template.clone()
            });
        }
        let after = &rest[start + WIKILINK_MARK.len_utf8()..];
        let end = after
            .find(WIKILINK_MARK)
            .expect("well-formed wikilink placeholder");
        let index: usize = after[..end]
            .parse()
            .expect("well-formed wikilink placeholder");
        let placeholder = &table[index];
        if placeholder.is_embed && has_image_extension(&placeholder.target) {
            spans.push(Span {
                text: placeholder.display.clone(),
                image: Some(ImageRef {
                    src: placeholder.target.clone(),
                    title: String::new(),
                }),
                ..template.clone()
            });
        } else {
            spans.push(Span {
                text: placeholder.display.clone(),
                wikilink: Some(placeholder.target.clone()),
                ..template.clone()
            });
        }
        rest = &after[end + WIKILINK_MARK.len_utf8()..];
    }
    if !rest.is_empty() {
        spans.push(Span {
            text: rest.to_string(),
            ..template.clone()
        });
    }
}

/// Rewrite every `[[Topic]]` / `[[Topic|Alias]]` wikilink in `markdown` whose target
/// matches `old_target` case-insensitively to point at `new_target` instead, keeping
/// any explicit alias unchanged (a link with no alias picks up the new name as its
/// display text too, matching what the old, un-aliased text implied). Used to keep
/// links pointing at a document that's just been renamed. Returns `None` if nothing
/// changed, skipping fenced code blocks the same way [`extract_wikilinks`] does.
pub fn rename_wikilink_target(
    markdown: &str,
    old_target: &str,
    new_target: &str,
) -> Option<String> {
    let mut output = String::with_capacity(markdown.len());
    let mut in_fence = false;
    let mut changed = false;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            output.push_str(line);
        } else if in_fence {
            output.push_str(line);
        } else {
            changed |= rename_wikilink_target_in_line(line, old_target, new_target, &mut output);
        }
    }
    changed.then_some(output)
}

fn rename_wikilink_target_in_line(
    line: &str,
    old_target: &str,
    new_target: &str,
    output: &mut String,
) -> bool {
    let mut rest = line;
    let mut changed = false;
    while let Some(start) = rest.find("[[") {
        output.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        match after_open.find("]]") {
            Some(end) => {
                let inner = &after_open[..end];
                let (target, alias) = match inner.split_once('|') {
                    Some((target, alias)) => (target.trim(), Some(alias.trim())),
                    None => (inner.trim(), None),
                };
                if target.eq_ignore_ascii_case(old_target) {
                    output.push_str("[[");
                    output.push_str(new_target);
                    if let Some(alias) = alias {
                        output.push('|');
                        output.push_str(alias);
                    }
                    output.push_str("]]");
                    changed = true;
                } else {
                    output.push_str("[[");
                    output.push_str(inner);
                    output.push_str("]]");
                }
                rest = &after_open[end + 2..];
            }
            None => {
                output.push_str(&rest[start..]);
                return changed;
            }
        }
    }
    output.push_str(rest);
    changed
}

/// The byte range (covering the full `[[...]]`) and target of every wikilink in
/// `markdown`, skipping fenced code blocks.
fn wikilink_spans(markdown: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut spans = Vec::new();
    let mut in_fence = false;
    let mut line_start = 0usize;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence {
            let mut cursor = 0usize;
            while let Some(start) = line[cursor..].find("[[") {
                let open_end = cursor + start + 2;
                match line[open_end..].find("]]") {
                    Some(end) => {
                        let inner = &line[open_end..open_end + end];
                        let target = inner.split_once('|').map_or(inner, |(t, _)| t).trim();
                        let abs_start = line_start + cursor + start;
                        let abs_end = line_start + open_end + end + 2;
                        spans.push((abs_start..abs_end, target.to_string()));
                        cursor = open_end + end + 2;
                    }
                    None => break,
                }
            }
        }
        line_start += line.len();
    }
    spans
}

/// If `cursor` (a byte offset into `markdown`) sits inside — or right after — a closed
/// `[[Target]]` / `[[Target|Alias]]` wikilink, return its target. Used to activate the
/// link the cursor is on via a keyboard shortcut (Ctrl+Enter), the editor's equivalent
/// of clicking a link in the preview.
pub fn wikilink_target_at(markdown: &str, cursor: usize) -> Option<String> {
    wikilink_spans(markdown)
        .into_iter()
        .find(|(range, _)| range.contains(&cursor) || range.end == cursor)
        .map(|(_, target)| target)
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Span {
        Span {
            text: text.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn parses_heading_levels() {
        let blocks = parse("# Title\n\n## Subtitle\n");
        assert_eq!(
            blocks,
            vec![
                Block {
                    kind: BlockKind::Heading(1),
                    spans: vec![plain("Title")],
                },
                Block {
                    kind: BlockKind::Heading(2),
                    spans: vec![plain("Subtitle")],
                },
            ]
        );
    }

    #[test]
    fn parses_paragraph_with_bold_and_italic() {
        let blocks = parse("Some **bold** and *italic* text.");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Paragraph);
        assert_eq!(
            blocks[0].spans,
            vec![
                plain("Some "),
                Span {
                    text: "bold".to_string(),
                    bold: true,
                    ..Default::default()
                },
                plain(" and "),
                Span {
                    text: "italic".to_string(),
                    italic: true,
                    ..Default::default()
                },
                plain(" text."),
            ]
        );
    }

    #[test]
    fn parses_inline_code_and_link() {
        let blocks = parse("Run `cargo test` or visit [docs](https://example.com).");
        let spans = &blocks[0].spans;
        assert!(spans.iter().any(|s| s.code && s.text == "cargo test"));
        assert!(
            spans
                .iter()
                .any(|s| s.link.as_deref() == Some("https://example.com") && s.text == "docs")
        );
    }

    #[test]
    fn parses_fenced_code_block_with_language() {
        let blocks = parse("```rust\nfn main() {}\n```\n");
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].kind,
            BlockKind::CodeBlock {
                language: Some("rust".to_string())
            }
        );
        assert_eq!(blocks[0].spans[0].text, "fn main() {}\n");
    }

    #[test]
    fn parses_blockquote_as_single_block() {
        let blocks = parse("> Stay awhile\n> and listen.\n");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::BlockQuote);
        let joined: String = blocks[0].spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "Stay awhile and listen.");
    }

    #[test]
    fn parses_unordered_list_items_with_depth() {
        let blocks = parse("- first\n- second\n  - nested\n");
        let items: Vec<&BlockKind> = blocks.iter().map(|b| &b.kind).collect();
        assert_eq!(
            items,
            vec![
                &BlockKind::ListItem {
                    ordered: false,
                    index: None,
                    depth: 0
                },
                &BlockKind::ListItem {
                    ordered: false,
                    index: None,
                    depth: 0
                },
                &BlockKind::ListItem {
                    ordered: false,
                    index: None,
                    depth: 1
                },
            ]
        );
    }

    #[test]
    fn parses_ordered_list_with_increasing_index() {
        let blocks = parse("3. third\n4. fourth\n");
        assert_eq!(
            blocks[0].kind,
            BlockKind::ListItem {
                ordered: true,
                index: Some(3),
                depth: 0
            }
        );
        assert_eq!(
            blocks[1].kind,
            BlockKind::ListItem {
                ordered: true,
                index: Some(4),
                depth: 0
            }
        );
    }

    #[test]
    fn parses_horizontal_rule() {
        let blocks = parse("above\n\n---\n\nbelow\n");
        assert_eq!(
            blocks.iter().map(|b| &b.kind).collect::<Vec<_>>(),
            vec![
                &BlockKind::Paragraph,
                &BlockKind::Rule,
                &BlockKind::Paragraph
            ]
        );
    }

    #[test]
    fn empty_input_produces_no_blocks() {
        assert_eq!(parse(""), vec![]);
    }

    #[test]
    fn strikethrough_is_flagged() {
        let blocks = parse("~~gone~~");
        assert!(blocks[0].spans[0].strikethrough);
    }

    #[test]
    fn parses_wikilink_with_alias() {
        let blocks = parse("See [[Topic|Alias]] for details.");
        assert_eq!(
            blocks[0].spans,
            vec![
                plain("See "),
                Span {
                    text: "Alias".to_string(),
                    wikilink: Some("Topic".to_string()),
                    ..Default::default()
                },
                plain(" for details."),
            ]
        );
    }

    #[test]
    fn parses_wikilink_without_alias() {
        let blocks = parse("[[Topic]]");
        assert_eq!(
            blocks[0].spans,
            vec![Span {
                text: "Topic".to_string(),
                wikilink: Some("Topic".to_string()),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn wikilink_inside_fenced_code_block_is_left_as_plain_text() {
        let blocks = parse("```\n[[Topic]]\n```\n");
        assert_eq!(blocks[0].spans[0].text, "[[Topic]]\n");
        assert!(blocks[0].spans[0].wikilink.is_none());
    }

    #[test]
    fn unterminated_wikilink_is_left_as_plain_text() {
        let blocks = parse("open [[Topic without close");
        let joined: String = blocks[0].spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "open [[Topic without close");
        assert!(blocks[0].spans.iter().all(|s| s.wikilink.is_none()));
    }

    #[test]
    fn multiple_wikilinks_in_one_paragraph_resolve_to_distinct_targets() {
        let blocks = parse("[[A]] and [[B|b]] and [[C]]");
        let links: Vec<(Option<String>, &str)> = blocks[0]
            .spans
            .iter()
            .filter(|s| s.wikilink.is_some())
            .map(|s| (s.wikilink.clone(), s.text.as_str()))
            .collect();
        assert_eq!(
            links,
            vec![
                (Some("A".to_string()), "A"),
                (Some("B".to_string()), "b"),
                (Some("C".to_string()), "C"),
            ]
        );
    }

    #[test]
    fn wikilink_inherits_surrounding_bold_formatting() {
        let blocks = parse("**[[Topic]]**");
        assert_eq!(blocks[0].spans.len(), 1);
        assert!(blocks[0].spans[0].bold);
        assert_eq!(blocks[0].spans[0].wikilink.as_deref(), Some("Topic"));
    }

    #[test]
    fn parses_wikilink_in_heading() {
        let blocks = parse("# See [[Topic]]\n");
        assert_eq!(blocks[0].kind, BlockKind::Heading(1));
        assert!(
            blocks[0]
                .spans
                .iter()
                .any(|s| s.wikilink.as_deref() == Some("Topic"))
        );
    }

    #[test]
    fn parses_wikilink_in_list_item() {
        let blocks = parse("- see [[Topic]]\n");
        assert_eq!(
            blocks[0].kind,
            BlockKind::ListItem {
                ordered: false,
                index: None,
                depth: 0
            }
        );
        assert!(
            blocks[0]
                .spans
                .iter()
                .any(|s| s.wikilink.as_deref() == Some("Topic"))
        );
    }

    #[test]
    fn wikilink_inside_tilde_fenced_code_block_is_left_as_plain_text() {
        let blocks = parse("~~~\n[[Topic]]\n~~~\n");
        assert_eq!(blocks[0].spans[0].text, "[[Topic]]\n");
        assert!(blocks[0].spans[0].wikilink.is_none());
    }

    #[test]
    fn rename_wikilink_target_updates_a_bare_link() {
        let updated =
            rename_wikilink_target("See [[Old Name]] for more.", "Old Name", "New Name").unwrap();
        assert_eq!(updated, "See [[New Name]] for more.");
    }

    #[test]
    fn rename_wikilink_target_keeps_an_explicit_alias() {
        let updated = rename_wikilink_target(
            "See [[Old Name|the other note]] for more.",
            "Old Name",
            "New Name",
        )
        .unwrap();
        assert_eq!(updated, "See [[New Name|the other note]] for more.");
    }

    #[test]
    fn rename_wikilink_target_is_case_insensitive() {
        let updated = rename_wikilink_target("[[old name]]", "Old Name", "New Name").unwrap();
        assert_eq!(updated, "[[New Name]]");
    }

    #[test]
    fn rename_wikilink_target_updates_every_matching_occurrence() {
        let updated = rename_wikilink_target(
            "[[Old Name]] and again [[Old Name|alias]]",
            "Old Name",
            "New Name",
        )
        .unwrap();
        assert_eq!(updated, "[[New Name]] and again [[New Name|alias]]");
    }

    #[test]
    fn rename_wikilink_target_leaves_other_links_untouched() {
        let updated = rename_wikilink_target("[[Unrelated]]", "Old Name", "New Name");
        assert_eq!(updated, None);
    }

    #[test]
    fn rename_wikilink_target_skips_fenced_code_blocks() {
        let updated = rename_wikilink_target("```\n[[Old Name]]\n```\n", "Old Name", "New Name");
        assert_eq!(updated, None);
    }

    #[test]
    fn wikilink_target_at_finds_target_when_cursor_is_inside_the_brackets() {
        let text = "See [[Topic]] please";
        let inside = text.find("Top").unwrap();
        assert_eq!(wikilink_target_at(text, inside), Some("Topic".to_string()));
    }

    #[test]
    fn wikilink_target_at_finds_target_when_cursor_is_right_after_the_closing_brackets() {
        let text = "[[Topic]]";
        assert_eq!(
            wikilink_target_at(text, text.len()),
            Some("Topic".to_string())
        );
    }

    #[test]
    fn wikilink_target_at_strips_alias() {
        let text = "[[Topic|Alias]]";
        let inside = text.find("Alias").unwrap();
        assert_eq!(wikilink_target_at(text, inside), Some("Topic".to_string()));
    }

    #[test]
    fn wikilink_target_at_is_none_outside_any_wikilink() {
        let text = "See [[Topic]] please";
        let outside = text.find("please").unwrap();
        assert_eq!(wikilink_target_at(text, outside), None);
    }

    #[test]
    fn wikilink_target_at_ignores_fenced_code_blocks() {
        let text = "```\n[[Topic]]\n```\n";
        let inside = text.find("Topic").unwrap();
        assert_eq!(wikilink_target_at(text, inside), None);
    }

    #[test]
    fn parses_inline_image_with_alt_and_title() {
        let blocks = parse(r#"See ![a cat](cat.png "My Cat") here."#);
        assert_eq!(blocks[0].kind, BlockKind::Paragraph);
        assert_eq!(
            blocks[0].spans,
            vec![
                plain("See "),
                Span {
                    text: "a cat".to_string(),
                    image: Some(ImageRef {
                        src: "cat.png".to_string(),
                        title: "My Cat".to_string(),
                    }),
                    ..Default::default()
                },
                plain(" here."),
            ]
        );
    }

    #[test]
    fn parses_image_without_title() {
        let blocks = parse("![alt](pic.png)");
        assert_eq!(
            blocks[0].spans,
            vec![Span {
                text: "alt".to_string(),
                image: Some(ImageRef {
                    src: "pic.png".to_string(),
                    title: String::new(),
                }),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn parses_simple_table_with_alignment_and_header() {
        let blocks = parse("| A | B |\n|:---|---:|\n| 1 | 2 |\n| 3 | 4 |\n");
        assert_eq!(blocks.len(), 1);
        match &blocks[0].kind {
            BlockKind::Table {
                alignments,
                header,
                rows,
            } => {
                assert_eq!(
                    alignments,
                    &vec![ColumnAlignment::Left, ColumnAlignment::Right]
                );
                assert_eq!(header, &vec![vec![plain("A")], vec![plain("B")]]);
                assert_eq!(
                    rows,
                    &vec![
                        vec![vec![plain("1")], vec![plain("2")]],
                        vec![vec![plain("3")], vec![plain("4")]],
                    ]
                );
            }
            other => panic!("expected a table, got {other:?}"),
        }
    }

    #[test]
    fn table_cells_support_inline_formatting_and_wikilinks() {
        let blocks = parse("| A |\n|---|\n| **bold** and [[Topic]] |\n");
        match &blocks[0].kind {
            BlockKind::Table { rows, .. } => {
                let cell = &rows[0][0];
                assert!(cell.iter().any(|s| s.bold && s.text == "bold"));
                assert!(cell.iter().any(|s| s.wikilink.as_deref() == Some("Topic")));
            }
            other => panic!("expected a table, got {other:?}"),
        }
    }

    #[test]
    fn table_without_alignment_markers_defaults_to_none() {
        let blocks = parse("| A |\n|---|\n| 1 |\n");
        match &blocks[0].kind {
            BlockKind::Table { alignments, .. } => {
                assert_eq!(alignments, &vec![ColumnAlignment::None]);
            }
            other => panic!("expected a table, got {other:?}"),
        }
    }

    #[test]
    fn parses_image_embed_with_no_alias() {
        let blocks = parse("![[cat.png]]");
        assert_eq!(
            blocks[0].spans,
            vec![Span {
                text: "cat.png".to_string(),
                image: Some(ImageRef {
                    src: "cat.png".to_string(),
                    title: String::new(),
                }),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn parses_image_embed_with_alias_as_alt_text() {
        let blocks = parse("![[cat.png|A cute cat]]");
        assert_eq!(
            blocks[0].spans,
            vec![Span {
                text: "A cute cat".to_string(),
                image: Some(ImageRef {
                    src: "cat.png".to_string(),
                    title: String::new(),
                }),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn image_embed_extension_check_is_case_insensitive() {
        let blocks = parse("![[cat.PNG]]");
        assert!(blocks[0].spans[0].image.is_some());
    }

    #[test]
    fn embed_of_a_non_image_target_falls_back_to_a_plain_wikilink() {
        let blocks = parse("![[Some Note]]");
        assert_eq!(
            blocks[0].spans,
            vec![Span {
                text: "Some Note".to_string(),
                wikilink: Some("Some Note".to_string()),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn embed_marker_does_not_leak_a_literal_exclamation_mark() {
        let blocks = parse("See ![[cat.png]] here.");
        let joined: String = blocks[0].spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "See cat.png here.");
        assert!(!joined.contains('!'));
    }

    #[test]
    fn unterminated_embed_marker_is_left_as_plain_text_including_the_bang() {
        let blocks = parse("open ![[cat.png without close");
        let joined: String = blocks[0].spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "open ![[cat.png without close");
    }

    #[test]
    fn table_row_shorter_than_header_is_padded_with_empty_cells() {
        // pulldown-cmark itself normalizes ragged rows to the header's column count
        // before we see the events; this just confirms our TableBuilder passes that
        // shape through unchanged instead of e.g. dropping/collapsing empty cells.
        let blocks = parse("| A | B | C |\n|---|---|---|\n| 1 |\n");
        match &blocks[0].kind {
            BlockKind::Table { rows, .. } => {
                assert_eq!(rows, &vec![vec![vec![plain("1")], vec![], vec![]]]);
            }
            other => panic!("expected a table, got {other:?}"),
        }
    }

    #[test]
    fn table_row_longer_than_header_is_truncated_to_header_width() {
        let blocks = parse("| A | B | C |\n|---|---|---|\n| 1 | 2 | 3 | 4 |\n");
        match &blocks[0].kind {
            BlockKind::Table { rows, .. } => {
                assert_eq!(
                    rows,
                    &vec![vec![vec![plain("1")], vec![plain("2")], vec![plain("3")]]]
                );
            }
            other => panic!("expected a table, got {other:?}"),
        }
    }

    #[test]
    fn table_cell_can_contain_an_image() {
        let blocks = parse("| A |\n|---|\n| ![alt](cat.png) |\n");
        match &blocks[0].kind {
            BlockKind::Table { rows, .. } => {
                assert_eq!(
                    rows,
                    &vec![vec![vec![Span {
                        text: "alt".to_string(),
                        image: Some(ImageRef {
                            src: "cat.png".to_string(),
                            title: String::new(),
                        }),
                        ..Default::default()
                    }]]]
                );
            }
            other => panic!("expected a table, got {other:?}"),
        }
    }

    #[test]
    fn parses_image_with_empty_alt_text() {
        let blocks = parse("![](pic.png)");
        assert_eq!(
            blocks[0].spans,
            vec![Span {
                text: String::new(),
                image: Some(ImageRef {
                    src: "pic.png".to_string(),
                    title: String::new(),
                }),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn embed_with_alias_for_non_image_target_uses_alias_as_wikilink_display() {
        let blocks = parse("![[Some Note|Custom Text]]");
        assert_eq!(
            blocks[0].spans,
            vec![Span {
                text: "Custom Text".to_string(),
                wikilink: Some("Some Note".to_string()),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn has_image_extension_recognizes_all_supported_extensions() {
        for ext in [
            "png", "jpg", "jpeg", "gif", "bmp", "webp", "svg", "tif", "tiff", "ico",
        ] {
            assert!(
                has_image_extension(&format!("cat.{ext}")),
                "expected {ext} to be recognized as an image extension"
            );
        }
    }

    #[test]
    fn has_image_extension_rejects_non_image_extensions() {
        assert!(!has_image_extension("video.mp4"));
        assert!(!has_image_extension("Some Note"));
        assert!(!has_image_extension("no_extension"));
    }
}
