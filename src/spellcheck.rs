//! Hunspell-compatible spell-checking for the Editor (GitHub issue #47) — inline
//! underlines on words the selected dictionary doesn't recognize, rendered by
//! `ui::editor_panel::build_editor_layout_job` the same way `[[wikilink]]`s are
//! colored. Checking itself is via [`spellbook`](https://docs.rs/spellbook), a
//! pure-Rust Hunspell-format engine with no C/FFI/system dependency — chosen
//! partly because it's the crate Helix's own spell-check feature uses, fitting
//! this app's existing Helix-inspired conventions.
//!
//! **The bundled `assets/dictionaries/*.aff`/`*.dic` files are placeholders**,
//! not real word lists — see `assets/dictionaries/NOTICE` for why a real
//! English/Norwegian dictionary hasn't been bundled yet (an unresolved
//! licensing question, not a technical one). `Settings::spell_check_language`
//! defaults to `Off` accordingly: turning this on today flags nearly every real
//! word, since only a couple dozen are actually known per language.
//!
//! This module deliberately splits two independent halves: [`misspelled_word_spans`]
//! is pure text-tokenization logic taking an injected "is this word wrong"
//! predicate (so it's unit-testable without loading any real dictionary), and
//! [`is_misspelled`] is the thin, dictionary-backed wrapper `ui::editor_panel`
//! actually calls.

use std::ops::Range;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

/// Which bundled dictionary (if any) checks spelling in the Editor — see
/// `Settings::spell_check_language`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SpellCheckLanguage {
    #[default]
    Off,
    English,
    Norwegian,
}

impl SpellCheckLanguage {
    pub const ALL: [SpellCheckLanguage; 3] = [Self::Off, Self::English, Self::Norwegian];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::English => "English",
            Self::Norwegian => "Norwegian (Bokmål)",
        }
    }
}

fn load_bundled(aff: &[u8], dic: &[u8]) -> spellbook::Dictionary {
    let aff = std::str::from_utf8(aff).expect("bundled .aff is valid UTF-8");
    let dic = std::str::from_utf8(dic).expect("bundled .dic is valid UTF-8");
    spellbook::Dictionary::new(aff, dic).expect("bundled dictionary must parse")
}

static EN_US: LazyLock<spellbook::Dictionary> = LazyLock::new(|| {
    load_bundled(
        include_bytes!("../assets/dictionaries/en_US.aff"),
        include_bytes!("../assets/dictionaries/en_US.dic"),
    )
});

static NB_NO: LazyLock<spellbook::Dictionary> = LazyLock::new(|| {
    load_bundled(
        include_bytes!("../assets/dictionaries/nb_NO.aff"),
        include_bytes!("../assets/dictionaries/nb_NO.dic"),
    )
});

/// Whether `word` is *not* recognized by `language`'s dictionary — always
/// `false` for `Off`. Never pre-lowercases `word`: Hunspell `.aff` files encode
/// their own case rules (e.g. `KEEPCASE`), which a naive lowercase-first would
/// bypass incorrectly.
pub fn is_misspelled(word: &str, language: SpellCheckLanguage) -> bool {
    match language {
        SpellCheckLanguage::Off => false,
        SpellCheckLanguage::English => !EN_US.check(word),
        SpellCheckLanguage::Norwegian => !NB_NO.check(word),
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphabetic() || c == '\'' || c == '\u{2019}'
}

/// Trim a leading/trailing run of apostrophes from `line[start..end]`, so a
/// quoted word like `'hello'` checks as `hello`, not `'hello'` — but an
/// internal apostrophe (`don't`, `O'Brien`) stays part of the word. Returns an
/// empty range (`start == end`) if the whole span was apostrophes.
fn trim_apostrophes(line: &str, mut start: usize, mut end: usize) -> Range<usize> {
    while start < end {
        let c = line[start..].chars().next().expect("start < end");
        if c == '\'' || c == '\u{2019}' {
            start += c.len_utf8();
        } else {
            break;
        }
    }
    while end > start {
        let c = line[..end].chars().next_back().expect("end > start");
        if c == '\'' || c == '\u{2019}' {
            end -= c.len_utf8();
        } else {
            break;
        }
    }
    start..end
}

/// Every "word" token in `line` (a maximal run of `is_word_char`, apostrophes
/// trimmed from each end), paired with its byte range.
fn tokenize_line(line: &str) -> Vec<(Range<usize>, &str)> {
    let mut tokens = Vec::new();
    let mut chars = line.char_indices().peekable();
    while let Some(&(start, ch)) = chars.peek() {
        if !is_word_char(ch) {
            chars.next();
            continue;
        }
        let mut end = start + ch.len_utf8();
        chars.next();
        while let Some(&(i, c)) = chars.peek() {
            if !is_word_char(c) {
                break;
            }
            end = i + c.len_utf8();
            chars.next();
        }
        let range = trim_apostrophes(line, start, end);
        if !range.is_empty() {
            tokens.push((range.clone(), &line[range]));
        }
    }
    tokens
}

/// Byte ranges of every word in `text` that `is_misspelled` flags — the pure,
/// dictionary-agnostic core behind `ui::editor_panel`'s underline rendering.
/// Skips, in order: the leading frontmatter block (the raw editor buffer
/// includes it, unlike `ui::markdown_preview` which strips it first); fenced
/// code blocks and single-backtick inline code (`markdown::inline_code_ranges`/
/// `in_inline_code`, the same skip `markdown::wikilink_spans` itself uses);
/// and the entire span of any `[[wikilink]]` (bare or aliased) or `#tag` —
/// deliberately the *whole* span, not just its syntax markers, since a
/// wikilink target/tag is frequently a proper noun or invented word that would
/// otherwise be a guaranteed false positive. A known, accepted v1 limitation
/// this does *not* handle: a URL is tokenized word-by-word like any other
/// text and will typically flag its path segments as misspelled — there's no
/// URL-detection skip, on the reasoning that v1 already has no way to silence
/// any false positive, so one more source of them doesn't change the story.
pub fn misspelled_word_spans(
    text: &str,
    is_misspelled: impl Fn(&str) -> bool,
) -> Vec<Range<usize>> {
    let body = crate::frontmatter::strip(text);
    let body_start = text.len() - body.len();

    let wikilinks = crate::markdown::wikilink_spans(text);
    let tags = crate::markdown::inline_tag_spans(text);
    let in_skipped_span = |start: usize, end: usize| -> bool {
        wikilinks
            .iter()
            .any(|(range, _)| range.start <= start && end <= range.end)
            || tags
                .iter()
                .any(|(range, _)| range.start <= start && end <= range.end)
    };

    let mut spans = Vec::new();
    let mut in_fence = false;
    let mut line_start = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence {
            let code_ranges = crate::markdown::inline_code_ranges(line);
            for (range, word) in tokenize_line(line) {
                let abs_start = line_start + range.start;
                let abs_end = line_start + range.end;
                if abs_start < body_start
                    || crate::markdown::in_inline_code(&code_ranges, range.start)
                    || in_skipped_span(abs_start, abs_end)
                {
                    continue;
                }
                if is_misspelled(word) {
                    spans.push(abs_start..abs_end);
                }
            }
        }
        line_start += line.len();
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only_wrong(word: &str) -> impl Fn(&str) -> bool + '_ {
        move |w| w == word
    }

    #[test]
    fn finds_a_single_misspelled_word() {
        let spans = misspelled_word_spans("The wrold is big.", only_wrong("wrold"));
        assert_eq!(spans, vec![4..9]);
    }

    #[test]
    fn finds_multiple_misspellings_on_one_line_with_correct_ranges() {
        let text = "wrold and wrold again";
        let spans = misspelled_word_spans(text, only_wrong("wrold"));
        assert_eq!(spans, vec![0..5, 10..15]);
        for range in &spans {
            assert_eq!(&text[range.clone()], "wrold");
        }
    }

    #[test]
    fn a_word_inside_a_fenced_code_block_is_not_flagged() {
        let spans = misspelled_word_spans("```\nwrold\n```\n", only_wrong("wrold"));
        assert!(spans.is_empty());
    }

    #[test]
    fn a_word_inside_inline_code_is_not_flagged() {
        let spans = misspelled_word_spans("See `wrold` here.", only_wrong("wrold"));
        assert!(spans.is_empty());
    }

    #[test]
    fn a_word_inside_the_frontmatter_block_is_not_flagged() {
        let spans =
            misspelled_word_spans("---\nstatus: wrold\n---\nBody text.", only_wrong("wrold"));
        assert!(spans.is_empty());
    }

    #[test]
    fn a_word_inside_a_bare_wikilink_is_not_flagged() {
        let spans = misspelled_word_spans("See [[Wrold]] for more.", only_wrong("Wrold"));
        assert!(spans.is_empty());
    }

    #[test]
    fn a_word_inside_an_aliased_wikilink_is_not_flagged() {
        let spans = misspelled_word_spans("See [[Target|Wrold]] for more.", only_wrong("Wrold"));
        assert!(spans.is_empty());
    }

    #[test]
    fn a_word_inside_a_tag_is_not_flagged() {
        let spans = misspelled_word_spans("Filed under #wrold today.", only_wrong("wrold"));
        assert!(spans.is_empty());
    }

    #[test]
    fn an_apostrophe_word_is_tokenized_as_one_unit() {
        let spans = misspelled_word_spans("don't", only_wrong("wrold"));
        assert!(spans.is_empty(), "sanity: don't isn't the misspelled word");
        let spans = misspelled_word_spans("don't", only_wrong("don't"));
        assert_eq!(spans, vec![0..5]);
    }

    #[test]
    fn a_quoted_word_is_checked_without_its_surrounding_quotes() {
        let text = "'wrold'";
        let spans = misspelled_word_spans(text, only_wrong("wrold"));
        assert_eq!(spans, vec![1..6]);
        assert_eq!(&text[1..6], "wrold");
    }

    #[test]
    fn empty_text_produces_no_spans() {
        assert!(misspelled_word_spans("", only_wrong("wrold")).is_empty());
    }

    #[test]
    fn whitespace_only_text_produces_no_spans() {
        assert!(misspelled_word_spans("   \n  \n", only_wrong("wrold")).is_empty());
    }

    #[test]
    fn is_misspelled_is_always_false_when_off() {
        assert!(!is_misspelled(
            "thisIsDefinitelyNotAWord",
            SpellCheckLanguage::Off
        ));
    }

    #[test]
    fn placeholder_english_dictionary_recognizes_its_own_bundled_words() {
        assert!(!is_misspelled("hello", SpellCheckLanguage::English));
        assert!(is_misspelled("wrold", SpellCheckLanguage::English));
    }

    #[test]
    fn placeholder_norwegian_dictionary_recognizes_its_own_bundled_words() {
        assert!(!is_misspelled("hei", SpellCheckLanguage::Norwegian));
        assert!(is_misspelled("wrold", SpellCheckLanguage::Norwegian));
    }

    #[test]
    fn settings_default_is_off() {
        assert_eq!(SpellCheckLanguage::default(), SpellCheckLanguage::Off);
    }
}
