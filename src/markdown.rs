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
    /// Set for a `[[Topic]]`/`[[Topic|Alias]]` span: the target note
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
/// actually writes, and matches what `ui::markdown_preview` renders.
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
    /// Set for `![[Target]]` embeds, as opposed to a plain
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
/// skipping fenced code blocks and single-backtick inline code spans (whose
/// content must stay literal). Returns the rewritten markdown plus a side table
/// indexed by the number embedded in each placeholder.
fn extract_wikilinks(markdown: &str) -> (String, Vec<WikilinkPlaceholder>) {
    // WIKILINK_MARK is meant to appear only in placeholders *we* insert below, each
    // immediately followed by a decimal table index and a closing WIKILINK_MARK.
    // expand_placeholders trusts that unconditionally when parsing them back out —
    // a literal WIKILINK_MARK already present in the input (a real, if rare,
    // possibility: it's a valid Unicode scalar value some icon-font/emoji-picker
    // workflows assign glyphs to) would leave it hunting for a partner that was
    // never there and panicking. Strip any pre-existing occurrence up front so the
    // only WIKILINK_MARKs `output` ever contains are well-formed ones we made.
    let markdown = if markdown.contains(WIKILINK_MARK) {
        std::borrow::Cow::Owned(markdown.replace(WIKILINK_MARK, "\u{FFFD}"))
    } else {
        std::borrow::Cow::Borrowed(markdown)
    };

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
    let code_ranges = inline_code_ranges(line);
    let mut rest = line;
    loop {
        let Some(start) = rest.find("[[") else {
            output.push_str(rest);
            return;
        };
        // `` `[[not a link]]` `` inside inline code: leave it as literal text
        // (including the brackets themselves) rather than a wikilink, same as
        // `wikilink_spans` does for the backlinks/cursor-lookup side.
        if in_inline_code(&code_ranges, line.len() - rest.len() + start) {
            output.push_str(&rest[..start + 2]);
            rest = &rest[start + 2..];
            continue;
        }
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

/// Byte ranges (covering both backticks) of every single-backtick inline code
/// span — `` `code` `` — on one line, used by `wikilink_spans`/
/// `inline_tag_spans`/`replace_wikilinks_in_line` to leave `` `[[not a
/// link]]` ``/`` `#not-a-tag` `` as literal text rather than a real wikilink/
/// tag. Deliberately only single backticks, not the general N-backtick-fence
/// CommonMark inline-code rule (which lets a span contain a literal backtick
/// via `` ``like `this` `` ``) — a rare enough case not to be worth the extra
/// complexity here. An unterminated trailing backtick (no closing partner on
/// the line) opens no span and is simply left out of the result, same as it
/// wouldn't open real code.
fn inline_code_ranges(line: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = line[cursor..].find('`') {
        let open = cursor + offset;
        match line[open + 1..].find('`') {
            Some(close_offset) => {
                let close = open + 1 + close_offset;
                ranges.push(open..close + 1);
                cursor = close + 1;
            }
            None => break,
        }
    }
    ranges
}

fn in_inline_code(ranges: &[std::ops::Range<usize>], pos: usize) -> bool {
    ranges.iter().any(|range| range.contains(&pos))
}

/// The byte range (covering the full `[[...]]`) and target of every wikilink in
/// `markdown`, skipping fenced code blocks and single-backtick inline code spans.
/// `pub(crate)` rather than private since `project::backlinks` (a different module)
/// needs the same byte ranges this crate's own `wikilink_target_at` already relies on.
pub(crate) fn wikilink_spans(markdown: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut spans = Vec::new();
    let mut in_fence = false;
    let mut line_start = 0usize;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence {
            let code_ranges = inline_code_ranges(line);
            let mut cursor = 0usize;
            while let Some(start) = line[cursor..].find("[[") {
                let open_end = cursor + start + 2;
                match line[open_end..].find("]]") {
                    Some(end) => {
                        if in_inline_code(&code_ranges, cursor + start) {
                            cursor = open_end + end + 2;
                            continue;
                        }
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

/// Whether `target` (a `[[wikilink]]`'s target, i.e. without its `|Alias`)
/// matches one of `note_titles` — the project's document filenames without
/// extension — case-insensitively (full Unicode case folding, matching
/// `project::BinderTree::find_document_by_stem`, the lookup that actually
/// resolves a followed link to a file). Duplicated here rather than depending
/// on `project` so both the Editor and Preview panels can style a wikilink
/// that doesn't resolve to any document, without a circular dependency between the two modules.
pub fn wikilink_resolves(target: &str, note_titles: &[String]) -> bool {
    let target = target.to_lowercase();
    note_titles
        .iter()
        .any(|title| title.to_lowercase() == target)
}

/// Total characters of context a backlink snippet shows, split ~evenly before/after
/// the link.
const MAX_SNIPPET_CHARS: usize = 120;

/// Build a short, single-line, human-readable snippet of the text surrounding
/// `link_range` (as returned by [`wikilink_spans`]) within `markdown`, for display in
/// the backlinks panel: up to [`MAX_SNIPPET_CHARS`] characters of context total,
/// split ~evenly before/after the link — a side that runs out of text early
/// (because the link sits near the start or end of the document) gives its unused
/// share to the other side, so a link near an edge still gets a full-length
/// snippet. Embedded newlines/whitespace runs collapse to a single space (a link can
/// sit on a soft-wrapped line), and an ellipsis is added on whichever end doesn't
/// reach the document's actual start/end. Leaves the `[[...]]` syntax itself as-is —
/// this is a plain-text excerpt, not rendered markdown.
///
/// Works entirely in char (not byte) offsets internally, only converting back to
/// byte offsets to slice `markdown` — `link_range`'s own byte offsets are safe to
/// index directly (they land on the ASCII `[[`/`]]` delimiters), but a *computed*
/// cut point further away must never land in the middle of a multi-byte UTF-8
/// scalar, which naive byte arithmetic wouldn't protect against.
pub(crate) fn wikilink_context_snippet(
    markdown: &str,
    link_range: &std::ops::Range<usize>,
) -> String {
    let total_chars = markdown.chars().count();
    let link_start_char = markdown[..link_range.start].chars().count();
    let link_end_char = markdown[..link_range.end].chars().count();

    let before_budget = MAX_SNIPPET_CHARS / 2;
    let after_budget = MAX_SNIPPET_CHARS - before_budget;

    let available_before = link_start_char;
    let available_after = total_chars - link_end_char;

    let before = before_budget.min(available_before);
    let after = after_budget.min(available_after);
    let before_shortfall = before_budget - before;
    let after_shortfall = after_budget - after;

    let after = (after + before_shortfall).min(available_after);
    let before = (before + after_shortfall).min(available_before);

    let start_char = link_start_char - before;
    let end_char = link_end_char + after;

    let start_byte = char_index_to_byte(markdown, total_chars, start_char);
    let end_byte = char_index_to_byte(markdown, total_chars, end_char);

    let mut snippet = markdown[start_byte..end_byte]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if start_char > 0 {
        snippet = format!("…{snippet}");
    }
    if end_char < total_chars {
        snippet = format!("{snippet}…");
    }
    snippet
}

/// The byte offset of the `char_index`-th character in `s` (`total_chars` is
/// `s.chars().count()`, passed in since callers already have it and would otherwise
/// recompute it). `char_index == total_chars` (one past the last character, as valid
/// an end-of-range as `s.len()`) maps to `s.len()` rather than `None`.
fn char_index_to_byte(s: &str, total_chars: usize, char_index: usize) -> usize {
    if char_index >= total_chars {
        return s.len();
    }
    s.char_indices()
        .nth(char_index)
        .map_or(s.len(), |(byte, _)| byte)
}

/// Rewrite every `#tag` marker in `markdown` matching `old_tag` case-insensitively
/// (an exact match against the whole tag, not a prefix — renaming `projects` never
/// touches `projects/smaragd`) to `#new_tag` instead. Reuses [`inline_tag_spans`] to
/// find the occurrences, so it inherits the same fenced-code-block/inline-code-span
/// skipping and heading/mid-word exclusions for free. Returns `None` if nothing
/// changed. The `#tag` counterpart to [`rename_wikilink_target`], used to rename a
/// tag across the whole vault (see `Project::rename_tag`).
pub fn rename_tag(markdown: &str, old_tag: &str, new_tag: &str) -> Option<String> {
    let mut output = String::with_capacity(markdown.len());
    let mut cursor = 0usize;
    let mut changed = false;
    for (range, tag) in inline_tag_spans(markdown) {
        if !tag.eq_ignore_ascii_case(old_tag) {
            continue;
        }
        output.push_str(&markdown[cursor..range.start]);
        output.push('#');
        output.push_str(new_tag);
        cursor = range.end;
        changed = true;
    }
    output.push_str(&markdown[cursor..]);
    changed.then_some(output)
}

/// A `#tag` marker's allowed characters after the leading `#`: ASCII letters,
/// digits, `_`, `-`, and `/` (the last for nested tags like `#projects/smaragd`).
/// `pub(crate)` rather than private since `autocomplete::active_tag_query`
/// needs the exact same character rule to know when an in-progress `#query`
/// stops being a valid tag.
pub(crate) fn is_tag_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/')
}

/// Every `#tag` marker in `markdown`'s raw source (byte range including the
/// `#`, plus the tag text itself), skipping fenced code blocks and
/// single-backtick inline code spans the same way `extract_wikilinks`/
/// `wikilink_spans` do. A `#` only starts a tag when the character
/// immediately before it (if any) isn't itself alphanumeric — so `foo#bar`
/// mid-word doesn't match, but `(#tag)`, a leading `#tag`, and `-#tag` do —
/// and the run of `is_tag_char` characters after it must contain at least one
/// ASCII letter or it's left as plain text: this rejects `#42`/`#1`-style
/// numeric references (issue numbers, footnote markers), common in prose and
/// never meant as tags, and also means an ATX heading's `#`/`##`/etc.
/// (always followed by a space or end of line) never matches, with no
/// separate heading-detection logic needed.
pub(crate) fn inline_tag_spans(markdown: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut spans = Vec::new();
    let mut in_fence = false;
    let mut line_start = 0usize;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence {
            let code_ranges = inline_code_ranges(line);
            let mut cursor = 0usize;
            while let Some(offset) = line[cursor..].find('#') {
                let hash_pos = cursor + offset;
                let preceded_by_word_char = line[..hash_pos]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric());
                let tag_start = hash_pos + 1;
                let tag_end = line[tag_start..]
                    .find(|c: char| !is_tag_char(c))
                    .map_or(line.len(), |end| tag_start + end);
                let tag = &line[tag_start..tag_end];
                if !preceded_by_word_char
                    && tag.chars().any(|c| c.is_ascii_alphabetic())
                    && !in_inline_code(&code_ranges, hash_pos)
                {
                    spans.push((line_start + hash_pos..line_start + tag_end, tag.to_string()));
                }
                cursor = tag_end.max(hash_pos + 1);
            }
        }
        line_start += line.len();
    }
    spans
}

/// Every distinct `#tag` in `markdown`, case-insensitively deduplicated
/// (first-seen casing kept) — what a document's inline tags actually are, as
/// opposed to `inline_tag_spans`' raw per-occurrence list with byte ranges.
pub(crate) fn inline_tags(markdown: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for (_, tag) in inline_tag_spans(markdown) {
        if !tags.iter().any(|seen| seen.eq_ignore_ascii_case(&tag)) {
            tags.push(tag);
        }
    }
    tags
}

/// Rewrite straight typewriter punctuation (`"`, `'`, `--`, `...`) into curly
/// quotes, an em dash, and an ellipsis, in place, across every block's spans —
/// an optional finishing pass over the same IR both the preview and export
/// renderers consume, so either can opt in without re-parsing. Source `.md`
/// files are never touched; this only rewrites the parsed `Block`/`Span` tree
/// handed to a renderer for one frame or one export run.
///
/// Skips `Span::code` runs (inline code and code-block content) and images —
/// literal punctuation in those must survive unchanged — but keeps tracking
/// quote-open/close context across them, so text resuming after an inline
/// `` `code` `` span still sees the right preceding character.
///
/// `BlockKind::Table` stores its cells outside `Block::spans`, so table cells
/// are curled independently, each starting its own quote context (a table
/// cell is never a continuation of prose from a neighboring cell).
pub fn apply_typewriter_quotes(blocks: &mut [Block]) {
    for block in blocks.iter_mut() {
        match &mut block.kind {
            BlockKind::Table { header, rows, .. } => {
                for cell in header.iter_mut() {
                    curl_spans(cell);
                }
                for row in rows.iter_mut() {
                    for cell in row.iter_mut() {
                        curl_spans(cell);
                    }
                }
            }
            _ => curl_spans(&mut block.spans),
        }
    }
}

/// True when a `"`/`'` immediately following `prev` should open a quote rather
/// than close one: at the very start of the tracked text, after whitespace, or
/// after an opening bracket/dash/another opening quote.
fn opens_quote(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(c) => c.is_whitespace() || matches!(c, '(' | '[' | '{' | '“' | '‘' | '—' | '–'),
    }
}

/// True for an apostrophe used mid-word (`don't`, `dogs'`) — always a closing
/// curl, never an opening quote, regardless of `opens_quote`.
fn is_word_internal_apostrophe(prev: Option<char>) -> bool {
    prev.is_some_and(|c| c.is_alphanumeric())
}

/// True for an elided-digits apostrophe (`'60s`, `'99`) at the start of a
/// quote-opening context — also a closing curl, since it stands in for
/// omitted digits rather than opening a quotation.
fn is_elision_apostrophe(prev: Option<char>, next: Option<char>) -> bool {
    opens_quote(prev) && next.is_some_and(|c| c.is_ascii_digit())
}

/// Curl every non-code, non-image span in `spans` in place, threading quote
/// context (the previous character seen) across span boundaries — formatting
/// changes (bold/italic/link) split one sentence into several spans, but the
/// quote logic needs to see the sentence as continuous.
fn curl_spans(spans: &mut [Span]) {
    let mut prev_char: Option<char> = None;
    for span in spans.iter_mut() {
        if span.code || span.image.is_some() {
            prev_char = span.text.chars().next_back().or(prev_char);
            continue;
        }
        let replaced = span.text.replace("...", "…").replace("--", "—");
        let chars: Vec<char> = replaced.chars().collect();
        let mut out = String::with_capacity(replaced.len());
        for (i, &c) in chars.iter().enumerate() {
            let next = chars.get(i + 1).copied();
            let curled = match c {
                '"' => {
                    if opens_quote(prev_char) {
                        '“'
                    } else {
                        '”'
                    }
                }
                '\'' => {
                    if is_word_internal_apostrophe(prev_char)
                        || is_elision_apostrophe(prev_char, next)
                    {
                        '’'
                    } else if opens_quote(prev_char) {
                        '‘'
                    } else {
                        '’'
                    }
                }
                other => other,
            };
            out.push(curled);
            prev_char = Some(c);
        }
        span.text = out;
    }
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
    fn wikilink_inside_inline_code_is_left_as_plain_text() {
        let blocks = parse("See `[[not a link]]` here.");
        let code_span = blocks[0]
            .spans
            .iter()
            .find(|s| s.code)
            .expect("inline code span");
        assert_eq!(code_span.text, "[[not a link]]");
        assert!(blocks[0].spans.iter().all(|s| s.wikilink.is_none()));
    }

    #[test]
    fn wikilink_spans_skips_a_wikilink_inside_inline_code() {
        assert_eq!(wikilink_spans("`[[not a link]]`"), Vec::new());
    }

    #[test]
    fn wikilink_spans_still_finds_a_real_wikilink_on_a_line_with_unrelated_code() {
        let text = "`code` and [[Topic]]";
        let spans = wikilink_spans(text);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].1, "Topic");
    }

    #[test]
    fn a_literal_private_use_area_character_does_not_panic() {
        // Regression test: WIKILINK_MARK ('\u{E000}') is used internally as a
        // placeholder delimiter; a document that already contains one (e.g. from an
        // icon-font paste) must not be able to desync that bookkeeping and panic.
        let blocks = parse("hello \u{E000} world");
        let joined: String = blocks[0].spans.iter().map(|s| s.text.as_str()).collect();
        assert!(joined.contains("hello"));
        assert!(joined.contains("world"));
        assert!(!joined.contains('\u{E000}'));
    }

    #[test]
    fn a_private_use_area_character_alongside_a_real_wikilink_still_resolves_the_link() {
        let blocks = parse("\u{E000} and [[Topic]]");
        let wikilink = blocks[0]
            .spans
            .iter()
            .find(|s| s.wikilink.is_some())
            .unwrap();
        assert_eq!(wikilink.wikilink.as_deref(), Some("Topic"));
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
    fn wikilink_resolves_matches_case_insensitively() {
        let titles = vec!["Chapter 1".to_string(), "Café".to_string()];
        assert!(wikilink_resolves("chapter 1", &titles));
        assert!(wikilink_resolves("CAFÉ", &titles));
    }

    #[test]
    fn wikilink_resolves_is_false_for_a_target_with_no_matching_document() {
        let titles = vec!["Chapter 1".to_string()];
        assert!(!wikilink_resolves("Chapter 2", &titles));
    }

    #[test]
    fn wikilink_target_at_ignores_fenced_code_blocks() {
        let text = "```\n[[Topic]]\n```\n";
        let inside = text.find("Topic").unwrap();
        assert_eq!(wikilink_target_at(text, inside), None);
    }

    #[test]
    fn inline_tag_spans_finds_a_simple_tag() {
        let spans = inline_tag_spans("Some #foo text.");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].1, "foo");
        assert_eq!(&"Some #foo text."[spans[0].0.clone()], "#foo");
    }

    #[test]
    fn inline_tag_spans_supports_nested_slash_tags() {
        let spans = inline_tag_spans("#projects/smaragd");
        assert_eq!(spans[0].1, "projects/smaragd");
    }

    #[test]
    fn inline_tag_spans_rejects_purely_numeric_tags() {
        assert_eq!(inline_tag_spans("See issue #42 for details."), Vec::new());
    }

    #[test]
    fn inline_tag_spans_rejects_a_hash_mid_word() {
        assert_eq!(inline_tag_spans("foo#bar"), Vec::new());
    }

    #[test]
    fn inline_tag_spans_does_not_match_atx_headings() {
        assert_eq!(inline_tag_spans("# Heading\n## Another"), Vec::new());
    }

    #[test]
    fn inline_tag_spans_matches_a_tag_at_the_very_start_of_a_line() {
        let spans = inline_tag_spans("#foo bar");
        assert_eq!(spans[0].1, "foo");
    }

    #[test]
    fn inline_tag_spans_matches_a_tag_after_punctuation() {
        let spans = inline_tag_spans("(#foo)");
        assert_eq!(spans[0].1, "foo");
    }

    #[test]
    fn inline_tag_spans_skips_fenced_code_blocks() {
        assert_eq!(inline_tag_spans("```\n#foo\n```\n"), Vec::new());
    }

    #[test]
    fn inline_tag_spans_skips_a_tag_inside_inline_code() {
        assert_eq!(inline_tag_spans("`#not-a-tag`"), Vec::new());
    }

    #[test]
    fn inline_tag_spans_still_finds_a_real_tag_on_a_line_with_unrelated_code() {
        let spans = inline_tag_spans("`code` and #foo");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].1, "foo");
    }

    #[test]
    fn inline_code_ranges_finds_multiple_spans_on_one_line() {
        let ranges = inline_code_ranges("`a` and `b`");
        assert_eq!(ranges.len(), 2);
        assert_eq!(&"`a` and `b`"[ranges[0].clone()], "`a`");
        assert_eq!(&"`a` and `b`"[ranges[1].clone()], "`b`");
    }

    #[test]
    fn inline_code_ranges_ignores_an_unterminated_trailing_backtick() {
        assert!(inline_code_ranges("some text with a stray ` backtick").is_empty());
    }

    #[test]
    fn inline_tag_spans_finds_multiple_tags_on_one_line() {
        let spans = inline_tag_spans("#foo and #bar");
        let tags: Vec<&str> = spans.iter().map(|(_, tag)| tag.as_str()).collect();
        assert_eq!(tags, vec!["foo", "bar"]);
    }

    #[test]
    fn inline_tags_dedups_case_insensitively_keeping_first_seen_casing() {
        assert_eq!(
            inline_tags("#Foo and #foo and #FOO"),
            vec!["Foo".to_string()]
        );
    }

    #[test]
    fn rename_tag_updates_a_simple_tag() {
        let updated = rename_tag("See #old-tag for more.", "old-tag", "new-tag").unwrap();
        assert_eq!(updated, "See #new-tag for more.");
    }

    #[test]
    fn rename_tag_is_case_insensitive() {
        let updated = rename_tag("#OLD-TAG", "old-tag", "new-tag").unwrap();
        assert_eq!(updated, "#new-tag");
    }

    #[test]
    fn rename_tag_updates_every_matching_occurrence() {
        let updated = rename_tag("#old-tag and again #old-tag", "old-tag", "new-tag").unwrap();
        assert_eq!(updated, "#new-tag and again #new-tag");
    }

    #[test]
    fn rename_tag_leaves_other_tags_untouched() {
        let updated = rename_tag("#unrelated", "old-tag", "new-tag");
        assert_eq!(updated, None);
    }

    #[test]
    fn rename_tag_does_not_match_a_tag_sharing_a_prefix() {
        let updated = rename_tag("#projects/smaragd stays put", "projects", "work");
        assert_eq!(updated, None);
    }

    #[test]
    fn rename_tag_renames_a_nested_tag_exactly() {
        let updated = rename_tag(
            "#projects/smaragd but not #projects",
            "projects/smaragd",
            "work/smaragd",
        )
        .unwrap();
        assert_eq!(updated, "#work/smaragd but not #projects");
    }

    #[test]
    fn rename_tag_skips_fenced_code_blocks() {
        let updated = rename_tag("```\n#old-tag\n```\n", "old-tag", "new-tag");
        assert_eq!(updated, None);
    }

    #[test]
    fn rename_tag_skips_inline_code_spans() {
        let updated = rename_tag("`#old-tag` stays literal", "old-tag", "new-tag");
        assert_eq!(updated, None);
    }

    fn only_span(markdown: &str) -> std::ops::Range<usize> {
        let spans = wikilink_spans(markdown);
        assert_eq!(
            spans.len(),
            1,
            "expected exactly one wikilink in {markdown:?}"
        );
        spans.into_iter().next().unwrap().0
    }

    #[test]
    fn wikilink_context_snippet_returns_whole_document_when_it_fits_the_budget() {
        let text = "Short intro. [[Topic]] short outro.";
        let range = only_span(text);
        assert_eq!(
            wikilink_context_snippet(text, &range),
            "Short intro. [[Topic]] short outro."
        );
    }

    #[test]
    fn wikilink_context_snippet_truncates_only_the_trailing_side_near_document_start() {
        let filler = "word ".repeat(100);
        let text = format!("[[Topic]] {filler}");
        let range = only_span(&text);
        let snippet = wikilink_context_snippet(&text, &range);
        assert!(snippet.starts_with("[[Topic]]"), "{snippet:?}");
        assert!(!snippet.starts_with('…'), "{snippet:?}");
        assert!(snippet.ends_with('…'), "{snippet:?}");
    }

    #[test]
    fn wikilink_context_snippet_truncates_only_the_leading_side_near_document_end() {
        let filler = "word ".repeat(100);
        let text = format!("{filler}[[Topic]]");
        let range = only_span(&text);
        let snippet = wikilink_context_snippet(&text, &range);
        assert!(snippet.ends_with("[[Topic]]"), "{snippet:?}");
        assert!(snippet.starts_with('…'), "{snippet:?}");
    }

    #[test]
    fn wikilink_context_snippet_truncates_both_sides_in_the_middle_of_a_long_document() {
        let filler = "word ".repeat(100);
        let text = format!("{filler}[[Topic]] {filler}");
        let range = only_span(&text);
        let snippet = wikilink_context_snippet(&text, &range);
        assert!(snippet.starts_with('…'), "{snippet:?}");
        assert!(snippet.ends_with('…'), "{snippet:?}");
        assert!(snippet.contains("[[Topic]]"), "{snippet:?}");
    }

    #[test]
    fn wikilink_context_snippet_collapses_an_embedded_newline_to_a_space() {
        let text = "Line one\n[[Topic]]\nLine two";
        let range = only_span(text);
        assert_eq!(
            wikilink_context_snippet(text, &range),
            "Line one [[Topic]] Line two"
        );
    }

    #[test]
    fn wikilink_context_snippet_does_not_split_a_multi_byte_character() {
        // Pad with enough multi-byte "é" characters that the computed cut point on
        // each side lands in the middle of a run of them, not just at a clean word
        // boundary at the very edge of the budget.
        let filler: String = std::iter::repeat_n('é', 200).collect();
        let text = format!("{filler}[[Topic]]{filler}");
        let range = only_span(&text);
        // Must not panic (a byte-index slice into the middle of a multi-byte 'é'
        // would), and the result must be valid, parseable UTF-8 by construction
        // (String always is) with the link intact.
        let snippet = wikilink_context_snippet(&text, &range);
        assert!(snippet.contains("[[Topic]]"), "{snippet:?}");
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

    fn joined_text(blocks: &[Block]) -> String {
        blocks[0].spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn typewriter_quotes_curls_a_simple_line_of_dialogue() {
        let mut blocks = parse(r#""Don't go," she said."#);
        apply_typewriter_quotes(&mut blocks);
        assert_eq!(joined_text(&blocks), "“Don’t go,” she said.");
    }

    #[test]
    fn typewriter_quotes_handles_nested_single_inside_double() {
        let mut blocks = parse(r#"She said, "He called it 'nonsense.'""#);
        apply_typewriter_quotes(&mut blocks);
        assert_eq!(joined_text(&blocks), "She said, “He called it ‘nonsense.’”");
    }

    #[test]
    fn typewriter_quotes_converts_double_dash_to_em_dash() {
        let mut blocks = parse("Wait--stop.");
        apply_typewriter_quotes(&mut blocks);
        assert_eq!(joined_text(&blocks), "Wait—stop.");
    }

    #[test]
    fn typewriter_quotes_converts_triple_dot_to_ellipsis() {
        let mut blocks = parse("I don't know...");
        apply_typewriter_quotes(&mut blocks);
        assert_eq!(joined_text(&blocks), "I don’t know…");
    }

    #[test]
    fn typewriter_quotes_treats_leading_elision_apostrophe_as_a_closing_curl() {
        let mut blocks = parse("Back in '99 it was different.");
        apply_typewriter_quotes(&mut blocks);
        assert_eq!(joined_text(&blocks), "Back in ’99 it was different.");
    }

    #[test]
    fn typewriter_quotes_leaves_inline_code_untouched() {
        let mut blocks = parse(r#"Run `git "commit"` please."#);
        apply_typewriter_quotes(&mut blocks);
        assert_eq!(joined_text(&blocks), r#"Run git "commit" please."#);
    }

    #[test]
    fn typewriter_quotes_leaves_fenced_code_blocks_untouched() {
        let mut blocks = parse("```\nlet s = \"hi\";\n```\n");
        apply_typewriter_quotes(&mut blocks);
        assert_eq!(blocks[0].spans[0].text, "let s = \"hi\";\n");
    }

    #[test]
    fn typewriter_quotes_curl_a_quote_that_spans_a_bold_run() {
        // The quote context (open vs. close) must survive a mid-sentence
        // formatting change into a separate Span, not just a single one.
        let mut blocks = parse(r#""very **bold** claim""#);
        apply_typewriter_quotes(&mut blocks);
        let joined = joined_text(&blocks);
        assert_eq!(joined, "“very bold claim”");
        assert!(blocks[0].spans[0].text.starts_with('“'));
        assert!(blocks[0].spans.last().unwrap().text.ends_with('”'));
    }

    #[test]
    fn typewriter_quotes_curls_table_cells_independently() {
        let mut blocks = parse("| A |\n|---|\n| \"hi\" |\n");
        apply_typewriter_quotes(&mut blocks);
        match &blocks[0].kind {
            BlockKind::Table { rows, .. } => {
                assert_eq!(rows[0][0][0].text, "“hi”");
            }
            other => panic!("expected a table, got {other:?}"),
        }
    }
}
