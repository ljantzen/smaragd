use serde::{Deserialize, Serialize};

/// Longform/Scrivener-style per-document metadata, stored as YAML frontmatter in a
/// `---`-delimited block at the very top of a markdown file. Every field is optional
/// in the YAML — a document with no frontmatter block, or an empty one, parses to
/// exactly `DocumentMeta::default()`.
///
/// Serialization back to disk (writing frontmatter) is deliberately not implemented
/// yet — nothing in the app calls it. `Serialize` is derived now only for parity with
/// `ProjectMeta`/`Settings` and to keep round-trip unit tests convenient; preserving
/// unknown/custom YAML keys on a future write-back is explicit future work.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DocumentMeta {
    /// Free-form section type ("Chapter", "Scene", "Part", or any user-defined
    /// string) — deliberately not a closed enum, and deliberately decoupled from
    /// folder nesting depth. YAML key is `type` (matching the vocabulary
    /// Longform-style tools use); `type` is a Rust keyword, hence the rename.
    #[serde(rename = "type")]
    pub section_type: Option<String>,
    /// Free-form drafting status ("draft", "revised", "final", ...) — not a closed
    /// enum, same reasoning as `section_type`.
    pub status: Option<String>,
    /// Point-of-view character/name, free text.
    pub pov: Option<String>,
    /// Target word count for this document, if the author has set one.
    pub word_count_target: Option<u32>,
    pub tags: Vec<String>,
}

/// A frontmatter block found at the top of a document: its raw YAML content, and the
/// byte offset (into the original string) where the body after it begins.
struct Frontmatter<'a> {
    yaml: &'a str,
    body_start: usize,
}

/// Find a leading `---`-delimited frontmatter block in `contents`, if any. The
/// opening delimiter must be exactly `---` (only a trailing `\r` tolerated, for CRLF
/// files) as the very first line; the same exact match closes it. No leading/trailing
/// whitespace tolerance, no YAML `...` end marker — matching the vast majority of
/// real frontmatter producers (Jekyll/Hugo/Obsidian all use exactly this).
fn extract_block(contents: &str) -> Option<Frontmatter<'_>> {
    let first_line_end = contents.find('\n').map(|i| i + 1).unwrap_or(contents.len());
    let first_line = contents[..first_line_end].trim_end_matches(['\n', '\r']);
    if first_line != "---" || first_line_end == contents.len() {
        return None;
    }

    let mut cursor = first_line_end;
    for line in contents[cursor..].split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            return Some(Frontmatter {
                yaml: &contents[first_line_end..cursor],
                body_start: cursor + line.len(),
            });
        }
        cursor += line.len();
    }

    // No closing "---" found before EOF: unterminated, same spirit as markdown.rs's
    // "unterminated [[wikilink" -> plain text" handling.
    None
}

fn parse_yaml_block(yaml: &str) -> DocumentMeta {
    // An empty (or whitespace-only) block means "no fields set" — handled explicitly
    // rather than relying on serde_norway's empty-document behavior, since an empty
    // YAML document deserializes to `null`, which the struct visitor rejects.
    if yaml.trim().is_empty() {
        return DocumentMeta::default();
    }
    serde_norway::from_str(yaml).unwrap_or_default()
}

/// Parse the YAML frontmatter block at the top of `contents`, if any. Never fails —
/// missing, malformed, non-mapping, or unterminated frontmatter all yield
/// `DocumentMeta::default()`, matching this codebase's existing tolerant-load
/// philosophy (`ProjectMeta`/`Settings` loading, markdown.rs's unterminated-marker
/// handling).
pub fn parse(contents: &str) -> DocumentMeta {
    extract_block(contents)
        .map(|fm| parse_yaml_block(fm.yaml))
        .unwrap_or_default()
}

/// Strip a leading YAML frontmatter block from `contents`, returning just the
/// markdown body that follows. Returned unchanged if there's no (or a malformed/
/// unterminated) frontmatter block. Used before handing text to `markdown::parse` so
/// the raw `---`/YAML lines never render as a garbled paragraph.
pub fn strip(contents: &str) -> &str {
    match extract_block(contents) {
        Some(fm) => &contents[fm.body_start..],
        None => contents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_returns_default_when_no_frontmatter_present() {
        assert_eq!(parse("# Just a heading\n\nSome text."), DocumentMeta::default());
    }

    #[test]
    fn parse_reads_all_fields_from_a_well_formed_block() {
        let contents = "---\ntype: Scene\nstatus: draft\npov: Alice\nword_count_target: 2500\ntags: [foo, bar]\n---\nBody text.\n";
        let meta = parse(contents);
        assert_eq!(
            meta,
            DocumentMeta {
                section_type: Some("Scene".to_string()),
                status: Some("draft".to_string()),
                pov: Some("Alice".to_string()),
                word_count_target: Some(2500),
                tags: vec!["foo".to_string(), "bar".to_string()],
            }
        );
    }

    #[test]
    fn parse_supports_block_style_tag_list() {
        let contents = "---\ntags:\n  - foo\n  - bar\n---\nBody.\n";
        assert_eq!(
            parse(contents).tags,
            vec!["foo".to_string(), "bar".to_string()]
        );
    }

    #[test]
    fn parse_supports_flow_style_tag_list() {
        let contents = "---\ntags: [foo, bar]\n---\nBody.\n";
        assert_eq!(
            parse(contents).tags,
            vec!["foo".to_string(), "bar".to_string()]
        );
    }

    #[test]
    fn parse_handles_quoted_values_containing_colons() {
        let contents = "---\npov: \"Jane: the elder sister\"\n---\nBody.\n";
        assert_eq!(
            parse(contents).pov,
            Some("Jane: the elder sister".to_string())
        );
    }

    #[test]
    fn parse_treats_an_empty_frontmatter_block_as_all_defaults() {
        assert_eq!(parse("---\n---\nBody.\n"), DocumentMeta::default());
    }

    #[test]
    fn parse_falls_back_to_default_when_opening_marker_has_no_closing_delimiter() {
        let contents = "---\ntype: Scene\nno closing delimiter here";
        assert_eq!(parse(contents), DocumentMeta::default());
    }

    #[test]
    fn parse_falls_back_to_default_on_yaml_that_is_not_a_mapping() {
        let contents = "---\n- a\n- b\n---\nBody.\n";
        assert_eq!(parse(contents), DocumentMeta::default());
    }

    #[test]
    fn parse_ignores_unknown_keys_without_erroring() {
        let contents = "---\ncustom_field: something\nstatus: draft\n---\nBody.\n";
        assert_eq!(parse(contents).status, Some("draft".to_string()));
    }

    #[test]
    fn parse_treats_a_first_line_that_is_not_exactly_dashes_as_plain_body() {
        assert_eq!(
            parse("--- not frontmatter\ntype: Scene\n---\n"),
            DocumentMeta::default()
        );
    }

    #[test]
    fn word_count_target_defaults_to_none_when_absent_but_other_fields_present() {
        let contents = "---\nstatus: draft\n---\nBody.\n";
        let meta = parse(contents);
        assert_eq!(meta.status, Some("draft".to_string()));
        assert_eq!(meta.word_count_target, None);
    }

    #[test]
    fn strip_removes_frontmatter_block_and_leaves_body_intact() {
        let contents = "---\ntype: Scene\n---\n# Heading\n\nBody text.\n";
        assert_eq!(strip(contents), "# Heading\n\nBody text.\n");
    }

    #[test]
    fn strip_returns_input_unchanged_when_there_is_no_frontmatter() {
        let contents = "# Heading\n\nBody text.\n";
        assert_eq!(strip(contents), contents);
    }

    #[test]
    fn strip_returns_input_unchanged_for_unterminated_frontmatter_marker() {
        let contents = "---\ntype: Scene\nstill going...";
        assert_eq!(strip(contents), contents);
    }

    #[test]
    fn strip_handles_frontmatter_with_no_body_after_it() {
        assert_eq!(strip("---\ntype: Scene\n---\n"), "");
    }
}
