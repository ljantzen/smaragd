//! Pure logic behind Obsidian-style `[[wikilink]]` autocomplete: detecting an
//! in-progress query at the cursor, filtering candidates against it, and splicing in
//! the chosen name. Kept free of any egui dependency so it's unit-testable without a
//! GUI context — the `ui` layer only drives these functions and renders the popup.
//! `filter_candidates` is generic enough to also back the `:` command prompt's
//! autocomplete (`ui/command_prompt.rs`), not just wikilinks.

/// The in-progress `[[query` at the cursor, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct WikilinkQuery {
    /// Byte offset into the buffer right after the opening `[[`, where the typed
    /// query (and eventually the chosen note name) begins.
    pub query_start: usize,
    pub query: String,
}

/// If `cursor` (a byte offset into `text`) sits inside an unclosed `[[...` on its
/// current line, return the query typed so far. Wikilinks don't span lines, and a
/// stray `]` or `[` in the query means it's no longer an open, in-progress link.
pub fn active_wikilink_query(text: &str, cursor: usize) -> Option<WikilinkQuery> {
    let line_start = text[..cursor].rfind('\n').map_or(0, |i| i + 1);
    let line_before_cursor = &text[line_start..cursor];
    let open = line_before_cursor.rfind("[[")?;
    let query_start = line_start + open + 2;
    let query = &text[query_start..cursor];
    if query.contains(['[', ']']) {
        return None;
    }
    Some(WikilinkQuery {
        query_start,
        query: query.to_string(),
    })
}

/// Filter `candidates` against `query`, case-insensitively: candidates starting with
/// `query` are ranked first, then candidates merely containing it elsewhere, each
/// group sorted alphabetically. An empty query matches everything. Generic over
/// `AsRef<str>` so it works equally for `&[String]` (note titles) and `&[&str]`
/// (e.g. a fixed list of command names).
pub fn filter_candidates<'a, S: AsRef<str>>(candidates: &'a [S], query: &str) -> Vec<&'a str> {
    let query = query.to_lowercase();
    let mut starts_with = Vec::new();
    let mut contains = Vec::new();
    for candidate in candidates {
        let candidate = candidate.as_ref();
        let lower = candidate.to_lowercase();
        if lower.starts_with(&query) {
            starts_with.push(candidate);
        } else if lower.contains(&query) {
            contains.push(candidate);
        }
    }
    starts_with.sort_unstable();
    contains.sort_unstable();
    starts_with.extend(contains);
    starts_with
}

/// Replace the query `text[query_start..cursor]` with `chosen`, followed by a single
/// closing `]]` — absorbing one already present right after the cursor so accepting a
/// suggestion never produces doubled brackets. Returns the new buffer and the byte
/// offset just after the `]]`, where the cursor should land.
pub fn apply_wikilink_completion(
    text: &str,
    query_start: usize,
    cursor: usize,
    chosen: &str,
) -> (String, usize) {
    let tail_start = if text[cursor..].starts_with("]]") {
        cursor + 2
    } else {
        cursor
    };
    let mut result = String::with_capacity(text.len() + chosen.len() + 2);
    result.push_str(&text[..query_start]);
    result.push_str(chosen);
    result.push_str("]]");
    let new_cursor = result.len();
    result.push_str(&text[tail_start..]);
    (result, new_cursor)
}

/// Convert a character offset (as used by egui's text cursor) to a byte offset.
pub fn char_offset_to_byte(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map_or(text.len(), |(byte_offset, _)| byte_offset)
}

/// Convert a byte offset back to a character offset — the inverse of
/// [`char_offset_to_byte`], for repositioning egui's cursor after an edit.
///
/// `byte_offset` is clamped to the nearest valid char boundary at or before it,
/// rather than trusted outright: a caller may be working from an offset computed
/// against a since-changed version of `text` (e.g. a `find_replace` result clicked
/// after the buffer was edited further) — walking it back to something safe to slice
/// on is a far better outcome than panicking on an out-of-bounds or mid-character
/// index.
pub fn byte_offset_to_char(text: &str, byte_offset: usize) -> usize {
    let mut boundary = byte_offset.min(text.len());
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text[..boundary].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_wikilink_query_finds_in_progress_query() {
        let text = "See [[Top";
        let found = active_wikilink_query(text, text.len()).unwrap();
        assert_eq!(found.query, "Top");
        assert_eq!(found.query_start, 6);
    }

    #[test]
    fn active_wikilink_query_is_empty_right_after_double_bracket() {
        let text = "[[";
        let found = active_wikilink_query(text, text.len()).unwrap();
        assert_eq!(found.query, "");
        assert_eq!(found.query_start, 2);
    }

    #[test]
    fn active_wikilink_query_is_none_once_closed() {
        let text = "See [[Topic]] and more";
        assert!(active_wikilink_query(text, text.len()).is_none());
    }

    #[test]
    fn active_wikilink_query_does_not_cross_lines() {
        let text = "[[Topic\nSee";
        assert!(active_wikilink_query(text, text.len()).is_none());
    }

    #[test]
    fn active_wikilink_query_is_none_without_double_bracket() {
        assert!(active_wikilink_query("just text", 9).is_none());
    }

    #[test]
    fn active_wikilink_query_uses_the_last_double_bracket_on_the_line() {
        let text = "[[A]] and [[B";
        let found = active_wikilink_query(text, text.len()).unwrap();
        assert_eq!(found.query, "B");
        assert_eq!(found.query_start, 12);
    }

    #[test]
    fn filter_candidates_ranks_prefix_matches_before_substring_matches() {
        let candidates = vec![
            "Backstory".to_string(),
            "Opening".to_string(),
            "The Opening Scene".to_string(),
        ];
        assert_eq!(
            filter_candidates(&candidates, "open"),
            vec!["Opening", "The Opening Scene"]
        );
    }

    #[test]
    fn filter_candidates_is_case_insensitive() {
        let candidates = vec!["Opening Scene".to_string()];
        assert_eq!(
            filter_candidates(&candidates, "OPENING"),
            vec!["Opening Scene"]
        );
    }

    #[test]
    fn filter_candidates_empty_query_returns_everything_sorted() {
        let candidates = vec!["Zeta".to_string(), "Alpha".to_string()];
        assert_eq!(filter_candidates(&candidates, ""), vec!["Alpha", "Zeta"]);
    }

    #[test]
    fn apply_wikilink_completion_inserts_closing_brackets() {
        let (text, cursor) = apply_wikilink_completion("See [[Top", 6, 9, "Topic");
        assert_eq!(text, "See [[Topic]]");
        assert_eq!(cursor, 13);
        assert_eq!(&text[cursor..], "");
    }

    #[test]
    fn apply_wikilink_completion_absorbs_an_existing_closing_bracket() {
        let (text, cursor) = apply_wikilink_completion("See [[Top]] please", 6, 9, "Topic");
        assert_eq!(text, "See [[Topic]] please");
        assert_eq!(cursor, 13);
    }

    #[test]
    fn apply_wikilink_completion_is_utf8_safe() {
        let input = "caf\u{e9} [[T";
        let query_start = input.find("[[").unwrap() + 2;
        let cursor = input.len();

        let (text, new_cursor) = apply_wikilink_completion(input, query_start, cursor, "T\u{e9}ma");

        assert_eq!(text, "caf\u{e9} [[T\u{e9}ma]]");
        assert_eq!(new_cursor, text.len());
    }

    #[test]
    fn char_and_byte_offsets_round_trip_with_multibyte_text() {
        let text = "caf\u{e9} [[world";
        let char_offset = text.chars().count();
        let byte_offset = char_offset_to_byte(text, char_offset);
        assert_eq!(byte_offset, text.len());
        assert_eq!(byte_offset_to_char(text, byte_offset), char_offset);

        // A char offset in the middle, right after the accented character.
        let mid_char_offset = 4; // "caf\u{e9}".chars().count()
        let mid_byte_offset = char_offset_to_byte(text, mid_char_offset);
        assert_eq!(&text[..mid_byte_offset], "caf\u{e9}");
        assert_eq!(byte_offset_to_char(text, mid_byte_offset), mid_char_offset);
    }

    #[test]
    fn byte_offset_to_char_clamps_an_out_of_bounds_offset_instead_of_panicking() {
        let text = "short";
        assert_eq!(byte_offset_to_char(text, 1000), text.chars().count());
    }

    #[test]
    fn byte_offset_to_char_walks_back_a_mid_character_offset_instead_of_panicking() {
        // "é" is a 2-byte character starting at byte 0; byte 1 falls inside it.
        let text = "\u{e9}bc";
        assert_eq!(byte_offset_to_char(text, 1), 0);
    }

    #[test]
    fn byte_offset_to_char_handles_an_empty_string() {
        assert_eq!(byte_offset_to_char("", 5), 0);
    }
}
