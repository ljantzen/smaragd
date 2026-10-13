//! Temporary instrumentation for an intermittent, unreproduced "Is a
//! directory" (EISDIR) failure seen when saving a document — the on-disk
//! path involved is only ever a plain file before and after the failure, so
//! a cold read of the write/rename call sites hasn't found the mechanism.
//! Every place that actually writes an open document's bytes to disk
//! (`editor::OpenDocument::save_with_store`/`flush_for_word_count`, and the
//! sync engine's own `write_file`) calls [`log_write_attempt`] right around
//! that write, so if this recurs, `smaragd-save-debug.log` will show the
//! exact path, which code path attempted it, and the exact error — instead
//! of needing to catch it live. Remove this module (and its call sites)
//! once the root cause is found and fixed.
use std::fs;
use std::io;
use std::path::Path;

/// Appends one line per write attempt to `$TMPDIR/smaragd-save-debug.log` —
/// deliberately outside the project folder (never synced, never committed)
/// and the OS data dir (no `directories` plumbing needed for what's meant to
/// be removed shortly). Best-effort: any I/O error while logging is silently
/// dropped rather than surfaced, since a logging hiccup must never make a
/// real save look like it failed.
pub fn log_write_attempt(label: &str, path: &Path, result: &io::Result<()>) {
    use std::io::Write as _;
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let line = match result {
        Ok(()) => format!("{now} {label} OK {}\n", path.display()),
        Err(err) => format!(
            "{now} {label} ERR {} -- {err} (kind: {:?})\n",
            path.display(),
            err.kind()
        ),
    };
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::temp_dir().join("smaragd-save-debug.log"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_write_attempt_appends_a_line_for_both_outcomes() {
        let log_path = std::env::temp_dir().join("smaragd-save-debug.log");
        let before = fs::read_to_string(&log_path).unwrap_or_default();

        log_write_attempt("test-label-ok", Path::new("some/doc.md"), &Ok(()));
        log_write_attempt(
            "test-label-err",
            Path::new("some/other.md"),
            &Err(io::Error::other("boom")),
        );

        let after = fs::read_to_string(&log_path).unwrap();
        let added = &after[before.len()..];
        assert!(added.contains("test-label-ok OK some/doc.md"));
        assert!(added.contains("test-label-err ERR some/other.md -- boom"));
    }
}
