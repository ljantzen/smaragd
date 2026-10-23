//! `${{...}}` variable substitution for "New From Template" (see
//! `project::Project::create_document_from_template`) and Daily Notes (see
//! `project::Project::open_or_create_daily_note`). Kept separate from
//! `project/mod.rs` since it's pure string handling with no project-state
//! dependency of its own.
//!
//! Supported placeholders, each either `${{keyword}}` or `${{keyword:FORMAT}}`
//! (an inline format override):
//! - `${{name}}` — the name passed to `substitute` (never takes `:FORMAT`).
//! - `${{date}}` — today's local date, via `FORMAT` if given, else `TemplateFormats::date`.
//! - `${{time}}` — the current local time, via `FORMAT` if given, else `TemplateFormats::time`.
//! - `${{previous_day}}` / `${{next_day}}` — today ± 1 day, via `FORMAT` if given, else
//!   `TemplateFormats::adjacent_day`.
//! - `${{cursor}}` — emits nothing; the byte offset of its first occurrence in the
//!   *output* is returned alongside the substituted string, for the caller to place the
//!   editor's cursor there. Any further occurrences are also stripped, without moving
//!   that offset.
//!
//! Anything else — an unrecognized keyword, or a `${{` with no matching `}}` — is left
//! untouched verbatim.

use std::fmt::Write;

use chrono::NaiveDate;

/// Used whenever a date format is blank (a fresh install) or fails to render as a valid
/// strftime pattern — see `format_date_value`'s doc comment for why an invalid custom
/// format doesn't just fail document creation outright.
pub const DEFAULT_DATE_FORMAT: &str = "%Y-%m-%d";

/// The `${{time}}` counterpart of `DEFAULT_DATE_FORMAT`.
pub const DEFAULT_TIME_FORMAT: &str = "%H:%M";

/// Formats consulted when a placeholder has no inline `:FORMAT` override of its own.
pub struct TemplateFormats<'a> {
    /// `${{date}}`'s fallback — `Settings::template_date_format`.
    pub date: &'a str,
    /// `${{time}}`'s fallback — `Settings::template_time_format`.
    pub time: &'a str,
    /// `${{previous_day}}`/`${{next_day}}`'s fallback. Daily Notes passes its own
    /// configured filename format here (so the rendered link matches an actual daily
    /// note's filename); plain "New From Template" has no such context and just reuses
    /// its own `date` format for this too.
    pub adjacent_day: &'a str,
}

/// Substitute every recognized `${{...}}` placeholder in `contents` (see the module
/// doc comment for the full list) and return the result, plus the byte offset of the
/// first `${{cursor}}` placeholder's position in that result, if any.
pub fn substitute(
    contents: &str,
    name: &str,
    formats: &TemplateFormats,
) -> (String, Option<usize>) {
    let mut out = String::with_capacity(contents.len());
    let mut cursor = None;
    let mut rest = contents;
    let today = chrono::Local::now().date_naive();

    while let Some(start) = rest.find("${{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 3..];
        let Some(end) = after.find("}}") else {
            // No closing brace anywhere in the rest of the string — emit it verbatim
            // and stop, rather than silently swallowing a malformed trailing token.
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let token = &after[..end];
        let (keyword, arg) = match token.split_once(':') {
            Some((k, a)) => (k, Some(a)),
            None => (token, None),
        };
        match keyword {
            "name" => out.push_str(name),
            "date" => out.push_str(&format_date_value(today, arg.unwrap_or(formats.date))),
            "time" => out.push_str(&format_time(arg.unwrap_or(formats.time))),
            "previous_day" => out.push_str(&format_date_value(
                today - chrono::Duration::days(1),
                arg.unwrap_or(formats.adjacent_day),
            )),
            "next_day" => out.push_str(&format_date_value(
                today + chrono::Duration::days(1),
                arg.unwrap_or(formats.adjacent_day),
            )),
            "cursor" => {
                if cursor.is_none() {
                    cursor = Some(out.len());
                }
            }
            _ => {
                // Unrecognized placeholder — leave it exactly as written.
                out.push_str("${{");
                out.push_str(token);
                out.push_str("}}");
            }
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    (out, cursor)
}

/// Today's local date formatted per `date_format`, falling back to
/// `DEFAULT_DATE_FORMAT` if `date_format` is blank or isn't a valid strftime pattern.
/// Also backs the date format field's live preview in `ui::settings_panel`.
pub fn format_date(date_format: &str) -> String {
    format_date_value(chrono::Local::now().date_naive(), date_format)
}

/// [`format_date`], for an arbitrary `date` rather than always today — what
/// `${{date}}`/`${{previous_day}}`/`${{next_day}}` are rendered through.
/// `chrono`'s own `Display`/`to_string()` on a formatted date *panics* on an invalid
/// pattern (it treats a formatting error as unexpected), and neither `Settings` nor
/// `ProjectMeta` validate a format string up front — a typo there must degrade
/// gracefully rather than crash document creation, so this renders through `write!`
/// and checks the `Result` explicitly instead.
pub fn format_date_value(date: NaiveDate, date_format: &str) -> String {
    let format = if date_format.trim().is_empty() {
        DEFAULT_DATE_FORMAT
    } else {
        date_format
    };
    try_format_date(date, format).unwrap_or_else(|| {
        try_format_date(date, DEFAULT_DATE_FORMAT).expect("default format is valid")
    })
}

/// The current local time formatted per `time_format`, falling back to
/// `DEFAULT_TIME_FORMAT` on the same blank/invalid conditions as `format_date_value`.
/// Also backs the time format field's live preview in `ui::settings_panel`.
pub fn format_time(time_format: &str) -> String {
    let format = if time_format.trim().is_empty() {
        DEFAULT_TIME_FORMAT
    } else {
        time_format
    };
    try_format_time(format)
        .unwrap_or_else(|| try_format_time(DEFAULT_TIME_FORMAT).expect("default format is valid"))
}

fn try_format_date(date: NaiveDate, format: &str) -> Option<String> {
    let mut buf = String::new();
    write!(buf, "{}", date.format(format)).ok()?;
    Some(buf)
}

fn try_format_time(format: &str) -> Option<String> {
    let mut buf = String::new();
    write!(buf, "{}", chrono::Local::now().format(format)).ok()?;
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formats<'a>(date: &'a str, time: &'a str, adjacent_day: &'a str) -> TemplateFormats<'a> {
        TemplateFormats {
            date,
            time,
            adjacent_day,
        }
    }

    #[test]
    fn substitutes_name_and_date() {
        let (result, cursor) = substitute(
            "# ${{name}}\n\nWritten ${{date}}.",
            "Aria",
            &formats("%Y-%m-%d", "%H:%M", "%Y-%m-%d"),
        );
        assert!(result.starts_with("# Aria\n\n"));
        assert!(result.contains(&chrono::Local::now().format("%Y-%m-%d").to_string()));
        assert_eq!(cursor, None);
    }

    #[test]
    fn substitutes_every_occurrence() {
        let (result, _) = substitute(
            "${{name}}-${{name}}",
            "Aria",
            &formats("%Y-%m-%d", "%H:%M", "%Y-%m-%d"),
        );
        assert_eq!(result, "Aria-Aria");
    }

    #[test]
    fn leaves_unrelated_text_untouched() {
        let (result, _) = substitute(
            "plain text",
            "Aria",
            &formats("%Y-%m-%d", "%H:%M", "%Y-%m-%d"),
        );
        assert_eq!(result, "plain text");
    }

    #[test]
    fn leaves_unrecognized_placeholders_untouched() {
        let (result, _) = substitute(
            "${{nonsense}}",
            "Aria",
            &formats("%Y-%m-%d", "%H:%M", "%Y-%m-%d"),
        );
        assert_eq!(result, "${{nonsense}}");
    }

    #[test]
    fn leaves_an_unterminated_placeholder_untouched() {
        let (result, _) = substitute(
            "prefix ${{date",
            "Aria",
            &formats("%Y-%m-%d", "%H:%M", "%Y-%m-%d"),
        );
        assert_eq!(result, "prefix ${{date");
    }

    #[test]
    fn substitutes_time() {
        let (result, _) = substitute(
            "${{time}}",
            "Aria",
            &formats("%Y-%m-%d", "%H:%M", "%Y-%m-%d"),
        );
        assert_eq!(result, chrono::Local::now().format("%H:%M").to_string());
    }

    #[test]
    fn inline_date_format_overrides_the_default() {
        let (result, _) = substitute(
            "${{date:%Y}}",
            "Aria",
            &formats("%Y-%m-%d", "%H:%M", "%Y-%m-%d"),
        );
        assert_eq!(result, chrono::Local::now().format("%Y").to_string());
    }

    #[test]
    fn inline_time_format_overrides_the_default() {
        let (result, _) = substitute(
            "${{time:%H}}",
            "Aria",
            &formats("%Y-%m-%d", "%H:%M", "%Y-%m-%d"),
        );
        assert_eq!(result, chrono::Local::now().format("%H").to_string());
    }

    #[test]
    fn previous_and_next_day_use_the_adjacent_day_format() {
        let (result, _) = substitute(
            "${{previous_day}}/${{next_day}}",
            "Aria",
            &formats("%Y-%m-%d", "%H:%M", "%Y-%m-%d"),
        );
        let today = chrono::Local::now().date_naive();
        let expected = format!(
            "{}/{}",
            (today - chrono::Duration::days(1)).format("%Y-%m-%d"),
            (today + chrono::Duration::days(1)).format("%Y-%m-%d")
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn previous_day_inline_format_overrides_the_adjacent_day_default() {
        let (result, _) = substitute(
            "${{previous_day:%Y}}",
            "Aria",
            &formats("%Y-%m-%d", "%H:%M", "%m-%d"),
        );
        let yesterday = chrono::Local::now().date_naive() - chrono::Duration::days(1);
        assert_eq!(result, yesterday.format("%Y").to_string());
    }

    #[test]
    fn previous_day_rolls_over_month_and_year_boundaries() {
        let new_years_day = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert_eq!(
            format_date_value(new_years_day - chrono::Duration::days(1), "%Y-%m-%d"),
            "2025-12-31"
        );
    }

    #[test]
    fn cursor_placeholder_reports_its_output_offset_and_is_stripped() {
        let (result, cursor) = substitute(
            "${{name}}${{cursor}} tail",
            "Aria",
            &formats("%Y-%m-%d", "%H:%M", "%Y-%m-%d"),
        );
        assert_eq!(result, "Aria tail");
        assert_eq!(cursor, Some("Aria".len()));
    }

    #[test]
    fn multiple_cursor_placeholders_collapse_to_the_first_offset() {
        let (result, cursor) = substitute(
            "${{cursor}}a${{cursor}}b",
            "Aria",
            &formats("%Y-%m-%d", "%H:%M", "%Y-%m-%d"),
        );
        assert_eq!(result, "ab");
        assert_eq!(cursor, Some(0));
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

    #[test]
    fn blank_time_format_falls_back_to_the_default() {
        assert_eq!(
            format_time(""),
            chrono::Local::now().format(DEFAULT_TIME_FORMAT).to_string()
        );
    }

    #[test]
    fn invalid_time_format_falls_back_to_the_default_instead_of_panicking() {
        assert_eq!(
            format_time("%Q"),
            chrono::Local::now().format(DEFAULT_TIME_FORMAT).to_string()
        );
    }
}
