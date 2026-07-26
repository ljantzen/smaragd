//! A vim/Helix-style `:` command prompt. Tachylite's editor has no modal
//! normal/insert distinction — a literal `:` keypress just types a colon — so the
//! prompt is opened by a dedicated keyboard shortcut instead, but behaves like the
//! command line once open: type a command name (optionally with an argument), Enter
//! to run it, Escape to cancel, Tab/click to accept an autocomplete suggestion.

use egui::{Id, Key, Modifiers};

use crate::autocomplete::filter_candidates;

/// UI state, owned by `app.rs` for the app's lifetime.
#[derive(Default)]
pub struct CommandPromptState {
    pub open: bool,
    /// Set alongside `open` so `show` focuses the input once rather than fighting the
    /// user for focus on every frame the modal is visible.
    pub focus_requested: bool,
    pub input: String,
    /// Index into the current frame's completion candidates, clamped to bounds each
    /// frame since the candidate list changes as the user types.
    pub selected: usize,
}

impl CommandPromptState {
    pub fn request_open(&mut self) {
        self.open = true;
        self.focus_requested = true;
        self.input.clear();
        self.selected = 0;
    }
}

pub enum DarkModeChoice {
    Dark,
    Light,
    System,
}

/// A parsed, ready-to-run command. `app.rs` owns actually executing it, keeping this
/// module a pure parsing/rendering layer, matching `BinderEvent`'s pattern.
pub enum Command {
    Save,
    Quit,
    SaveAndQuit,
    Open(String),
    New(String),
    DarkMode(DarkModeChoice),
    Find(String),
}

pub enum CommandPromptEvent {
    Run(Command),
    /// The input didn't parse to a known command — the message is ready to show
    /// as-is in the status bar.
    Error(String),
}

/// The canonical, completable name for each command — one entry per command, biased
/// toward the descriptive long form where one exists (`write`, not `w`) since the
/// point of completion is discoverability; short aliases like `w`/`q`/`x` still work
/// when typed in full, they just aren't themselves completion targets.
const COMMAND_NAMES: &[&str] = &["write", "quit", "wq", "open", "new", "dmode", "find"];
const DARK_MODE_CHOICES: &[&str] = &["dark", "light", "system"];

/// Parse a line of command-prompt input (a leading `:`, if the user typed one, is
/// tolerated and ignored) into a `Command`, following Helix's short-name-first
/// convention (`:w` before `:write`).
fn parse_command(input: &str) -> Result<Command, String> {
    let input = input.trim().trim_start_matches(':').trim();
    if input.is_empty() {
        return Err("Empty command".to_string());
    }

    let mut parts = input.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();

    match name {
        "w" | "write" => Ok(Command::Save),
        "q" | "quit" => Ok(Command::Quit),
        "wq" | "x" => Ok(Command::SaveAndQuit),
        "o" | "open" if !rest.is_empty() => Ok(Command::Open(rest.to_string())),
        "o" | "open" => Err("Usage: :open <title>".to_string()),
        "new" if !rest.is_empty() => Ok(Command::New(rest.to_string())),
        "new" => Err("Usage: :new <title>".to_string()),
        "dmode" => match rest {
            "dark" => Ok(Command::DarkMode(DarkModeChoice::Dark)),
            "light" => Ok(Command::DarkMode(DarkModeChoice::Light)),
            "system" => Ok(Command::DarkMode(DarkModeChoice::System)),
            _ => Err("Usage: :dmode dark|light|system".to_string()),
        },
        "find" => Ok(Command::Find(rest.to_string())),
        other => Err(format!("Unknown command: {other}")),
    }
}

/// Which part of `input` completion applies to: the command name (no space typed
/// yet), or a recognized command's argument (text after the first space).
enum CompletionTarget<'a> {
    CommandName,
    Argument { command: &'a str, query: &'a str },
}

fn completion_target(input: &str) -> CompletionTarget<'_> {
    match input.find(char::is_whitespace) {
        None => CompletionTarget::CommandName,
        Some(idx) => CompletionTarget::Argument {
            command: &input[..idx],
            query: input[idx..].trim_start(),
        },
    }
}

/// Completion candidates for the current input. Empty for commands that take
/// freeform text (`:new`, `:find`) or none at all (`:w`, `:q`, `:wq`) — there's
/// nothing sensible to suggest there.
fn completions<'a>(input: &str, note_titles: &'a [String]) -> Vec<&'a str> {
    match completion_target(input) {
        CompletionTarget::CommandName => filter_candidates(COMMAND_NAMES, input),
        CompletionTarget::Argument {
            command: "o" | "open",
            query,
        } => filter_candidates(note_titles, query),
        CompletionTarget::Argument {
            command: "dmode",
            query,
        } => filter_candidates(DARK_MODE_CHOICES, query),
        CompletionTarget::Argument { .. } => Vec::new(),
    }
}

/// Splice `chosen` into `input` at the position `completions` suggested it for.
fn apply_completion(input: &str, chosen: &str) -> String {
    match completion_target(input) {
        CompletionTarget::CommandName => format!("{chosen} "),
        CompletionTarget::Argument { command, .. } => format!("{command} {chosen}"),
    }
}

/// Consume (and act on) Tab/arrow keys meant for the suggestion list, so the
/// `TextEdit` underneath never sees them — Tab would otherwise move focus off the
/// field entirely, and the arrows don't do anything useful in a singleline field.
fn steal_popup_key(ctx: &egui::Context) -> Option<PopupAction> {
    ctx.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            Some(PopupAction::Next)
        } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            Some(PopupAction::Prev)
        } else if i.consume_key(Modifiers::NONE, Key::Tab) {
            Some(PopupAction::Accept)
        } else {
            None
        }
    })
}

enum PopupAction {
    Next,
    Prev,
    Accept,
}

/// Renders the command prompt if `state.open`, including an autocomplete popup for
/// command names and, where applicable, their argument (existing document titles for
/// `:open`, the fixed choice list for `:dmode`). Returns `Some` once the user confirms
/// (Enter) or cancels (Escape) this frame.
pub fn show(
    ctx: &egui::Context,
    state: &mut CommandPromptState,
    note_titles: &[String],
) -> Option<CommandPromptEvent> {
    if !state.open {
        return None;
    }

    let mut event = None;
    let mut close = false;

    let candidates = completions(&state.input, note_titles);
    if !candidates.is_empty() {
        state.selected = state.selected.min(candidates.len() - 1);
    }
    let popup_action = (!candidates.is_empty())
        .then(|| steal_popup_key(ctx))
        .flatten();
    match popup_action {
        Some(PopupAction::Next) => state.selected = (state.selected + 1) % candidates.len(),
        Some(PopupAction::Prev) => {
            state.selected = (state.selected + candidates.len() - 1) % candidates.len();
        }
        Some(PopupAction::Accept) => {
            state.input = apply_completion(&state.input, candidates[state.selected]);
            state.selected = 0;
        }
        None => {}
    }

    egui::Modal::new(Id::new("command_prompt_modal")).show(ctx, |ui| {
        ui.set_min_width(360.0);
        ui.horizontal(|ui| {
            ui.label(":");
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.input)
                    .desired_width(f32::INFINITY)
                    .hint_text(
                        "w | q | wq | open <title> | new <title> | dmode dark|light|system | find <text>",
                    ),
            );
            if state.focus_requested {
                response.request_focus();
                state.focus_requested = false;
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                event = Some(match parse_command(&state.input) {
                    Ok(command) => CommandPromptEvent::Run(command),
                    Err(err) => CommandPromptEvent::Error(err),
                });
                close = true;
            }
        });

        if !candidates.is_empty() {
            ui.separator();
            for (index, candidate) in candidates.iter().enumerate() {
                if ui
                    .selectable_label(index == state.selected, *candidate)
                    .clicked()
                {
                    state.input = apply_completion(&state.input, candidate);
                    state.selected = 0;
                }
            }
        }

        if ui.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
    });

    if close {
        state.open = false;
    }
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_short_and_long_save() {
        assert!(matches!(parse_command("w"), Ok(Command::Save)));
        assert!(matches!(parse_command("write"), Ok(Command::Save)));
    }

    #[test]
    fn parses_short_and_long_quit() {
        assert!(matches!(parse_command("q"), Ok(Command::Quit)));
        assert!(matches!(parse_command("quit"), Ok(Command::Quit)));
    }

    #[test]
    fn parses_save_and_quit_aliases() {
        assert!(matches!(parse_command("wq"), Ok(Command::SaveAndQuit)));
        assert!(matches!(parse_command("x"), Ok(Command::SaveAndQuit)));
    }

    #[test]
    fn parses_open_with_a_multi_word_title() {
        match parse_command("open Opening Scene") {
            Ok(Command::Open(title)) => assert_eq!(title, "Opening Scene"),
            other => panic!("expected Command::Open, got {}", describe(&other)),
        }
    }

    #[test]
    fn open_without_a_title_is_an_error() {
        assert!(parse_command("open").is_err());
        assert!(parse_command("o").is_err());
    }

    #[test]
    fn parses_new_with_a_title() {
        match parse_command("new Chapter 2") {
            Ok(Command::New(title)) => assert_eq!(title, "Chapter 2"),
            other => panic!("expected Command::New, got {}", describe(&other)),
        }
    }

    #[test]
    fn new_without_a_title_is_an_error() {
        assert!(parse_command("new").is_err());
    }

    #[test]
    fn parses_dmode_choices() {
        assert!(matches!(
            parse_command("dmode dark"),
            Ok(Command::DarkMode(DarkModeChoice::Dark))
        ));
        assert!(matches!(
            parse_command("dmode light"),
            Ok(Command::DarkMode(DarkModeChoice::Light))
        ));
        assert!(matches!(
            parse_command("dmode system"),
            Ok(Command::DarkMode(DarkModeChoice::System))
        ));
    }

    #[test]
    fn dmode_with_an_invalid_argument_is_an_error() {
        assert!(parse_command("dmode neon").is_err());
        assert!(parse_command("dmode").is_err());
    }

    #[test]
    fn parses_find_with_and_without_a_query() {
        match parse_command("find needle") {
            Ok(Command::Find(query)) => assert_eq!(query, "needle"),
            other => panic!("expected Command::Find, got {}", describe(&other)),
        }
        assert!(matches!(parse_command("find"), Ok(Command::Find(query)) if query.is_empty()));
    }

    #[test]
    fn a_leading_colon_is_tolerated() {
        assert!(matches!(parse_command(":w"), Ok(Command::Save)));
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert!(matches!(parse_command("  w  "), Ok(Command::Save)));
    }

    #[test]
    fn empty_input_is_an_error() {
        assert!(parse_command("").is_err());
        assert!(parse_command("   ").is_err());
    }

    #[test]
    fn unknown_command_is_an_error_naming_it() {
        match parse_command("frobnicate") {
            Err(msg) => assert!(msg.contains("frobnicate")),
            Ok(_) => panic!("expected an error"),
        }
    }

    fn describe(result: &Result<Command, String>) -> &'static str {
        match result {
            Ok(_) => "Ok(..)",
            Err(_) => "Err(..)",
        }
    }

    #[test]
    fn command_name_completions_are_prefix_filtered() {
        // Prefix matches ("wq", "write") rank ahead of a mere substring match
        // ("new" contains "w"), each group sorted alphabetically.
        assert_eq!(completions("w", &[]), vec!["wq", "write", "new"]);
        // Likewise "dmode" (prefix) ahead of "find" (contains "d").
        assert_eq!(completions("d", &[]), vec!["dmode", "find"]);
    }

    #[test]
    fn command_name_completions_still_include_a_fully_typed_exact_match() {
        // "quit" is itself the only candidate once you've typed the whole word.
        assert_eq!(completions("quit", &[]), vec!["quit"]);
    }

    #[test]
    fn open_argument_completes_against_note_titles() {
        let titles = vec!["Opening Scene".to_string(), "Backstory".to_string()];
        assert_eq!(completions("open open", &titles), vec!["Opening Scene"]);
        assert_eq!(completions("o open", &titles), vec!["Opening Scene"]);
    }

    #[test]
    fn dmode_argument_completes_against_the_fixed_choices() {
        assert_eq!(completions("dmode d", &[]), vec!["dark"]);
    }

    #[test]
    fn new_and_find_arguments_have_no_completions() {
        let titles = vec!["Opening Scene".to_string()];
        assert!(completions("new Open", &titles).is_empty());
        assert!(completions("find Open", &titles).is_empty());
    }

    #[test]
    fn unrecognized_command_has_no_argument_completions() {
        assert!(completions("frobnicate a", &[]).is_empty());
    }

    #[test]
    fn apply_completion_replaces_the_command_name_and_appends_a_space() {
        assert_eq!(apply_completion("w", "write"), "write ");
    }

    #[test]
    fn apply_completion_replaces_the_whole_argument() {
        assert_eq!(
            apply_completion("open open", "Opening Scene"),
            "open Opening Scene"
        );
    }
}
