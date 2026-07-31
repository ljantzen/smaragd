//! Pure text diffing used to translate `egui::TextEdit` buffer mutations into
//! CRDT operations and back. See `src/collab/crdt.rs` for how [`TextChange`]
//! feeds into `yrs`.

/// A single contiguous edit: delete `old[pos..pos + deleted_len]`, then insert
/// `inserted` at `pos`. All offsets are byte offsets, matching the rest of
/// this codebase's convention (e.g. `autocomplete::char_offset_to_byte`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChange {
    pub pos: usize,
    pub deleted_len: usize,
    pub inserted: String,
}

/// Diffs `old` against `new`, returning the single contiguous edit that
/// turns `old` into `new`, or `None` if they're identical.
///
/// This is the standard common-prefix/common-suffix technique for plain
/// textareas: `egui::TextEdit` always replaces exactly one contiguous byte
/// range per frame (a keystroke, IME commit, paste, cut, or drag-delete), so
/// there is never more than one edit span to reconstruct between two
/// frame-to-frame snapshots.
pub fn diff(old: &str, new: &str) -> Option<TextChange> {
    if old == new {
        return None;
    }

    let old_bytes = old.as_bytes();
    let new_bytes = new.as_bytes();
    let max_common = old_bytes.len().min(new_bytes.len());

    let mut prefix = 0;
    while prefix < max_common && old_bytes[prefix] == new_bytes[prefix] {
        prefix += 1;
    }
    // The raw byte-equal prefix may split a multi-byte UTF-8 sequence right
    // at the point where `old` and `new` diverge (two different multi-byte
    // characters can share a leading byte). Clamp down to a boundary valid
    // in both strings before slicing on it.
    while prefix > 0 && !(old.is_char_boundary(prefix) && new.is_char_boundary(prefix)) {
        prefix -= 1;
    }

    let max_suffix = max_common - prefix;
    let mut suffix = 0;
    while suffix < max_suffix
        && old_bytes[old_bytes.len() - 1 - suffix] == new_bytes[new_bytes.len() - 1 - suffix]
    {
        suffix += 1;
    }
    while suffix > 0
        && !(old.is_char_boundary(old_bytes.len() - suffix)
            && new.is_char_boundary(new_bytes.len() - suffix))
    {
        suffix -= 1;
    }

    let deleted_len = old_bytes.len() - prefix - suffix;
    let inserted = new[prefix..new_bytes.len() - suffix].to_string();

    Some(TextChange {
        pos: prefix,
        deleted_len,
        inserted,
    })
}

/// Adjusts a local cursor's byte offset for a remote edit that just changed
/// the buffer out from under it, so the caret stays put relative to the
/// surrounding text rather than jumping to wherever it now falls in the raw
/// byte stream. Unchanged if the edit is entirely after the cursor; shifted
/// by the edit's net length delta if entirely before; clamped to the edit's
/// start if the cursor was sitting inside text the remote peer just deleted
/// (the least-surprising landing spot — that's where the deleted text used
/// to start).
pub fn adjust_cursor(cursor_byte: usize, change: &TextChange) -> usize {
    if cursor_byte <= change.pos {
        cursor_byte
    } else if cursor_byte >= change.pos + change.deleted_len {
        let delta = change.inserted.len() as isize - change.deleted_len as isize;
        (cursor_byte as isize + delta).max(0) as usize
    } else {
        change.pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(pos: usize, deleted_len: usize, inserted: &str) -> TextChange {
        TextChange {
            pos,
            deleted_len,
            inserted: inserted.to_string(),
        }
    }

    /// Applies a [`TextChange`] the same way the CRDT layer will, as an
    /// independent way to assert `diff` actually reconstructs `new`.
    fn apply(old: &str, change: &TextChange) -> String {
        let mut result = String::new();
        result.push_str(&old[..change.pos]);
        result.push_str(&change.inserted);
        result.push_str(&old[change.pos + change.deleted_len..]);
        result
    }

    #[test]
    fn identical_strings_produce_no_change() {
        assert_eq!(diff("hello", "hello"), None);
        assert_eq!(diff("", ""), None);
    }

    #[test]
    fn pure_insertion_at_end() {
        let result = diff("hello", "hello world").unwrap();
        assert_eq!(result, change(5, 0, " world"));
    }

    #[test]
    fn pure_insertion_at_start() {
        let result = diff("world", "hello world").unwrap();
        assert_eq!(result, change(0, 0, "hello "));
    }

    #[test]
    fn insertion_in_the_middle() {
        let result = diff("ac", "abc").unwrap();
        assert_eq!(result, change(1, 0, "b"));
    }

    #[test]
    fn pure_deletion() {
        let result = diff("hello world", "hello").unwrap();
        assert_eq!(result, change(5, 6, ""));
    }

    #[test]
    fn deletion_in_the_middle() {
        let result = diff("abc", "ac").unwrap();
        assert_eq!(result, change(1, 1, ""));
    }

    #[test]
    fn replace_a_span() {
        let result = diff("hello world", "hello there").unwrap();
        assert_eq!(result, change(6, 5, "there"));
    }

    #[test]
    fn empty_to_nonempty_is_a_full_insertion() {
        let result = diff("", "hello").unwrap();
        assert_eq!(result, change(0, 0, "hello"));
    }

    #[test]
    fn nonempty_to_empty_is_a_full_deletion() {
        let result = diff("hello", "").unwrap();
        assert_eq!(result, change(0, 5, ""));
    }

    #[test]
    fn completely_disjoint_strings_replace_everything() {
        let result = diff("abc", "xyz").unwrap();
        assert_eq!(result, change(0, 3, "xyz"));
    }

    #[test]
    fn diff_never_splits_a_multibyte_char_even_when_raw_prefix_would() {
        // 'é' (C3 A9) and 'è' (C3 A8) share a leading byte, so a naive
        // byte-by-byte prefix match would stop one byte into the character,
        // landing mid-sequence. The diff must back off to the boundary
        // before it and treat the whole character as replaced.
        let old = "a\u{e9}b"; // "aéb"
        let new = "a\u{e8}b"; // "aèb"
        let result = diff(old, new).unwrap();
        assert_eq!(result, change(1, 2, "\u{e8}"));
        assert_eq!(apply(old, &result), new);
    }

    #[test]
    fn diff_handles_emoji_boundaries() {
        let old = "before 🎉 after";
        let new = "before 🎊 after";
        let result = diff(old, new).unwrap();
        assert_eq!(apply(old, &result), new);
        // The emoji itself (4 bytes) must be replaced as a whole, not split.
        assert_eq!(result.inserted, "🎊");
    }

    #[test]
    fn diff_result_always_reconstructs_new_from_old() {
        let cases: &[(&str, &str)] = &[
            ("hello", "hello world"),
            ("hello world", "hello"),
            ("abc", "xyz"),
            ("", "non-empty"),
            ("non-empty", ""),
            ("café", "coffee"),
            ("🎉🎉🎉", "🎉🎊🎉"),
            ("The quick brown fox", "The slow brown fox"),
        ];
        for (old, new) in cases {
            if let Some(result) = diff(old, new) {
                assert_eq!(apply(old, &result), *new, "failed for {old:?} -> {new:?}");
            } else {
                assert_eq!(old, new);
            }
        }
    }

    #[test]
    fn adjust_cursor_is_unchanged_when_the_edit_is_entirely_after_it() {
        let change = change(10, 3, "xyz");
        assert_eq!(adjust_cursor(5, &change), 5);
        assert_eq!(adjust_cursor(10, &change), 10);
    }

    #[test]
    fn adjust_cursor_shifts_by_the_net_length_delta_when_entirely_before_it() {
        // A 3-byte deletion replaced by a 5-byte insertion: +2 net.
        let change = change(0, 3, "hello");
        assert_eq!(adjust_cursor(3, &change), 5);
        assert_eq!(adjust_cursor(10, &change), 12);
    }

    #[test]
    fn adjust_cursor_shifts_back_for_a_net_deletion() {
        // A 5-byte deletion replaced by nothing: -5 net.
        let change = change(0, 5, "");
        assert_eq!(adjust_cursor(10, &change), 5);
    }

    #[test]
    fn adjust_cursor_clamps_to_the_edit_start_when_inside_the_deleted_span() {
        let change = change(4, 6, "x");
        assert_eq!(adjust_cursor(7, &change), 4);
        assert_eq!(adjust_cursor(9, &change), 4);
    }

    #[test]
    fn adjust_cursor_never_underflows_on_a_large_deletion_before_a_small_cursor() {
        let change = change(0, 100, "");
        assert_eq!(adjust_cursor(0, &change), 0);
    }
}
