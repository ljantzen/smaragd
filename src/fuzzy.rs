//! fzf-style fuzzy matching, distinct from `autocomplete::filter_candidates`'s plain
//! prefix/substring check — subsequence matching with real scoring (via
//! `nucleo-matcher`, the engine behind the Helix editor's picker), so a query like
//! "ch1sc2" matches "Chapter 1/Scene 2".

use std::path::PathBuf;

use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

/// Fuzzy-filter `candidates` (display string, path) against `query`. An empty query
/// returns every candidate in its original order. Results are capped at `limit` and
/// ordered best-match-first.
pub fn fuzzy_match_documents<'a>(
    candidates: &'a [(String, PathBuf)],
    query: &str,
    limit: usize,
) -> Vec<&'a (String, PathBuf)> {
    if query.is_empty() {
        return candidates.iter().take(limit).collect();
    }
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    // A single literal `Atom` wrapped directly in a `Pattern`, rather than
    // `Pattern::new`/`Pattern::parse` (both of which — despite `new`'s doc comment
    // suggesting otherwise — internally split the query on whitespace into
    // independent atoms via `pattern_atoms`, each free to match *anywhere* in the
    // candidate on its own). That splitting is why a query like "Scene 01" was
    // matching a document whose path only had "Scene" and "01" in entirely
    // unrelated places (e.g. a different chapter number elsewhere in the path) —
    // confirmed directly against `Matcher::fuzzy_match`, which correctly rejects
    // that candidate for the single, space-included needle "Scene 01" bypassing
    // `Pattern::new`'s splitting.  Treating the whole query as one ordered
    // subsequence (space included) is also the standard "quick open a file"
    // convention (what VSCode's Ctrl+P or Sublime's Goto Anything do).
    let mut pattern = Pattern::new(
        "",
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    pattern.atoms = vec![Atom::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
        false,
    )];
    let mut scored: Vec<(&str, u32)> = pattern.match_list(
        candidates.iter().map(|(name, _)| name.as_str()),
        &mut matcher,
    );
    scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
    scored
        .into_iter()
        .take(limit)
        .filter_map(|(name, _)| candidates.iter().find(|(n, _)| n == name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Vec<(String, PathBuf)> {
        vec![
            ("Manus/Chapter 1/Scene 1".to_string(), PathBuf::from("c1s1")),
            ("Manus/Chapter 1/Scene 2".to_string(), PathBuf::from("c1s2")),
            ("Manus/Chapter 2/Scene 1".to_string(), PathBuf::from("c2s1")),
            ("Research/Notes".to_string(), PathBuf::from("notes")),
        ]
    }

    #[test]
    fn empty_query_returns_everything_in_original_order() {
        let candidates = candidates();
        let results = fuzzy_match_documents(&candidates, "", 10);
        let names: Vec<&str> = results.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Manus/Chapter 1/Scene 1",
                "Manus/Chapter 1/Scene 2",
                "Manus/Chapter 2/Scene 1",
                "Research/Notes",
            ]
        );
    }

    #[test]
    fn exact_substring_matches() {
        let candidates = candidates();
        let results = fuzzy_match_documents(&candidates, "Notes", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "Research/Notes");
    }

    #[test]
    fn a_query_with_a_number_does_not_match_an_unrelated_document_sharing_that_number() {
        // Regression test for a reported bug: "Scene 01" (a space-separated query)
        // was matching "Part 01/Chapter 03/Scene 02" too, because `Pattern::parse`
        // treats "Scene" and "01" as independent terms, each free to match anywhere
        // in the candidate — "Scene" found near the end, "01" found earlier in
        // "Part 01", with no requirement that they relate to each other at all.
        let candidates = vec![
            (
                "Part 01/Chapter 01/Scene 01".to_string(),
                PathBuf::from("right"),
            ),
            (
                "Part 01/Chapter 03/Scene 02".to_string(),
                PathBuf::from("wrong"),
            ),
        ];
        let results = fuzzy_match_documents(&candidates, "Scene 01", 10);
        let names: Vec<&str> = results.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["Part 01/Chapter 01/Scene 01"]);
    }

    #[test]
    fn subsequence_query_matches_out_of_order_fragments() {
        // The whole point of fzf-style matching over `filter_candidates`'s plain
        // substring check: a query whose characters appear in order but not
        // contiguously should still match.
        let candidates = candidates();
        let results = fuzzy_match_documents(&candidates, "ch1sc2", 10);
        let names: Vec<&str> = results.iter().map(|(name, _)| name.as_str()).collect();
        assert!(
            names.contains(&"Manus/Chapter 1/Scene 2"),
            "expected a subsequence match for \"ch1sc2\", got {names:?}"
        );
    }

    #[test]
    fn ranks_a_contiguous_match_above_a_scattered_one() {
        // A single-word query (no space) so `Pattern::parse` treats it as one atom
        // rather than splitting on whitespace into independent AND-matched terms.
        let candidates = vec![
            (
                "Scattered: s-c-e-n-e1 spread out".to_string(),
                PathBuf::from("a"),
            ),
            ("Scene1".to_string(), PathBuf::from("b")),
        ];
        let results = fuzzy_match_documents(&candidates, "scene1", 10);
        assert_eq!(results[0].0, "Scene1");
    }

    #[test]
    fn no_match_returns_empty() {
        let candidates = candidates();
        let results = fuzzy_match_documents(&candidates, "zzzznotpresent", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn respects_the_limit() {
        let candidates = candidates();
        let results = fuzzy_match_documents(&candidates, "", 2);
        assert_eq!(results.len(), 2);
    }
}
