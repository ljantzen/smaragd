//! `${{name}}`/`${{date}}` variable substitution for "New From Template" (see
//! `project::Project::create_document_from_template`). Kept separate from
//! `project/mod.rs` since it's pure string handling with no project-state
//! dependency of its own.

use std::fmt::Write;

/// Used whenever `Settings::template_date_format` is blank (a fresh install) or
/// fails to render as a valid strftime pattern — see `format_date`'s doc comment
/// for why an invalid custom format doesn't just fail document creation outright.
pub const DEFAULT_DATE_FORMAT: &str = "%Y-%m-%d";

/// Replace every `${{name}}` with `name` and every `${{date}}` with today's local
/// date, formatted per `date_format` (a chrono strftime pattern, configured in
/// `File > Settings`).
pub fn substitute(contents: &str, name: &str, date_format: &str) -> String {
    contents
        .replace("${{name}}", name)
        .replace("${{date}}", &format_date(date_format))
}

/// Today's local date formatted per `date_format`, falling back to
/// `DEFAULT_DATE_FORMAT` if `date_format` is blank or isn't a valid strftime
/// pattern. `chrono`'s own `Display`/`to_string()` on a formatted date *panics* on
/// an invalid pattern (it treats a formatting error as unexpected), and Settings
/// has no format-string validation of its own — a typo there must degrade
/// gracefully rather than crash document creation, so this renders through
/// `write!` and checks the `Result` explicitly instead. Also backs the format
/// field's live preview in `ui::settings_panel`.
pub fn format_date(date_format: &str) -> String {
    let format = if date_format.trim().is_empty() {
        DEFAULT_DATE_FORMAT
    } else {
        date_format
    };
    try_format_date(format)
        .unwrap_or_else(|| try_format_date(DEFAULT_DATE_FORMAT).expect("default format is valid"))
}

fn try_format_date(format: &str) -> Option<String> {
    let mut buf = String::new();
    write!(buf, "{}", chrono::Local::now().format(format)).ok()?;
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_name_and_date() {
        let result = substitute("# ${{name}}\n\nWritten ${{date}}.", "Aria", "%Y-%m-%d");
        assert!(result.starts_with("# Aria\n\n"));
        assert!(result.contains(&chrono::Local::now().format("%Y-%m-%d").to_string()));
    }

    #[test]
    fn substitutes_every_occurrence() {
        assert_eq!(
            substitute("${{name}}-${{name}}", "Aria", "%Y-%m-%d"),
            "Aria-Aria"
        );
    }

    #[test]
    fn leaves_unrelated_text_untouched() {
        assert_eq!(substitute("plain text", "Aria", "%Y-%m-%d"), "plain text");
    }

    #[test]
    fn blank_date_format_falls_back_to_the_default() {
        assert_eq!(
            format_date(""),
            chrono::Local::now().format(DEFAULT_DATE_FORMAT).to_string()
        );
    }

    #[test]
    fn invalid_date_format_falls_back_to_the_default_instead_of_panicking() {
        // `%Q` isn't a real strftime specifier — chrono can't render it.
        assert_eq!(
            format_date("%Q"),
            chrono::Local::now().format(DEFAULT_DATE_FORMAT).to_string()
        );
    }

    #[test]
    fn valid_custom_format_is_used_as_is() {
        assert_eq!(
            format_date("%Y"),
            chrono::Local::now().format("%Y").to_string()
        );
    }
}
