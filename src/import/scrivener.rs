//! Parses a `.scriv` project directory into [`super::ImportedNode`]s.
//!
//! **The highest-risk importer of the four.** Scrivener's `.scrivx` binder
//! format has no formal published schema, and this was written without a
//! real sample project to validate against — it's scoped from best-available
//! public knowledge of the format (the type strings and directory layout
//! below are what's widely documented/reverse-engineered by other
//! open-source Scrivener tooling, not verified against a real file). Expect
//! this one specifically to need follow-up fixes once tried against an
//! actual Scrivener project; not something further guessing can substitute
//! for.
//!
//! A `.scriv` "project" is a **directory**, not an archive: `<name>.scrivx`
//! (an XML binder index) alongside `Files/Data/<UUID>/content.rtf` (or
//! `content.txt`) per item. `<BinderItem Type="...">`: `DraftFolder` (also
//! seen as `Draft` in older Scrivener 2 files) is the project's one
//! manuscript folder, mapped to smaragd's `FolderRole::Manuscript`;
//! `TrashFolder`/`Trash` is skipped entirely rather than imported (re-
//! importing already-discarded content isn't likely wanted); `Folder` is a
//! plain folder (including one named "Research" — Scrivener's Research
//! folder isn't a distinct structural `Type` as far as this could be
//! confirmed without a sample, so it isn't special-cased here, unlike Draft/
//! Trash which are); `Text` is a document. Anything else is treated as a
//! plain folder if it has children, else skipped.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;

use super::{ImportError, ImportedKind, ImportedNode};
use crate::export::sanitize_filename_component;
use crate::project::FolderRole;

#[derive(Debug)]
pub enum ScrivenerImportError {
    Io(io::Error),
    /// No `*.scrivx` file found directly inside the given directory — not a
    /// `.scriv` project (or not the top-level project folder).
    MissingScrivxFile,
}

impl fmt::Display for ScrivenerImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScrivenerImportError::Io(err) => write!(f, "{err}"),
            ScrivenerImportError::MissingScrivxFile => {
                write!(
                    f,
                    "no .scrivx file found — is this a Scrivener project folder?"
                )
            }
        }
    }
}

impl From<io::Error> for ScrivenerImportError {
    fn from(err: io::Error) -> Self {
        ScrivenerImportError::Io(err)
    }
}

/// Parses the `.scriv` project directory at `project_dir` into a tree of
/// [`ImportedNode`]s, ready for [`super::write_imported_tree`].
pub fn parse(project_dir: &Path) -> Result<Vec<ImportedNode>, ImportError> {
    parse_inner(project_dir).map_err(ImportError::Scrivener)
}

fn parse_inner(project_dir: &Path) -> Result<Vec<ImportedNode>, ScrivenerImportError> {
    let scrivx_path = find_scrivx_file(project_dir)?;
    let scrivx_xml = fs::read_to_string(&scrivx_path)?;
    let items = parse_binder(&scrivx_xml);
    Ok(items
        .into_iter()
        .filter_map(|item| raw_item_to_imported_node(&item, project_dir))
        .collect())
}

fn find_scrivx_file(project_dir: &Path) -> Result<std::path::PathBuf, ScrivenerImportError> {
    for entry in fs::read_dir(project_dir)?.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("scrivx"))
        {
            return Ok(path);
        }
    }
    Err(ScrivenerImportError::MissingScrivxFile)
}

// ---------------------------------------------------------------------
// `.scrivx` binder XML -> an intermediate tree
// ---------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum ItemKind {
    Draft,
    Trash,
    Folder,
    Text,
    Other,
}

impl ItemKind {
    fn from_type_attr(value: &str) -> Self {
        match value {
            "DraftFolder" | "Draft" => ItemKind::Draft,
            "TrashFolder" | "Trash" => ItemKind::Trash,
            "Folder" => ItemKind::Folder,
            "Text" => ItemKind::Text,
            _ => ItemKind::Other,
        }
    }
}

struct RawItem {
    kind: ItemKind,
    title: String,
    uuid: String,
    children: Vec<RawItem>,
}

/// One in-progress `<BinderItem>` frame, built up as its `Type`/`UUID`
/// attributes and nested `<Title>`/`<BinderItem>` children are seen, then
/// popped into its parent's `children` (or the top-level list) on `</BinderItem>`.
struct Frame {
    kind: ItemKind,
    uuid: String,
    title: String,
    children: Vec<RawItem>,
}

fn parse_binder(scrivx_xml: &str) -> Vec<RawItem> {
    let mut reader = Reader::from_str(scrivx_xml);
    let mut stack: Vec<Frame> = Vec::new();
    let mut top_level: Vec<RawItem> = Vec::new();
    let mut in_title = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"BinderItem" => {
                    let mut kind = ItemKind::Other;
                    let mut uuid = String::new();
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"Type" => {
                                if let Some(value) = attr_value(&attr) {
                                    kind = ItemKind::from_type_attr(&value);
                                }
                            }
                            b"UUID" => uuid = attr_value(&attr).unwrap_or_default(),
                            _ => {}
                        }
                    }
                    stack.push(Frame {
                        kind,
                        uuid,
                        title: String::new(),
                        children: Vec::new(),
                    });
                }
                b"Title" => in_title = true,
                _ => {}
            },
            Ok(Event::Text(e)) if in_title => {
                if let Some(frame) = stack.last_mut() {
                    frame.title.push_str(&e.decode().unwrap_or_default());
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"BinderItem" => {
                    if let Some(frame) = stack.pop() {
                        let item = RawItem {
                            kind: frame.kind,
                            title: frame.title.trim().to_string(),
                            uuid: frame.uuid,
                            children: frame.children,
                        };
                        match stack.last_mut() {
                            Some(parent) => parent.children.push(item),
                            None => top_level.push(item),
                        }
                    }
                }
                b"Title" => in_title = false,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    top_level
}

fn attr_value(attr: &quick_xml::events::attributes::Attribute) -> Option<String> {
    std::str::from_utf8(&attr.value).ok().map(str::to_string)
}

// ---------------------------------------------------------------------
// Intermediate tree -> `ImportedNode`
// ---------------------------------------------------------------------

/// `None` for a `Trash`(Folder) item (skipped entirely) or an unrecognized
/// leaf with no children (nothing importable there).
fn raw_item_to_imported_node(item: &RawItem, project_dir: &Path) -> Option<ImportedNode> {
    let name = if item.title.is_empty() {
        "Untitled".to_string()
    } else {
        sanitize_filename_component(&item.title)
    };
    match item.kind {
        ItemKind::Trash => None,
        ItemKind::Text => {
            let markdown = read_item_content(project_dir, &item.uuid).unwrap_or_default();
            Some(ImportedNode {
                name,
                kind: ImportedKind::Document { markdown },
            })
        }
        ItemKind::Draft => Some(ImportedNode {
            name,
            kind: ImportedKind::Folder {
                role: Some(FolderRole::Manuscript),
                children: child_nodes(item, project_dir),
            },
        }),
        ItemKind::Folder | ItemKind::Other => {
            let children = child_nodes(item, project_dir);
            if item.kind == ItemKind::Other && children.is_empty() {
                None
            } else {
                Some(ImportedNode {
                    name,
                    kind: ImportedKind::Folder {
                        role: None,
                        children,
                    },
                })
            }
        }
    }
}

fn child_nodes(item: &RawItem, project_dir: &Path) -> Vec<ImportedNode> {
    item.children
        .iter()
        .filter_map(|child| raw_item_to_imported_node(child, project_dir))
        .collect()
}

/// `Files/Data/<uuid>/content.rtf`, or `content.txt` if that's what exists
/// instead (Scrivener supports plain-text documents too) — `None` (empty
/// content, not an import failure) if neither file exists, e.g. a genuinely
/// empty placeholder document.
fn read_item_content(project_dir: &Path, uuid: &str) -> Option<String> {
    let item_dir = project_dir.join("Files").join("Data").join(uuid);
    if let Ok(bytes) = fs::read(item_dir.join("content.rtf")) {
        return Some(rtf_to_markdown(&bytes));
    }
    fs::read_to_string(item_dir.join("content.txt")).ok()
}

// ---------------------------------------------------------------------
// RTF -> markdown — a small, purpose-built converter, not a general-purpose
// RTF library: covers exactly what Scrivener's own prose RTF uses (`\par`
// paragraphs, `\b`/`\i` bold/italic, `\'XX` codepage-1252 and `\uN` Unicode
// text escapes, skipping non-text destination groups like `\fonttbl`), and
// silently drops anything else (tables, images, footnotes, complex nested
// formatting) rather than erroring the whole import over it.
// ---------------------------------------------------------------------

/// A destination group's controlling keyword — its whole `{...}` body is
/// skipped, since it's metadata (fonts/colors/styles/document info), not
/// document text.
const SKIPPED_DESTINATIONS: &[&str] = &[
    "fonttbl",
    "colortbl",
    "stylesheet",
    "info",
    "generator",
    "pict",
    "object",
    "themedata",
    "colorschememapping",
    "listtable",
    "listoverridetable",
    "rsidtable",
    "xmlnstbl",
    "footer",
    "header",
    "footnote",
];

#[derive(Clone, Copy, Default)]
struct RtfState {
    bold: bool,
    italic: bool,
}

/// One maximal run of text sharing the same formatting — coalesced this way
/// (rather than wrapping markers character-by-character) so e.g. `bold` in
/// `\b bold\b0  text` becomes `**bold** text`, not `**b**o**l**d`.
struct Run {
    text: String,
    state: RtfState,
}

fn rtf_to_markdown(bytes: &[u8]) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut group_stack: Vec<RtfState> = vec![RtfState::default()];
    // How many characters after a `\uN` to skip, per the RTF spec's `\ucN`
    // (defaults to 1) — the ASCII fallback glyph(s) RTF writers emit
    // alongside a Unicode escape for readers that don't support `\u`.
    let mut unicode_skip_width: i32 = 1;

    let mut i = 0usize;
    while i < bytes.len() {
        let byte = bytes[i];
        match byte {
            b'{' => {
                // Checked *before* pushing group state: a skipped group's
                // matching `}` is never processed (we jump straight past
                // it), so pushing here first would leave an extra frame on
                // `group_stack` that's never popped, corrupting every
                // following paragraph's bold/italic state for the rest of
                // the document.
                if let Some(skip_to) = skip_if_destination_group(bytes, i) {
                    i = skip_to;
                    continue;
                }
                let state = *group_stack.last().unwrap_or(&RtfState::default());
                group_stack.push(state);
                i += 1;
            }
            b'}' => {
                if group_stack.len() > 1 {
                    group_stack.pop();
                }
                i += 1;
            }
            b'\\' => {
                let (advance, action) = read_control(bytes, i);
                i += advance;
                match action {
                    ControlAction::Bold(on) => {
                        if let Some(state) = group_stack.last_mut() {
                            state.bold = on;
                        }
                    }
                    ControlAction::Italic(on) => {
                        if let Some(state) = group_stack.last_mut() {
                            state.italic = on;
                        }
                    }
                    ControlAction::Par => {
                        flush_paragraph(&mut runs, &mut paragraphs);
                    }
                    ControlAction::Line => {
                        push_char(&mut runs, &group_stack, '\n');
                    }
                    ControlAction::UnicodeSkipWidth(n) => unicode_skip_width = n,
                    ControlAction::Char(ch) => push_char(&mut runs, &group_stack, ch),
                    ControlAction::Unicode(ch) => {
                        push_char(&mut runs, &group_stack, ch);
                        // Consume the following `unicode_skip_width` bytes as
                        // the ASCII fallback that accompanies a `\u` escape —
                        // not document text of its own.
                        let mut skipped = 0;
                        while skipped < unicode_skip_width && i < bytes.len() {
                            if bytes[i] == b'\\' {
                                let (adv, _) = read_control(bytes, i);
                                i += adv;
                            } else {
                                i += 1;
                            }
                            skipped += 1;
                        }
                    }
                    ControlAction::None => {}
                }
            }
            b'\r' | b'\n' => {
                // Source-file line breaks, not semantic paragraph breaks —
                // only `\par` means that. Treated as plain whitespace.
                i += 1;
            }
            _ => {
                push_char(&mut runs, &group_stack, byte as char);
                i += 1;
            }
        }
    }
    flush_paragraph(&mut runs, &mut paragraphs);
    paragraphs.join("\n\n")
}

enum ControlAction {
    Bold(bool),
    Italic(bool),
    Par,
    Line,
    UnicodeSkipWidth(i32),
    Char(char),
    Unicode(char),
    None,
}

/// Reads one control word (`\word123 `) or control symbol (`\'XX`, `\~`,
/// `\_`, `\\`, `\{`, `\}`) starting at `bytes[at]` (the `\`). Returns how
/// many bytes it consumed and what it means.
fn read_control(bytes: &[u8], at: usize) -> (usize, ControlAction) {
    let mut i = at + 1;
    if i >= bytes.len() {
        return (1, ControlAction::None);
    }
    match bytes[i] {
        b'\'' => {
            // `\'XX`: two hex digits, a codepage-1252 byte.
            let hex: String = bytes
                .get(i + 1..i + 3)
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default();
            let consumed = 1 + 1 + hex.len();
            match u8::from_str_radix(&hex, 16) {
                Ok(byte) => (consumed, ControlAction::Char(cp1252_to_char(byte))),
                Err(_) => (consumed, ControlAction::None),
            }
        }
        b'\\' | b'{' | b'}' => (2, ControlAction::Char(bytes[i] as char)),
        b'~' => (2, ControlAction::Char('\u{a0}')),
        b'_' => (2, ControlAction::Char('-')),
        b if b.is_ascii_alphabetic() => {
            let word_start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let word = std::str::from_utf8(&bytes[word_start..i]).unwrap_or("");
            let num_start = i;
            let negative = i < bytes.len() && bytes[i] == b'-';
            if negative {
                i += 1;
            }
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let num: Option<i32> = bytes
                .get(num_start..i)
                .and_then(|b| std::str::from_utf8(b).ok())
                .and_then(|s| s.parse().ok());
            // A single trailing space delimits the control word without
            // being part of the document text.
            if i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            let consumed = i - at;
            let action = match word {
                // `\pard` (paragraph *formatting* reset) deliberately falls
                // through to `_ => ControlAction::None` below — only a
                // literal `\par` is a real paragraph break.
                "par" => ControlAction::Par,
                "line" => ControlAction::Line,
                "b" => ControlAction::Bold(num != Some(0)),
                "i" => ControlAction::Italic(num != Some(0)),
                "uc" => ControlAction::UnicodeSkipWidth(num.unwrap_or(1)),
                "u" => match num.and_then(|n| char::from_u32(n.rem_euclid(0x10000) as u32)) {
                    Some(ch) => ControlAction::Unicode(ch),
                    None => ControlAction::None,
                },
                "tab" => ControlAction::Char('\t'),
                "emdash" => ControlAction::Char('\u{2014}'),
                "endash" => ControlAction::Char('\u{2013}'),
                "lquote" => ControlAction::Char('\u{2018}'),
                "rquote" => ControlAction::Char('\u{2019}'),
                "ldblquote" => ControlAction::Char('\u{201c}'),
                "rdblquote" => ControlAction::Char('\u{201d}'),
                _ => ControlAction::None,
            };
            (consumed, action)
        }
        _ => (2, ControlAction::None),
    }
}

/// Whether `{` at `bytes[open_brace]` starts a destination group whose whole
/// body should be skipped (`{\*\...}` or `{\fonttbl ...}` and similar) —
/// `Some(index right after the matching '}')` if so, tracking nested braces
/// so a group containing its own sub-groups is skipped correctly.
fn skip_if_destination_group(bytes: &[u8], open_brace: usize) -> Option<usize> {
    let mut i = open_brace + 1;
    // `{\*\keyword` — the `\*` marks "skip if unrecognized," which for this
    // purpose means always skip (we don't render any destination content).
    let starts_star = bytes.get(i) == Some(&b'\\') && bytes.get(i + 1) == Some(&b'*');
    if starts_star {
        i += 2;
    }
    if bytes.get(i) != Some(&b'\\') {
        if !starts_star {
            return None;
        }
    } else {
        let word_start = i + 1;
        let mut word_end = word_start;
        while word_end < bytes.len() && bytes[word_end].is_ascii_alphabetic() {
            word_end += 1;
        }
        let word = std::str::from_utf8(&bytes[word_start..word_end]).unwrap_or("");
        if !starts_star && !SKIPPED_DESTINATIONS.contains(&word) {
            return None;
        }
    }

    let mut depth = 1i32;
    let mut j = open_brace + 1;
    while j < bytes.len() && depth > 0 {
        match bytes[j] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'\\' if bytes.get(j + 1) == Some(&b'\'') => {
                j += 3; // skip the `\'XX` hex pair so a literal `{`/`}` byte
                // value inside it is never mistaken for a real brace.
            }
            _ => {}
        }
        j += 1;
    }
    Some(j)
}

fn push_char(runs: &mut Vec<Run>, group_stack: &[RtfState], ch: char) {
    let state = *group_stack.last().unwrap_or(&RtfState::default());
    match runs.last_mut() {
        Some(run) if run.state.bold == state.bold && run.state.italic == state.italic => {
            run.text.push(ch);
        }
        _ => runs.push(Run {
            text: ch.to_string(),
            state,
        }),
    }
}

fn flush_paragraph(runs: &mut Vec<Run>, paragraphs: &mut Vec<String>) {
    if runs.is_empty() {
        return;
    }
    let mut markdown = String::new();
    for run in runs.drain(..) {
        let trimmed = run.text.trim();
        if trimmed.is_empty() {
            markdown.push_str(&run.text);
            continue;
        }
        let leading = &run.text[..run.text.len() - run.text.trim_start().len()];
        let trailing = &run.text[leading.len() + trimmed.len()..];
        let mut emphasized = trimmed.to_string();
        if run.state.bold {
            emphasized = format!("**{emphasized}**");
        }
        if run.state.italic {
            emphasized = format!("_{emphasized}_");
        }
        markdown.push_str(leading);
        markdown.push_str(&emphasized);
        markdown.push_str(trailing);
    }
    let markdown = markdown.trim().to_string();
    if !markdown.is_empty() {
        paragraphs.push(markdown);
    }
}

/// Windows-1252 (cp1252) byte -> Unicode, for `\'XX` escapes. ASCII
/// (0x00-0x7F) and 0xA0-0xFF (identical to Latin-1 in cp1252) map directly;
/// only the 0x80-0x9F range actually differs and needs a real lookup table —
/// it's where the punctuation prose most commonly hits (curly quotes,
/// em/en-dash, ellipsis) lives when a document was written with cp1252
/// escapes rather than `\u` Unicode escapes.
fn cp1252_to_char(byte: u8) -> char {
    match byte {
        0x80 => '\u{20ac}',
        0x82 => '\u{201a}',
        0x83 => '\u{0192}',
        0x84 => '\u{201e}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02c6}',
        0x89 => '\u{2030}',
        0x8a => '\u{0160}',
        0x8b => '\u{2039}',
        0x8c => '\u{0152}',
        0x8e => '\u{017d}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201c}',
        0x94 => '\u{201d}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02dc}',
        0x99 => '\u{2122}',
        0x9a => '\u{0161}',
        0x9b => '\u{203a}',
        0x9c => '\u{0153}',
        0x9e => '\u{017e}',
        0x9f => '\u{0178}',
        _ => byte as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_node(title: &str) -> RawItem {
        RawItem {
            kind: ItemKind::Text,
            title: title.to_string(),
            uuid: "00000000-0000-0000-0000-000000000000".to_string(),
            children: Vec::new(),
        }
    }

    #[test]
    fn parse_binder_reads_titles_and_nested_children() {
        let scrivx = r#"<?xml version="1.0"?>
<ScrivenerProject>
  <Binder>
    <BinderItem UUID="AAA" Type="DraftFolder">
      <Title>Manuscript</Title>
      <Children>
        <BinderItem UUID="BBB" Type="Text">
          <Title>Chapter One</Title>
        </BinderItem>
      </Children>
    </BinderItem>
    <BinderItem UUID="CCC" Type="TrashFolder">
      <Title>Trash</Title>
    </BinderItem>
  </Binder>
</ScrivenerProject>"#;

        let items = parse_binder(scrivx);

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, ItemKind::Draft);
        assert_eq!(items[0].title, "Manuscript");
        assert_eq!(items[0].children.len(), 1);
        assert_eq!(items[0].children[0].kind, ItemKind::Text);
        assert_eq!(items[0].children[0].title, "Chapter One");
        assert_eq!(items[1].kind, ItemKind::Trash);
    }

    #[test]
    fn raw_item_to_imported_node_skips_trash() {
        let item = RawItem {
            kind: ItemKind::Trash,
            title: "Trash".to_string(),
            uuid: String::new(),
            children: vec![text_node("Old draft")],
        };

        assert!(raw_item_to_imported_node(&item, Path::new("/nonexistent")).is_none());
    }

    #[test]
    fn raw_item_to_imported_node_maps_draft_to_the_manuscript_role() {
        let item = RawItem {
            kind: ItemKind::Draft,
            title: "Manuscript".to_string(),
            uuid: String::new(),
            children: vec![],
        };

        let node = raw_item_to_imported_node(&item, Path::new("/nonexistent")).unwrap();

        assert!(matches!(
            node.kind,
            ImportedKind::Folder {
                role: Some(FolderRole::Manuscript),
                ..
            }
        ));
    }

    #[test]
    fn rtf_to_markdown_extracts_plain_text_and_splits_paragraphs_on_par() {
        let rtf = br"{\rtf1\ansi\deff0 Hello world.\par Second paragraph.}";

        assert_eq!(rtf_to_markdown(rtf), "Hello world.\n\nSecond paragraph.");
    }

    #[test]
    fn rtf_to_markdown_wraps_bold_and_italic_runs() {
        let rtf = br"{\rtf1 plain \b bold\b0  \i italic\i0  plain}";

        assert_eq!(rtf_to_markdown(rtf), "plain **bold** _italic_ plain");
    }

    #[test]
    fn rtf_to_markdown_skips_font_table_destination_content() {
        let rtf = br"{\rtf1{\fonttbl{\f0 Times New Roman;}}Actual text.}";

        assert_eq!(rtf_to_markdown(rtf), "Actual text.");
    }

    #[test]
    fn rtf_to_markdown_decodes_cp1252_smart_quotes() {
        let rtf = br"{\rtf1 \'93quoted\'94 and \'97dash}";

        assert_eq!(
            rtf_to_markdown(rtf),
            "\u{201c}quoted\u{201d} and \u{2014}dash"
        );
    }

    #[test]
    fn rtf_to_markdown_decodes_unicode_escapes_and_skips_their_ascii_fallback() {
        let rtf = br"{\rtf1 caf\u233\'e9}";

        assert_eq!(rtf_to_markdown(rtf), "caf\u{e9}");
    }
}
