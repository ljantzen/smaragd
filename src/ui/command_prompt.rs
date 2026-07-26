//! A vim/Helix-style `:` command prompt. Tachylite's editor has no modal
//! normal/insert distinction — a literal `:` keypress just types a colon — so the
//! prompt is opened by a dedicated keyboard shortcut instead, but behaves like the
//! command line once open: type a command name (optionally with an argument), Enter
//! to run it, Escape to cancel.

/// UI state, owned by `app.rs` for the app's lifetime.
#[derive(Default)]
pub struct CommandPromptState {
    pub open: bool,
    /// Set alongside `open` so `show` focuses the input once rather than fighting the
    /// user for focus on every frame the modal is visible.
    pub focus_requested: bool,
    pub input: String,
}

impl CommandPromptState {
    pub fn request_open(&mut self) {
        self.open = true;
        self.focus_requested = true;
        self.input.clear();
    }
}

pub enum ThemeChoice {
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
    Theme(ThemeChoice),
    Find(String),
}

pub enum CommandPromptEvent {
    Run(Command),
    /// The input didn't parse to a known command — the message is ready to show
    /// as-is in the status bar.
    Error(String),
}

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
        "theme" => match rest {
            "dark" => Ok(Command::Theme(ThemeChoice::Dark)),
            "light" => Ok(Command::Theme(ThemeChoice::Light)),
            "system" => Ok(Command::Theme(ThemeChoice::System)),
            _ => Err("Usage: :theme dark|light|system".to_string()),
        },
        "find" => Ok(Command::Find(rest.to_string())),
        other => Err(format!("Unknown command: {other}")),
    }
}

/// Renders the command prompt if `state.open`. Returns `Some` once the user confirms
/// (Enter) or cancels (Escape) this frame.
pub fn show(ctx: &egui::Context, state: &mut CommandPromptState) -> Option<CommandPromptEvent> {
    if !state.open {
        return None;
    }

    let mut event = None;
    let mut close = false;

    egui::Modal::new(egui::Id::new("command_prompt_modal")).show(ctx, |ui| {
        ui.set_min_width(360.0);
        ui.horizontal(|ui| {
            ui.label(":");
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.input)
                    .desired_width(f32::INFINITY)
                    .hint_text("w | q | wq | open <title> | new <title> | theme dark|light|system | find <text>"),
            );
            if state.focus_requested {
                response.request_focus();
                state.focus_requested = false;
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                event = Some(match parse_command(&state.input) {
                    Ok(command) => CommandPromptEvent::Run(command),
                    Err(err) => CommandPromptEvent::Error(err),
                });
                close = true;
            }
        });

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
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
    fn parses_theme_choices() {
        assert!(matches!(
            parse_command("theme dark"),
            Ok(Command::Theme(ThemeChoice::Dark))
        ));
        assert!(matches!(
            parse_command("theme light"),
            Ok(Command::Theme(ThemeChoice::Light))
        ));
        assert!(matches!(
            parse_command("theme system"),
            Ok(Command::Theme(ThemeChoice::System))
        ));
    }

    #[test]
    fn theme_with_an_invalid_argument_is_an_error() {
        assert!(parse_command("theme neon").is_err());
        assert!(parse_command("theme").is_err());
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
}
