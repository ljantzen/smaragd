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
//! English/Norwegian dictionary isn't bundled directly into the binary (an
//! unresolved licensing question, not a technical one). Instead, real
//! dictionaries can be fetched at runtime, on request, into the platform data
//! directory (`app::dictionary_download`, `ui::settings_panel`'s "Dictionaries"
//! list) from `catalog()` — sourced from `assets/dictionaries/catalog.json`,
//! itself bundled into the binary as plain tracking metadata (source, license,
//! attribution, expected SHA-256 — no copyrighted word-list content). Only a
//! catalog entry whose `review_status` doesn't start with `"blocked"` is ever
//! offered for download; `download_dictionary` also independently re-verifies
//! every downloaded file's SHA-256 against the catalog before keeping it, so a
//! tampered mirror or a stale catalog entry can't silently swap in different
//! content. `Settings::spell_check_language` defaults to `Off`, and checking
//! against a language nobody's downloaded yet quietly falls back to the tiny
//! placeholder list — flagging nearly every real word, but never panicking.
//!
//! This module deliberately splits two independent halves: [`misspelled_word_spans`]
//! is pure text-tokenization logic taking an injected "is this word wrong"
//! predicate (so it's unit-testable without loading any real dictionary), and
//! [`is_misspelled`] is the thin, dictionary-backed wrapper `ui::editor_panel`
//! actually calls.

use std::collections::HashMap;
use std::io::Read;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, RwLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Which bundled dictionary (if any) checks spelling in the Editor — see
/// `Settings::spell_check_language`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
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

    /// This language's `language_code` in `catalog()` — `None` for `Off`, which
    /// has no dictionary of its own.
    fn code(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::English => Some("en_US"),
            Self::Norwegian => Some("nb_NO"),
        }
    }

    /// The reverse of `code` — which language (if any) a catalog entry's
    /// `language_code` names. Used by `ui::settings_panel` to turn a catalog
    /// entry into a `SpellCheckLanguage` it can act on.
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|lang| lang.code() == Some(code))
    }

    /// This language's entry in `catalog()`, if any.
    pub fn catalog_entry(self) -> Option<&'static CatalogEntry> {
        let code = self.code()?;
        catalog().iter().find(|entry| entry.language_code == code)
    }
}

/// One dictionary tracked in `assets/dictionaries/catalog.json` — see that
/// file's own `notes` field, and `assets/dictionaries/NOTICE`, for the full
/// story. Every field here is metadata (source, license, expected hashes),
/// never the copyrighted word-list content itself.
#[derive(Debug, Clone, Deserialize)]
pub struct CatalogEntry {
    pub language: String,
    pub language_code: String,
    pub files: Vec<String>,
    pub version: Option<String>,
    pub source: String,
    pub copyright: String,
    pub license: String,
    pub license_url: String,
    pub attribution: String,
    pub redistributable: bool,
    pub review_status: String,
    #[serde(default)]
    pub review_status_notes: String,
    pub review_date: String,
    pub sha256: HashMap<String, String>,
    pub download_urls: HashMap<String, String>,
}

impl CatalogEntry {
    /// Whether this entry is withheld from the in-app downloader entirely,
    /// regardless of its own `redistributable` value — see `download_dictionary`.
    pub fn is_blocked(&self) -> bool {
        self.review_status.starts_with("blocked")
    }
}

#[derive(Deserialize)]
struct Catalog {
    dictionaries: Vec<CatalogEntry>,
}

static CATALOG: LazyLock<Vec<CatalogEntry>> = LazyLock::new(|| {
    let catalog: Catalog =
        serde_json::from_str(include_str!("../assets/dictionaries/catalog.json"))
            .expect("bundled catalog.json must parse");
    catalog.dictionaries
});

/// Every dictionary this build knows about, downloadable or not — see
/// `CatalogEntry`/`assets/dictionaries/catalog.json`.
pub fn catalog() -> &'static [CatalogEntry] {
    &CATALOG
}

fn load_bundled(aff: &[u8], dic: &[u8]) -> spellbook::Dictionary {
    let aff = std::str::from_utf8(aff).expect("bundled .aff is valid UTF-8");
    let dic = std::str::from_utf8(dic).expect("bundled .dic is valid UTF-8");
    spellbook::Dictionary::new(aff, dic).expect("bundled dictionary must parse")
}

fn load_placeholder(language: SpellCheckLanguage) -> spellbook::Dictionary {
    match language {
        SpellCheckLanguage::Off => unreachable!("Off has no dictionary to load"),
        SpellCheckLanguage::English => load_bundled(
            include_bytes!("../assets/dictionaries/en_US.aff"),
            include_bytes!("../assets/dictionaries/en_US.dic"),
        ),
        SpellCheckLanguage::Norwegian => load_bundled(
            include_bytes!("../assets/dictionaries/nb_NO.aff"),
            include_bytes!("../assets/dictionaries/nb_NO.dic"),
        ),
    }
}

/// Where downloaded dictionaries live on disk: `<platform data dir>/dictionaries/
/// <language_code>/<filename>` — a sibling of `backup::backups_dir`'s own
/// `<data dir>/backups`, same `directories::ProjectDirs::from("", "", "smaragd")`
/// convention. `None` only when the platform has no meaningful data directory
/// (`directories` couldn't resolve one), in which case downloading is simply
/// unavailable and every language falls back to its placeholder.
fn dictionaries_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "smaragd")
        .map(|dirs| dirs.data_dir().join("dictionaries"))
}

fn downloaded_file_path(language_code: &str, filename: &str) -> Option<PathBuf> {
    dictionaries_dir().map(|dir| dir.join(language_code).join(filename))
}

/// Whether every file `language`'s catalog entry lists has already been
/// downloaded (see `download_dictionary`) — read by `ui::settings_panel` to
/// show "Downloaded" instead of a "Download" button. `false` for `Off` and for
/// any language with no catalog entry.
pub fn is_downloaded(language: SpellCheckLanguage) -> bool {
    let Some(entry) = language.catalog_entry() else {
        return false;
    };
    entry.files.iter().all(|file| {
        downloaded_file_path(&entry.language_code, file).is_some_and(|path| path.is_file())
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url)
        .call()
        .map_err(|err| format!("request failed: {err}"))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .map_err(|err| format!("reading response body: {err}"))?;
    Ok(bytes)
}

/// Download every file in `language`'s catalog entry, verify each one's SHA-256
/// against what the catalog recorded, and save them to `dictionaries_dir()` —
/// call `invalidate_dictionary_cache(language)` afterward so the next spell
/// check actually picks up the new file instead of whatever was already
/// cached in memory. Blocking (real network I/O): always call this from a
/// background thread, never the UI thread — see `app::dictionary_download`.
///
/// Refuses a language with no catalog entry, and refuses (rather than silently
/// skipping the safety net) any entry `is_blocked()` — the in-app downloader
/// must never be the thing that smuggles in a dictionary this repo's own
/// review flagged as license-incompatible. A file whose downloaded bytes don't
/// match the catalog's recorded SHA-256 is treated as corrupted or tampered
/// with and rejected outright, not saved under a warning.
pub fn download_dictionary(language: SpellCheckLanguage) -> Result<(), String> {
    let entry = language
        .catalog_entry()
        .ok_or_else(|| "No catalog entry for this language".to_string())?;
    if entry.is_blocked() {
        return Err(format!(
            "{} is not offered for download (review status: {})",
            entry.language, entry.review_status
        ));
    }
    let dir = dictionaries_dir()
        .ok_or_else(|| "Couldn't determine a data directory for this platform".to_string())?
        .join(&entry.language_code);
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("Couldn't create {}: {err}", dir.display()))?;

    for file in &entry.files {
        let url = entry
            .download_urls
            .get(file)
            .ok_or_else(|| format!("No download URL recorded for {file}"))?;
        let expected_sha256 = entry
            .sha256
            .get(file)
            .ok_or_else(|| format!("No SHA-256 recorded for {file}"))?;

        let bytes = fetch(url).map_err(|err| format!("Downloading {file}: {err}"))?;
        let actual_sha256 = sha256_hex(&bytes);
        if &actual_sha256 != expected_sha256 {
            return Err(format!(
                "{file} failed its integrity check (expected sha256 {expected_sha256}, got \
                 {actual_sha256}) — not saving a file that doesn't match the catalog"
            ));
        }

        // Write to a temp file and rename into place, so a crash or interrupted
        // download mid-write can never leave a partial file that `is_downloaded`
        // would then treat as complete.
        let dest = dir.join(file);
        let tmp = dest.with_extension("part");
        std::fs::write(&tmp, &bytes).map_err(|err| format!("Writing {file}: {err}"))?;
        std::fs::rename(&tmp, &dest).map_err(|err| format!("Saving {file}: {err}"))?;
    }
    Ok(())
}

/// In-memory memo of each language's loaded `Dictionary` — `Arc` so
/// `is_misspelled`/`misspelled_word_spans` callers don't hold a lock while
/// running (potentially many) `.check()` calls. Lazily populated on first use
/// per language, same spirit as `project::queries::TagCache`: cheap to keep
/// around once loaded (parsing a real dictionary isn't free — spellbook's own
/// benchmarks put it at ~100ms for an English-sized word list), invalidated
/// explicitly rather than on a timer.
static LOADED: LazyLock<RwLock<HashMap<SpellCheckLanguage, Arc<spellbook::Dictionary>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn dictionary_for(language: SpellCheckLanguage) -> Option<Arc<spellbook::Dictionary>> {
    if language == SpellCheckLanguage::Off {
        return None;
    }
    if let Some(dict) = LOADED.read().expect("not poisoned").get(&language) {
        return Some(dict.clone());
    }
    let dict = Arc::new(load_dictionary_for(language));
    LOADED
        .write()
        .expect("not poisoned")
        .insert(language, dict.clone());
    Some(dict)
}

/// Loads `language`'s dictionary from a downloaded file on disk if one's
/// present and parses cleanly, falling back to the bundled placeholder
/// otherwise (including when a downloaded file exists but fails to parse —
/// e.g. left over from an older, incompatible version).
fn load_dictionary_for(language: SpellCheckLanguage) -> spellbook::Dictionary {
    if let Some(dict) = load_downloaded(language) {
        return dict;
    }
    load_placeholder(language)
}

/// `load_dictionary_for`'s downloaded-file half — `None` whenever anything about
/// the download is unusable (not present, missing an `.aff`/`.dic` filename in
/// the catalog entry, unreadable, or fails to parse), so the caller can fall
/// back to the placeholder without caring which.
fn load_downloaded(language: SpellCheckLanguage) -> Option<spellbook::Dictionary> {
    let entry = language.catalog_entry()?;
    let aff_name = entry.files.iter().find(|f| f.ends_with(".aff"))?;
    let dic_name = entry.files.iter().find(|f| f.ends_with(".dic"))?;
    let aff_path = downloaded_file_path(&entry.language_code, aff_name)?;
    let dic_path = downloaded_file_path(&entry.language_code, dic_name)?;
    let aff = std::fs::read_to_string(aff_path).ok()?;
    let dic = std::fs::read_to_string(dic_path).ok()?;
    spellbook::Dictionary::new(&aff, &dic).ok()
}

/// Drop `language`'s memoized dictionary so the next spell check reloads it —
/// call after `download_dictionary` succeeds, so a freshly downloaded file
/// takes effect without restarting the app.
pub fn invalidate_dictionary_cache(language: SpellCheckLanguage) {
    LOADED.write().expect("not poisoned").remove(&language);
}

/// Whether `word` is *not* recognized by `language`'s dictionary — always
/// `false` for `Off`. Never pre-lowercases `word`: Hunspell `.aff` files encode
/// their own case rules (e.g. `KEEPCASE`), which a naive lowercase-first would
/// bypass incorrectly.
pub fn is_misspelled(word: &str, language: SpellCheckLanguage) -> bool {
    match dictionary_for(language) {
        Some(dict) => !dict.check(word),
        None => false,
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
    fn catalog_parses_and_has_an_entry_for_each_downloadable_language() {
        let codes: Vec<&str> = catalog()
            .iter()
            .map(|entry| entry.language_code.as_str())
            .collect();
        assert_eq!(codes, vec!["en_US", "nb_NO"]);
    }

    #[test]
    fn catalog_entries_carry_a_download_url_and_sha256_for_every_listed_file() {
        for entry in catalog() {
            for file in &entry.files {
                assert!(
                    entry.download_urls.contains_key(file),
                    "{} is missing a download URL for {file}",
                    entry.language
                );
                assert!(
                    entry.sha256.contains_key(file),
                    "{} is missing a SHA-256 for {file}",
                    entry.language
                );
            }
        }
    }

    #[test]
    fn the_blocked_norwegian_entry_is_flagged_blocked() {
        let nb_no = catalog()
            .iter()
            .find(|entry| entry.language_code == "nb_NO")
            .expect("nb_NO is in the catalog");
        assert!(nb_no.is_blocked());
    }

    #[test]
    fn the_needs_review_english_entry_is_not_flagged_blocked() {
        let en_us = catalog()
            .iter()
            .find(|entry| entry.language_code == "en_US")
            .expect("en_US is in the catalog");
        assert!(!en_us.is_blocked());
    }

    #[test]
    fn spell_check_language_from_code_round_trips_every_non_off_variant() {
        assert_eq!(
            SpellCheckLanguage::from_code("en_US"),
            Some(SpellCheckLanguage::English)
        );
        assert_eq!(
            SpellCheckLanguage::from_code("nb_NO"),
            Some(SpellCheckLanguage::Norwegian)
        );
    }

    #[test]
    fn spell_check_language_from_code_is_none_for_an_unknown_code() {
        assert_eq!(SpellCheckLanguage::from_code("xx_XX"), None);
    }

    #[test]
    fn catalog_entry_finds_the_right_entry_for_each_language() {
        assert_eq!(
            SpellCheckLanguage::English
                .catalog_entry()
                .unwrap()
                .language_code,
            "en_US"
        );
        assert_eq!(
            SpellCheckLanguage::Norwegian
                .catalog_entry()
                .unwrap()
                .language_code,
            "nb_NO"
        );
        assert!(SpellCheckLanguage::Off.catalog_entry().is_none());
    }

    #[test]
    fn sha256_hex_matches_a_known_vector() {
        // sha256("hello world")
        assert_eq!(
            sha256_hex(b"hello world"),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
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

    /// End-to-end proof the real download path works, not just its pieces in
    /// isolation: downloads the real English dictionary, confirms
    /// `is_downloaded`/`is_misspelled` pick it up over the placeholder without
    /// restarting anything (via `invalidate_dictionary_cache`), then cleans up
    /// after itself so a manual run doesn't leave a real dictionary sitting in
    /// this machine's data directory. Ignored by default — real network access,
    /// same convention `collab::net`'s iroh-relay tests use.
    #[test]
    #[ignore = "requires real internet access to github.com"]
    fn download_dictionary_fetches_and_verifies_the_real_english_dictionary() {
        assert!(
            is_misspelled("wonderful", SpellCheckLanguage::English),
            "sanity: this word must not already be in the tiny placeholder list"
        );

        download_dictionary(SpellCheckLanguage::English).expect("download should succeed");
        assert!(is_downloaded(SpellCheckLanguage::English));

        invalidate_dictionary_cache(SpellCheckLanguage::English);
        assert!(
            !is_misspelled("wonderful", SpellCheckLanguage::English),
            "a real English dictionary must recognize an ordinary word the placeholder didn't"
        );
        assert!(is_misspelled("wrold", SpellCheckLanguage::English));

        // Clean up: remove the downloaded files and drop the in-memory cache
        // entry, so a repeat run (or any other test in this process) sees the
        // placeholder again, not this test's leftover state.
        if let Some(entry) = SpellCheckLanguage::English.catalog_entry() {
            for file in &entry.files {
                if let Some(path) = downloaded_file_path(&entry.language_code, file) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        invalidate_dictionary_cache(SpellCheckLanguage::English);
        assert!(!is_downloaded(SpellCheckLanguage::English));
    }
}
