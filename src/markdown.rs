use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

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
}

struct ListLevel {
    ordered: bool,
    next_index: Option<u64>,
}

/// Parse `markdown` into a flat sequence of blocks. Deliberately does not model
/// arbitrarily deep nesting (e.g. a blockquote inside a list inside a blockquote) or
/// tables/images/footnotes — a flat block list with single-level list nesting covers
/// what an author actually writes, and matches what the glow-style preview renders.
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

    for event in Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH) {
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

            Event::Text(text) => {
                if current_kind.is_some() {
                    let template = Span {
                        bold: bold_depth > 0,
                        italic: italic_depth > 0,
                        code: in_code_block,
                        strikethrough: strike_depth > 0,
                        link: link_stack.last().cloned(),
                        ..Default::default()
                    };
                    if in_code_block {
                        spans.push(Span {
                            text: text.to_string(),
                            ..template
                        });
                    } else {
                        expand_placeholders(&mut spans, &text, &template, &wikilinks);
                    }
                }
            }
            Event::Code(text) => {
                if current_kind.is_some() {
                    spans.push(Span {
                        text: text.to_string(),
                        bold: bold_depth > 0,
                        italic: italic_depth > 0,
                        code: true,
                        strikethrough: strike_depth > 0,
                        link: link_stack.last().cloned(),
                        wikilink: None,
                    });
                }
            }
            Event::SoftBreak => {
                if let Some(last) = spans.last_mut() {
                    last.text.push(' ');
                } else {
                    spans.push(Span {
                        text: " ".to_string(),
                        ..Default::default()
                    });
                }
            }
            Event::HardBreak => spans.push(Span {
                text: "\n".to_string(),
                ..Default::default()
            }),
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

/// Replace `[[Topic]]` / `[[Topic|Alias]]` wikilinks in `markdown` with placeholders
/// pulldown-cmark's inline parser won't fragment, skipping fenced code blocks (whose
/// content must stay literal). Returns the rewritten markdown plus a side table of
/// `(target, display)` pairs, indexed by the number embedded in each placeholder.
fn extract_wikilinks(markdown: &str) -> (String, Vec<(String, String)>) {
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

fn replace_wikilinks_in_line(line: &str, table: &mut Vec<(String, String)>, output: &mut String) {
    let mut rest = line;
    while let Some(start) = rest.find("[[") {
        output.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        match after_open.find("]]") {
            Some(end) => {
                let inner = &after_open[..end];
                let (target, display) = match inner.split_once('|') {
                    Some((target, alias)) => (target.trim().to_string(), alias.trim().to_string()),
                    None => (inner.trim().to_string(), inner.trim().to_string()),
                };
                table.push((target, display));
                output.push(WIKILINK_MARK);
                output.push_str(&(table.len() - 1).to_string());
                output.push(WIKILINK_MARK);
                rest = &after_open[end + 2..];
            }
            None => {
                // Unterminated "[[": no wikilink here, keep the rest as plain text.
                output.push_str(&rest[start..]);
                return;
            }
        }
    }
    output.push_str(rest);
}

/// Split `text` (already through pulldown-cmark) on wikilink placeholders inserted by
/// [`extract_wikilinks`], pushing plain-text spans and wikilink spans in order, all
/// inheriting `template`'s formatting.
fn expand_placeholders(
    spans: &mut Vec<Span>,
    text: &str,
    template: &Span,
    table: &[(String, String)],
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
        let (target, display) = &table[index];
        spans.push(Span {
            text: display.clone(),
            wikilink: Some(target.clone()),
            ..template.clone()
        });
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
}
