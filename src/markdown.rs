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
                    spans.push(Span {
                        text: text.to_string(),
                        bold: bold_depth > 0,
                        italic: italic_depth > 0,
                        code: in_code_block,
                        strikethrough: strike_depth > 0,
                        link: link_stack.last().cloned(),
                    });
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
}
