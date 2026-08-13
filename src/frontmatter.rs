use serde::{Deserialize, Serialize};

/// Longform/Scrivener-style per-document metadata, stored as YAML frontmatter in a
/// `---`-delimited block at the very top of a markdown file. Every field is optional
/// in the YAML — a document with no frontmatter block, or an empty one, parses to
/// exactly `DocumentMeta::default()`.
///
/// Writing this back to disk (see `write_back`) preserves any YAML key/value data
/// this struct doesn't know about — it never round-trips *through* `DocumentMeta`
/// itself, which would silently drop it; `write_back` only ever touches the five
/// keys below in the raw YAML mapping. It does not, however, preserve comments or
/// the original formatting/key order of the block, since `serde_norway::Mapping`
/// has no representation for either. `Serialize` is derived for parity with
/// `ProjectMeta`/`Settings` and round-trip unit test convenience, not because
/// `DocumentMeta` itself is ever serialized directly to produce a frontmatter block.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
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
/// opening delimiter must be `---` (trailing whitespace — a trailing `\r` for CRLF
/// files, or stray trailing spaces/tabs a template or editor left behind — tolerated
/// and ignored) as the very first line; the same tolerant match closes it. No leading
/// whitespace tolerance, no YAML `...` end marker — matching the vast majority of real
/// frontmatter producers (Jekyll/Hugo/Obsidian all use exactly this). Trailing
/// whitespace specifically *is* tolerated, unlike leading, because it's invisible in
/// most editors and easy to introduce by accident (e.g. a template file saved with a
/// trailing space on its `---` line) — treating a delimiter line as "no frontmatter
/// at all" over that would silently drop every field, far worse than just ignoring
/// whitespace nobody meant to be significant.
fn extract_block(contents: &str) -> Option<Frontmatter<'_>> {
    let first_line_end = contents.find('\n').map(|i| i + 1).unwrap_or(contents.len());
    let first_line = contents[..first_line_end].trim_end();
    if first_line != "---" || first_line_end == contents.len() {
        return None;
    }

    let mut cursor = first_line_end;
    for line in contents[cursor..].split_inclusive('\n') {
        let trimmed = line.trim_end();
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

/// A diagnostic for a frontmatter block whose YAML content failed to parse — the
/// `---`-delimited block exists, but `serde_norway` couldn't read it as a mapping
/// `parse` can use (so it silently fell back to `DocumentMeta::default()` instead).
/// Surfaced as a status-bar warning rather than blocking anything, matching this
/// codebase's tolerant-load philosophy — a bad block never stops a document from
/// opening or saving, it just gets flagged.
#[derive(Debug, Clone, PartialEq)]
pub struct FrontmatterError {
    /// `serde_norway`'s own diagnostic text, verbatim — already human-readable, and
    /// for most errors already ends with its own "at line N column M", but that
    /// position is relative to the frontmatter block's own content (`line`/`column`
    /// below give the corrected position within the whole document instead).
    pub message: String,
    /// 1-indexed line within the *whole document* (not just the YAML block) that
    /// `serde_norway` pointed at, when it reported a position at all. Frontmatter's
    /// opening `---` always occupies exactly the document's first line (see
    /// `extract_block`), so this is always the block-relative line `serde_norway`
    /// reported, plus one.
    pub line: Option<usize>,
    /// 1-indexed column within that line — unaffected by the line correction above,
    /// since re-basing which line something's on doesn't change how far into that
    /// line it is.
    pub column: Option<usize>,
}

impl std::fmt::Display for FrontmatterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => write!(
                f,
                "Frontmatter YAML error at line {line}, column {column}: {}",
                self.message
            ),
            _ => write!(f, "Frontmatter YAML error: {}", self.message),
        }
    }
}

/// Check whether `contents`' leading frontmatter block, if any, fails to parse as
/// YAML — for surfacing a warning, since `parse` itself never errors (see its doc
/// comment) and would otherwise silently fall back to `DocumentMeta::default()`.
/// Returns `None` when there's no frontmatter block at all (nothing to validate),
/// the block is empty, or it parses successfully.
pub fn validate(contents: &str) -> Option<FrontmatterError> {
    let fm = extract_block(contents)?;
    if fm.yaml.trim().is_empty() {
        return None;
    }
    let err = serde_norway::from_str::<DocumentMeta>(fm.yaml).err()?;
    let location = err.location();
    Some(FrontmatterError {
        message: err.to_string(),
        line: location.as_ref().map(|loc| loc.line() + 1),
        column: location.as_ref().map(|loc| loc.column()),
    })
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

/// A document's word count for display in the Metadata panel: whitespace-separated
/// tokens in the body, after stripping frontmatter (so the YAML block's own keys/
/// values never inflate the count). Deliberately doesn't strip markdown syntax
/// (`#`, `**`, `[[...]]`, etc.) — those characters stay attached to the word they're
/// next to, so this is a token count of the raw text, the same simple definition
/// most plain-text word counters use, not a rendered-prose word count.
pub fn count_words(contents: &str) -> usize {
    strip(contents).split_whitespace().count()
}

/// A document's line count for display in the Binder (see
/// `Settings::show_document_stats_in_binder`): logical (`\n`-delimited) lines
/// in the body, after stripping frontmatter — same convention `count_words`
/// uses, and the same "logical, not wrapped" definition
/// `ui::editor_panel::paint_gutter`'s line numbers use. An empty body counts
/// as zero lines rather than one, unlike the gutter (which always shows at
/// least line 1 for the cursor to sit on) — there's no cursor here, just a
/// document that may have no body at all.
pub fn count_lines(contents: &str) -> usize {
    let body = strip(contents);
    if body.is_empty() {
        0
    } else {
        body.matches('\n').count() + 1
    }
}

/// A document's character count for display in the Binder (see
/// `Settings::show_document_stats_in_binder`): every character in the body,
/// after stripping frontmatter — same convention `count_words` uses.
pub fn count_chars(contents: &str) -> usize {
    strip(contents).chars().count()
}

/// Rewrite `contents`' leading frontmatter block — or add one, if it didn't have one
/// and `meta` isn't entirely empty — to reflect `meta`, leaving the body untouched.
///
/// `parse`/`DocumentMeta` silently discard any YAML keys they don't recognize (see
/// the struct's doc comment) — fine for reading, but a write-back can't afford to do
/// the same: editing just the "status" field through a metadata UI shouldn't destroy
/// a custom key a user (or another tool) added to the block by hand. So this parses
/// the existing block into a raw `Mapping` and only ever touches the five keys
/// `DocumentMeta` owns, leaving everything else in it exactly as found. A field set
/// to `None`/empty removes that key entirely rather than writing e.g. `status: null`.
pub fn write_back(contents: &str, meta: &DocumentMeta) -> String {
    let (existing_yaml, body) = match extract_block(contents) {
        Some(fm) => (fm.yaml, &contents[fm.body_start..]),
        None => ("", contents),
    };

    let mut mapping: serde_norway::Mapping = if existing_yaml.trim().is_empty() {
        serde_norway::Mapping::new()
    } else {
        serde_norway::from_str(existing_yaml).unwrap_or_default()
    };

    set_or_remove(&mut mapping, "type", meta.section_type.as_deref());
    set_or_remove(&mut mapping, "status", meta.status.as_deref());
    set_or_remove(&mut mapping, "pov", meta.pov.as_deref());
    match meta.word_count_target {
        Some(target) => {
            mapping.insert("word_count_target".into(), target.into());
        }
        None => {
            mapping.remove("word_count_target");
        }
    }
    if meta.tags.is_empty() {
        mapping.remove("tags");
    } else {
        mapping.insert("tags".into(), meta.tags.clone().into());
    }

    if mapping.is_empty() {
        return body.to_string();
    }
    let yaml = serde_norway::to_string(&mapping).unwrap_or_default();
    format!("---\n{yaml}---\n{body}")
}

/// Set `key` to `value` in `mapping`, or remove `key` entirely if `value` is `None`
/// or empty — an empty string in the UI means "unset this field", not "set it to the
/// empty string".
fn set_or_remove(mapping: &mut serde_norway::Mapping, key: &str, value: Option<&str>) {
    match value {
        Some(value) if !value.is_empty() => {
            mapping.insert(key.into(), value.into());
        }
        _ => {
            mapping.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_returns_default_when_no_frontmatter_present() {
        assert_eq!(
            parse("# Just a heading\n\nSome text."),
            DocumentMeta::default()
        );
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
    fn validate_is_none_when_there_is_no_frontmatter_block() {
        assert_eq!(validate("# Just a heading\n\nSome text."), None);
    }

    #[test]
    fn validate_is_none_when_the_block_is_empty() {
        assert_eq!(validate("---\n---\nBody.\n"), None);
    }

    #[test]
    fn validate_is_none_for_a_well_formed_block() {
        assert_eq!(validate("---\nstatus: draft\n---\nBody.\n"), None);
    }

    #[test]
    fn validate_reports_an_error_for_yaml_that_is_not_a_mapping() {
        let err = validate("---\n- a\n- b\n---\nBody.\n").unwrap();
        assert!(err.message.contains("invalid type"), "{err:?}");
    }

    #[test]
    fn validate_reports_a_document_relative_line_and_column() {
        // The bad key starts on the YAML block's own line 2 (serde_norway reports
        // that as-is), which is the whole document's line 3 (line 1 is the opening
        // "---", line 2 is "status: draft").
        let contents = "---\nstatus: draft\n  bad_indent: true\nfoo\n---\nBody.\n";
        let err = validate(contents).unwrap();
        assert_eq!(err.line, Some(3));
        assert!(err.column.is_some(), "{err:?}");
    }

    #[test]
    fn validate_error_display_includes_the_position_when_available() {
        let contents = "---\nstatus: draft\n  bad_indent: true\nfoo\n---\nBody.\n";
        let err = validate(contents).unwrap();
        let rendered = err.to_string();
        assert!(
            rendered.starts_with("Frontmatter YAML error at line 3, column"),
            "{rendered:?}"
        );
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
    fn parse_tolerates_trailing_whitespace_on_the_opening_delimiter_line() {
        assert_eq!(
            parse("--- \nstatus: draft\n---\nBody.\n").status,
            Some("draft".to_string())
        );
    }

    #[test]
    fn parse_tolerates_trailing_whitespace_on_the_closing_delimiter_line() {
        assert_eq!(
            parse("---\nstatus: draft\n--- \nBody.\n").status,
            Some("draft".to_string())
        );
    }

    #[test]
    fn strip_removes_a_block_whose_delimiters_have_trailing_whitespace() {
        let contents = "--- \ntype: Scene \n--- \n# Heading\n\nBody text.\n";
        assert_eq!(strip(contents), "# Heading\n\nBody text.\n");
    }

    #[test]
    fn count_words_excludes_a_frontmatter_block_whose_delimiters_have_trailing_whitespace() {
        let contents = "--- \ntype: Scene \n--- \nTwo words.\n";
        assert_eq!(count_words(contents), 2);
    }

    /// Regression test for a real "New From Template"-produced document whose
    /// template had trailing spaces on both `---` lines and indented YAML keys
    /// (`  type: Character `, `  created : 2026-07-29`) — before the trailing-
    /// whitespace tolerance fix, `extract_block` missed the delimiters entirely,
    /// so `type` never made it into `DocumentMeta` and the whole raw block
    /// (delimiters included) was counted as body text.
    #[test]
    fn parse_and_count_words_handle_a_real_indented_trailing_space_template_document() {
        let contents =
            "--- \n  type: Character \n  created : 2026-07-29\n--- \n\n# Crash test dummy";
        assert_eq!(parse(contents).section_type, Some("Character".to_string()));
        assert_eq!(count_words(contents), 4);
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

    #[test]
    fn count_words_counts_whitespace_separated_tokens_in_the_body() {
        assert_eq!(count_words("# Heading\n\nThree word body.\n"), 5);
    }

    #[test]
    fn count_words_excludes_the_frontmatter_block() {
        let contents = "---\ntype: Scene\nstatus: draft\n---\nTwo words.\n";
        assert_eq!(count_words(contents), 2);
    }

    #[test]
    fn count_words_is_zero_for_an_empty_document() {
        assert_eq!(count_words(""), 0);
    }

    #[test]
    fn count_words_is_zero_for_a_frontmatter_only_document_with_no_body() {
        assert_eq!(count_words("---\ntype: Scene\n---\n"), 0);
    }

    #[test]
    fn count_lines_counts_logical_lines_in_the_body() {
        assert_eq!(count_lines("One line.\n"), 2);
        assert_eq!(count_lines("Line one.\nLine two."), 2);
    }

    #[test]
    fn count_lines_excludes_the_frontmatter_block() {
        let contents = "---\ntype: Scene\n---\nOne line.\n";
        assert_eq!(count_lines(contents), 2);
    }

    #[test]
    fn count_lines_is_zero_for_an_empty_document() {
        assert_eq!(count_lines(""), 0);
        assert_eq!(count_lines("---\ntype: Scene\n---\n"), 0);
    }

    #[test]
    fn count_chars_counts_characters_in_the_body() {
        assert_eq!(count_chars("caf\u{e9}"), 4);
    }

    #[test]
    fn count_chars_excludes_the_frontmatter_block() {
        let contents = "---\ntype: Scene\n---\nHi";
        assert_eq!(count_chars(contents), 2);
    }

    #[test]
    fn write_back_adds_a_frontmatter_block_where_there_was_none() {
        let meta = DocumentMeta {
            status: Some("draft".to_string()),
            ..Default::default()
        };
        let result = write_back("# Heading\n\nBody.\n", &meta);
        assert_eq!(parse(&result), meta);
        assert_eq!(strip(&result), "# Heading\n\nBody.\n");
    }

    #[test]
    fn write_back_updates_an_existing_block_and_leaves_the_body_untouched() {
        let contents = "---\ntype: Scene\nstatus: draft\n---\n# Heading\n\nBody.\n";
        let meta = DocumentMeta {
            section_type: Some("Scene".to_string()),
            status: Some("final".to_string()),
            ..Default::default()
        };
        let result = write_back(contents, &meta);
        assert_eq!(parse(&result), meta);
        assert_eq!(strip(&result), "# Heading\n\nBody.\n");
    }

    #[test]
    fn write_back_preserves_unknown_keys_it_does_not_understand() {
        let contents = "---\ncustom_field: keep me\nstatus: draft\n---\nBody.\n";
        let meta = DocumentMeta {
            status: Some("final".to_string()),
            ..Default::default()
        };
        let result = write_back(contents, &meta);
        assert!(result.contains("custom_field: keep me"));
        assert_eq!(parse(&result).status, Some("final".to_string()));
    }

    #[test]
    fn write_back_removes_a_field_cleared_to_none_rather_than_writing_null() {
        let contents = "---\ntype: Scene\nstatus: draft\n---\nBody.\n";
        let meta = DocumentMeta {
            section_type: Some("Scene".to_string()),
            status: None,
            ..Default::default()
        };
        let result = write_back(contents, &meta);
        assert!(!result.contains("status"));
        assert_eq!(parse(&result).status, None);
    }

    #[test]
    fn write_back_removes_a_field_cleared_to_an_empty_string() {
        let contents = "---\npov: Alice\n---\nBody.\n";
        let meta = DocumentMeta {
            pov: Some(String::new()),
            ..Default::default()
        };
        let result = write_back(contents, &meta);
        assert!(!result.contains("pov"));
    }

    #[test]
    fn write_back_round_trips_tags() {
        let meta = DocumentMeta {
            tags: vec!["foo".to_string(), "bar".to_string()],
            ..Default::default()
        };
        let result = write_back("Body.\n", &meta);
        assert_eq!(parse(&result).tags, vec!["foo", "bar"]);
    }

    #[test]
    fn write_back_removes_the_whole_block_when_every_field_ends_up_empty() {
        let contents = "---\nstatus: draft\n---\nBody.\n";
        let result = write_back(contents, &DocumentMeta::default());
        assert_eq!(result, "Body.\n");
    }

    #[test]
    fn write_back_of_an_all_default_meta_with_no_existing_block_adds_nothing() {
        let result = write_back("Body.\n", &DocumentMeta::default());
        assert_eq!(result, "Body.\n");
    }

    #[test]
    fn write_back_round_trips_word_count_target() {
        let meta = DocumentMeta {
            word_count_target: Some(2500),
            ..Default::default()
        };
        let result = write_back("Body.\n", &meta);
        assert_eq!(parse(&result).word_count_target, Some(2500));
    }
}
