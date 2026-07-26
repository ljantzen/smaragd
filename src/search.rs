//! Plain-text find/replace across one or more documents. Pure string manipulation —
//! `app.rs` owns deciding which files are in scope and whether a match should be
//! written to disk or to the live editor buffer.

use std::fs;
use std::path::{Path, PathBuf};

/// Which set of documents a search or replace runs over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchScope {
    #[default]
    CurrentFile,
    CurrentDirectory,
    ModifiedFiles,
    AllFiles,
}

impl SearchScope {
    pub const ALL: [SearchScope; 4] = [
        Self::CurrentFile,
        Self::CurrentDirectory,
        Self::ModifiedFiles,
        Self::AllFiles,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::CurrentFile => "Current File",
            Self::CurrentDirectory => "Current Directory",
            Self::ModifiedFiles => "Modified Files",
            Self::AllFiles => "All Files",
        }
    }
}

/// A single match, ready to display in a results list and, if the user clicks it,
/// jump the editor cursor to `byte_start`.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchMatch {
    pub path: PathBuf,
    /// 1-based line number, for display.
    pub line: usize,
    /// The full text of the line the match is on, for a results-list preview.
    pub line_text: String,
    pub byte_start: usize,
    pub byte_end: usize,
}

/// Byte ranges of every non-overlapping occurrence of `query` in `text`, scanning
/// left to right (so a match immediately follows the end of the previous one rather
/// than overlapping it, matching how "replace all" is expected to behave). An empty
/// `query` matches nothing.
///
/// Case-insensitive comparison folds per-character (via `char::to_lowercase`) rather
/// than lowercasing the whole haystack up front, since lowercasing can change a
/// string's byte length for some Unicode characters and would throw off the byte
/// offsets we report.
pub fn find_matches(text: &str, query: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }

    let query_chars: Vec<char> = query.chars().collect();
    let text_chars: Vec<(usize, char)> = text.char_indices().collect();

    let mut matches = Vec::new();
    let mut i = 0;
    while i + query_chars.len() <= text_chars.len() {
        let is_match = text_chars[i..i + query_chars.len()]
            .iter()
            .zip(&query_chars)
            .all(|(&(_, tc), &qc)| chars_eq(tc, qc, case_sensitive));

        if is_match {
            let start = text_chars[i].0;
            let end = text_chars
                .get(i + query_chars.len())
                .map(|&(idx, _)| idx)
                .unwrap_or(text.len());
            matches.push((start, end));
            i += query_chars.len();
        } else {
            i += 1;
        }
    }
    matches
}

fn chars_eq(a: char, b: char, case_sensitive: bool) -> bool {
    if case_sensitive {
        a == b
    } else {
        a.to_lowercase().eq(b.to_lowercase())
    }
}

/// Replace every occurrence of `query` in `text` with `replacement`, returning the
/// new text and how many replacements were made.
pub fn replace_all(
    text: &str,
    query: &str,
    replacement: &str,
    case_sensitive: bool,
) -> (String, usize) {
    let matches = find_matches(text, query, case_sensitive);
    if matches.is_empty() {
        return (text.to_string(), 0);
    }

    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;
    for &(start, end) in &matches {
        result.push_str(&text[last_end..start]);
        result.push_str(replacement);
        last_end = end;
    }
    result.push_str(&text[last_end..]);
    (result, matches.len())
}

/// The 1-based line number and full line text containing `byte_offset`.
fn line_at(text: &str, byte_offset: usize) -> (usize, String) {
    let line_start = text[..byte_offset].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[byte_offset..]
        .find('\n')
        .map_or(text.len(), |i| byte_offset + i);
    let line_number = text[..line_start].matches('\n').count() + 1;
    (line_number, text[line_start..line_end].to_string())
}

/// Search every file in `paths` for `query`. `live_override`, if given, is
/// `(path, content)` for a document whose on-disk content is stale relative to an
/// in-memory buffer (the currently open, possibly-unsaved document) — that content is
/// used instead of reading the file. Files that can't be read (e.g. removed since the
/// scope was computed) are silently skipped.
pub fn search_paths(
    paths: &[PathBuf],
    query: &str,
    case_sensitive: bool,
    live_override: Option<(&Path, &str)>,
) -> Vec<SearchMatch> {
    let mut results = Vec::new();
    for path in paths {
        let content = match live_override {
            Some((live_path, live_text)) if live_path == path.as_path() => {
                Some(live_text.to_string())
            }
            _ => fs::read_to_string(path).ok(),
        };
        let Some(content) = content else { continue };

        for (byte_start, byte_end) in find_matches(&content, query, case_sensitive) {
            let (line, line_text) = line_at(&content, byte_start);
            results.push(SearchMatch {
                path: path.clone(),
                line,
                line_text,
                byte_start,
                byte_end,
            });
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_matches_finds_every_non_overlapping_occurrence() {
        let matches = find_matches("abcabcabc", "abc", true);
        assert_eq!(matches, vec![(0, 3), (3, 6), (6, 9)]);
    }

    #[test]
    fn find_matches_does_not_overlap_matches() {
        // "aaa" against "aa": a naive scan could find (0,2) and (1,3); we want the
        // second search to resume after the first match ends, at index 2.
        let matches = find_matches("aaa", "aa", true);
        assert_eq!(matches, vec![(0, 2)]);
    }

    #[test]
    fn find_matches_empty_query_matches_nothing() {
        assert!(find_matches("hello", "", true).is_empty());
    }

    #[test]
    fn find_matches_case_sensitive_is_exact() {
        assert!(find_matches("Hello World", "hello", true).is_empty());
    }

    #[test]
    fn find_matches_case_insensitive_ignores_case() {
        let matches = find_matches("Hello World", "hello", false);
        assert_eq!(matches, vec![(0, 5)]);
    }

    #[test]
    fn find_matches_reports_correct_byte_offsets_with_multibyte_text() {
        // "café " is 5 chars but 6 bytes ('é' is 2 bytes); the match starts after it.
        let matches = find_matches("café world", "world", true);
        assert_eq!(matches, vec![(6, 11)]);
        assert_eq!(&"café world"[6..11], "world");
    }

    #[test]
    fn replace_all_replaces_every_occurrence_and_counts_them() {
        let (text, count) = replace_all("foo bar foo", "foo", "baz", true);
        assert_eq!(text, "baz bar baz");
        assert_eq!(count, 2);
    }

    #[test]
    fn replace_all_with_no_matches_returns_original_text_unchanged() {
        let (text, count) = replace_all("hello world", "xyz", "abc", true);
        assert_eq!(text, "hello world");
        assert_eq!(count, 0);
    }

    #[test]
    fn replace_all_is_case_insensitive_when_requested() {
        let (text, count) = replace_all("Foo foo FOO", "foo", "bar", false);
        assert_eq!(text, "bar bar bar");
        assert_eq!(count, 3);
    }

    #[test]
    fn search_paths_finds_matches_across_multiple_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        fs::write(&a, "one needle here").unwrap();
        fs::write(&b, "two matches: needle and needle again").unwrap();

        let results = search_paths(&[a.clone(), b.clone()], "needle", true, None);

        assert_eq!(results.iter().filter(|m| m.path == a).count(), 1);
        assert_eq!(results.iter().filter(|m| m.path == b).count(), 2);
    }

    #[test]
    fn search_paths_uses_live_override_instead_of_disk_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "saved content").unwrap();

        let results = search_paths(
            std::slice::from_ref(&path),
            "unsaved",
            true,
            Some((&path, "unsaved content")),
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_text, "unsaved content");
    }

    #[test]
    fn search_paths_reports_line_number_and_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "line one\nline two has needle\nline three").unwrap();

        let results = search_paths(&[path], "needle", true, None);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line, 2);
        assert_eq!(results[0].line_text, "line two has needle");
    }

    #[test]
    fn search_paths_skips_unreadable_files() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.md");

        let results = search_paths(&[missing], "needle", true, None);

        assert!(results.is_empty());
    }
}
