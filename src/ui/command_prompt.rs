//! A vim/Helix-style `:` command prompt. Smaragd's editor has no modal
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
    /// A Helix-style color theme by id, or `None` for "default" (no theme, plain
    /// `:dmode` styling) — `app.rs` resolves the id against `color_theme::find`,
    /// since validating it needs no data this pure-parsing module has access to.
    ColorTheme(Option<String>),
    Git(GitCommand),
    Find(String),
    /// Open the Tags dock filtered to documents carrying the given tag (empty
    /// string just opens the dock without changing its current filter).
    Tag(String),
    /// A `:` command a loaded plugin registered (name, argument) — `app.rs` looks
    /// up which plugin owns `name` and runs it.
    Plugin(String, String),
}

/// A `:git <subcommand>` action,
/// `Commit`/`Backup`'s `Option<String>` is the commit message: `Some` when
/// given inline (`:git commit fixed typo`), `None` to prompt for one instead (with a
/// default pre-filled) — `app.rs` decides which since it owns the message-prompt
/// modal.
pub enum GitCommand {
    Enable,
    Commit(Option<String>),
    Push,
    Pull,
    /// Commit and push in one action
    Backup(Option<String>),
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
const COMMAND_NAMES: &[&str] = &[
    "write", "quit", "wq", "open", "new", "dmode", "theme", "git", "find", "tag",
];
const DARK_MODE_CHOICES: &[&str] = &["dark", "light", "system"];
const GIT_SUBCOMMANDS: &[&str] = &["enable", "commit", "push", "pull", "backup"];

/// Parse a line of command-prompt input (a leading `:`, if the user typed one, is
/// tolerated and ignored) into a `Command`, following Helix's short-name-first
/// convention (`:w` before `:write`). `plugin_commands` is checked only once none
/// of the built-in names match, so a plugin can never shadow a built-in command.
fn parse_command(input: &str, plugin_commands: &[String]) -> Result<Command, String> {
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
        "theme" if rest.eq_ignore_ascii_case("default") => Ok(Command::ColorTheme(None)),
        "theme" if !rest.is_empty() => Ok(Command::ColorTheme(Some(rest.to_lowercase()))),
        "theme" => Err("Usage: :theme <name> (or :theme default)".to_string()),
        "git" if !rest.is_empty() => parse_git_subcommand(rest),
        "git" => Err("Usage: :git enable|commit|push|pull|backup [message]".to_string()),
        "find" => Ok(Command::Find(rest.to_string())),
        "tag" => Ok(Command::Tag(rest.to_string())),
        other if plugin_commands.iter().any(|c| c == other) => {
            Ok(Command::Plugin(other.to_string(), rest.to_string()))
        }
        other => Err(format!("Unknown command: {other}")),
    }
}

fn parse_git_subcommand(rest: &str) -> Result<Command, String> {
    let mut parts = rest.splitn(2, char::is_whitespace);
    let sub = parts.next().unwrap_or("");
    let message = parts.next().unwrap_or("").trim();
    let message = (!message.is_empty()).then(|| message.to_string());

    match sub {
        "enable" => Ok(Command::Git(GitCommand::Enable)),
        "commit" => Ok(Command::Git(GitCommand::Commit(message))),
        "push" => Ok(Command::Git(GitCommand::Push)),
        "pull" => Ok(Command::Git(GitCommand::Pull)),
        "backup" => Ok(Command::Git(GitCommand::Backup(message))),
        other => Err(format!("Unknown git subcommand: {other}")),
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

/// Cap on how many suggestions are shown at once, matching the wikilink popup
/// (`editor_panel.rs`) and the story-card linked-document popup (`corkboard_panel.rs`)
/// — `:open` in particular can otherwise dump an entire large vault's worth of titles.
const MAX_SUGGESTIONS: usize = 8;

/// Completion candidates for the current input. Empty for commands that take
/// freeform text (`:new`, `:find`) or none at all (`:w`, `:q`, `:wq`) — there's
/// nothing sensible to suggest there.
fn completions<'a>(
    input: &str,
    note_titles: &'a [String],
    plugin_commands: &'a [String],
    theme_ids: &'a [String],
    tag_names: &'a [String],
    git_enabled: bool,
) -> Vec<&'a str> {
    let mut matches = match completion_target(input) {
        CompletionTarget::CommandName => {
            let mut matches = filter_candidates(COMMAND_NAMES, input);
            if !git_enabled {
                matches.retain(|name| *name != "git");
            }
            matches.extend(filter_candidates(plugin_commands, input));
            matches
        }
        CompletionTarget::Argument {
            command: "o" | "open",
            query,
        } => filter_candidates(note_titles, query),
        CompletionTarget::Argument {
            command: "dmode",
            query,
        } => filter_candidates(DARK_MODE_CHOICES, query),
        CompletionTarget::Argument {
            command: "theme",
            query,
        } => {
            let mut matches = filter_candidates(theme_ids, query);
            if "default".starts_with(&query.to_lowercase()) {
                // Inserted at the front, not pushed to the back: with MAX_SUGGESTIONS
                // now truncating this list, appending would let "default" silently
                // fall off the end once there are 8+ real theme matches.
                matches.insert(0, "default");
            }
            matches
        }
        CompletionTarget::Argument {
            command: "tag",
            query,
        } => filter_candidates(tag_names, query),
        CompletionTarget::Argument {
            command: "git",
            query,
        } => {
            if !git_enabled || query.contains(char::is_whitespace) {
                // Already past the subcommand, into freeform commit-message text —
                // or git integration is off, in which case there's nothing to
                // suggest either (see the `CommandName` arm above).
                Vec::new()
            } else {
                filter_candidates(GIT_SUBCOMMANDS, query)
            }
        }
        CompletionTarget::Argument { .. } => Vec::new(),
    };
    matches.truncate(MAX_SUGGESTIONS);
    matches
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
    plugin_commands: &[String],
    theme_ids: &[String],
    tag_names: &[String],
    git_enabled: bool,
) -> Option<CommandPromptEvent> {
    if !state.open {
        return None;
    }

    let mut event = None;
    let mut close = false;

    let candidates = completions(
        &state.input,
        note_titles,
        plugin_commands,
        theme_ids,
        tag_names,
        git_enabled,
    );
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
                        "w | q | wq | open <title> | new <title> | dmode dark|light|system | theme <name> | git enable|commit|push|pull|backup | find <text> | tag <name>",
                    ),
            );
            if state.focus_requested {
                response.request_focus();
                state.focus_requested = false;
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                event = Some(match parse_command(&state.input, plugin_commands) {
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
        assert!(matches!(parse_command("w", &[]), Ok(Command::Save)));
        assert!(matches!(parse_command("write", &[]), Ok(Command::Save)));
    }

    #[test]
    fn parses_short_and_long_quit() {
        assert!(matches!(parse_command("q", &[]), Ok(Command::Quit)));
        assert!(matches!(parse_command("quit", &[]), Ok(Command::Quit)));
    }

    #[test]
    fn parses_save_and_quit_aliases() {
        assert!(matches!(parse_command("wq", &[]), Ok(Command::SaveAndQuit)));
        assert!(matches!(parse_command("x", &[]), Ok(Command::SaveAndQuit)));
    }

    #[test]
    fn parses_open_with_a_multi_word_title() {
        match parse_command("open Opening Scene", &[]) {
            Ok(Command::Open(title)) => assert_eq!(title, "Opening Scene"),
            other => panic!("expected Command::Open, got {}", describe(&other)),
        }
    }

    #[test]
    fn open_without_a_title_is_an_error() {
        assert!(parse_command("open", &[]).is_err());
        assert!(parse_command("o", &[]).is_err());
    }

    #[test]
    fn parses_new_with_a_title() {
        match parse_command("new Chapter 2", &[]) {
            Ok(Command::New(title)) => assert_eq!(title, "Chapter 2"),
            other => panic!("expected Command::New, got {}", describe(&other)),
        }
    }

    #[test]
    fn new_without_a_title_is_an_error() {
        assert!(parse_command("new", &[]).is_err());
    }

    #[test]
    fn parses_dmode_choices() {
        assert!(matches!(
            parse_command("dmode dark", &[]),
            Ok(Command::DarkMode(DarkModeChoice::Dark))
        ));
        assert!(matches!(
            parse_command("dmode light", &[]),
            Ok(Command::DarkMode(DarkModeChoice::Light))
        ));
        assert!(matches!(
            parse_command("dmode system", &[]),
            Ok(Command::DarkMode(DarkModeChoice::System))
        ));
    }

    #[test]
    fn dmode_with_an_invalid_argument_is_an_error() {
        assert!(parse_command("dmode neon", &[]).is_err());
        assert!(parse_command("dmode", &[]).is_err());
    }

    #[test]
    fn parses_theme_by_id() {
        match parse_command("theme dracula", &[]) {
            Ok(Command::ColorTheme(Some(id))) => assert_eq!(id, "dracula"),
            other => panic!("expected Command::ColorTheme, got {}", describe(&other)),
        }
    }

    #[test]
    fn theme_id_is_lowercased() {
        match parse_command("theme Dracula", &[]) {
            Ok(Command::ColorTheme(Some(id))) => assert_eq!(id, "dracula"),
            other => panic!("expected Command::ColorTheme, got {}", describe(&other)),
        }
    }

    #[test]
    fn parses_theme_default_as_none() {
        assert!(matches!(
            parse_command("theme default", &[]),
            Ok(Command::ColorTheme(None))
        ));
        assert!(matches!(
            parse_command("theme Default", &[]),
            Ok(Command::ColorTheme(None))
        ));
    }

    #[test]
    fn theme_without_a_name_is_an_error() {
        assert!(parse_command("theme", &[]).is_err());
    }

    #[test]
    fn parses_git_enable_push_pull() {
        assert!(matches!(
            parse_command("git enable", &[]),
            Ok(Command::Git(GitCommand::Enable))
        ));
        assert!(matches!(
            parse_command("git push", &[]),
            Ok(Command::Git(GitCommand::Push))
        ));
        assert!(matches!(
            parse_command("git pull", &[]),
            Ok(Command::Git(GitCommand::Pull))
        ));
    }

    #[test]
    fn parses_git_commit_without_a_message_as_none() {
        assert!(matches!(
            parse_command("git commit", &[]),
            Ok(Command::Git(GitCommand::Commit(None)))
        ));
    }

    #[test]
    fn parses_git_commit_with_an_inline_message() {
        match parse_command("git commit fixed typo", &[]) {
            Ok(Command::Git(GitCommand::Commit(Some(message)))) => {
                assert_eq!(message, "fixed typo");
            }
            other => panic!(
                "expected Command::Git(Commit(Some(..))), got {}",
                describe(&other)
            ),
        }
    }

    #[test]
    fn parses_git_backup_with_and_without_a_message() {
        assert!(matches!(
            parse_command("git backup", &[]),
            Ok(Command::Git(GitCommand::Backup(None)))
        ));
        match parse_command("git backup end of day", &[]) {
            Ok(Command::Git(GitCommand::Backup(Some(message)))) => {
                assert_eq!(message, "end of day");
            }
            other => panic!(
                "expected Command::Git(Backup(Some(..))), got {}",
                describe(&other)
            ),
        }
    }

    #[test]
    fn git_without_a_subcommand_is_an_error() {
        assert!(parse_command("git", &[]).is_err());
    }

    #[test]
    fn git_with_an_unknown_subcommand_is_an_error_naming_it() {
        match parse_command("git frobnicate", &[]) {
            Err(msg) => assert!(msg.contains("frobnicate")),
            Ok(_) => panic!("expected an error"),
        }
    }

    #[test]
    fn git_subcommand_completes_against_known_subcommands() {
        // Prefix match ("commit") ranks ahead of a mere substring match ("backup"
        // contains "c"), each group sorted alphabetically.
        assert_eq!(
            completions("git c", &[], &[], &[], &[], true),
            vec!["commit", "backup"]
        );
        assert_eq!(
            completions("git pu", &[], &[], &[], &[], true),
            vec!["pull", "push"]
        );
    }

    #[test]
    fn git_message_argument_has_no_completions() {
        assert!(completions("git commit fix", &[], &[], &[], &[], true).is_empty());
    }

    #[test]
    fn git_is_omitted_from_command_name_completions_when_disabled() {
        assert_eq!(completions("gi", &[], &[], &[], &[], false), Vec::<&str>::new());
    }

    #[test]
    fn git_subcommand_has_no_completions_when_disabled() {
        assert!(completions("git c", &[], &[], &[], &[], false).is_empty());
    }

    #[test]
    fn parses_find_with_and_without_a_query() {
        match parse_command("find needle", &[]) {
            Ok(Command::Find(query)) => assert_eq!(query, "needle"),
            other => panic!("expected Command::Find, got {}", describe(&other)),
        }
        assert!(matches!(parse_command("find", &[]), Ok(Command::Find(query)) if query.is_empty()));
    }

    #[test]
    fn parses_tag_with_and_without_a_query() {
        match parse_command("tag foo", &[]) {
            Ok(Command::Tag(tag)) => assert_eq!(tag, "foo"),
            other => panic!("expected Command::Tag, got {}", describe(&other)),
        }
        assert!(matches!(parse_command("tag", &[]), Ok(Command::Tag(tag)) if tag.is_empty()));
    }

    #[test]
    fn a_leading_colon_is_tolerated() {
        assert!(matches!(parse_command(":w", &[]), Ok(Command::Save)));
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert!(matches!(parse_command("  w  ", &[]), Ok(Command::Save)));
    }

    #[test]
    fn empty_input_is_an_error() {
        assert!(parse_command("", &[]).is_err());
        assert!(parse_command("   ", &[]).is_err());
    }

    #[test]
    fn unknown_command_is_an_error_naming_it() {
        match parse_command("frobnicate", &[]) {
            Err(msg) => assert!(msg.contains("frobnicate")),
            Ok(_) => panic!("expected an error"),
        }
    }

    #[test]
    fn a_registered_plugin_command_parses_with_its_argument() {
        let plugins = vec!["wordcount".to_string()];
        match parse_command("wordcount", &plugins) {
            Ok(Command::Plugin(name, arg)) => {
                assert_eq!(name, "wordcount");
                assert_eq!(arg, "");
            }
            other => panic!("expected Command::Plugin, got {}", describe(&other)),
        }
        match parse_command("wordcount extra text", &plugins) {
            Ok(Command::Plugin(name, arg)) => {
                assert_eq!(name, "wordcount");
                assert_eq!(arg, "extra text");
            }
            other => panic!("expected Command::Plugin, got {}", describe(&other)),
        }
    }

    #[test]
    fn a_built_in_command_name_is_never_shadowed_by_a_plugin() {
        // "find" is a real built-in — a plugin claiming the same name must lose.
        let plugins = vec!["find".to_string()];
        assert!(matches!(
            parse_command("find needle", &plugins),
            Ok(Command::Find(query)) if query == "needle"
        ));
    }

    #[test]
    fn an_unregistered_name_is_still_an_unknown_command_error() {
        assert!(parse_command("wordcount", &[]).is_err());
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
        assert_eq!(
            completions("w", &[], &[], &[], &[], true),
            vec!["wq", "write", "new"]
        );
        // Likewise "dmode" (prefix) ahead of "find" (contains "d").
        assert_eq!(completions("d", &[], &[], &[], &[], true), vec!["dmode", "find"]);
    }

    #[test]
    fn command_name_completions_still_include_a_fully_typed_exact_match() {
        // "quit" is itself the only candidate once you've typed the whole word.
        assert_eq!(completions("quit", &[], &[], &[], &[], true), vec!["quit"]);
    }

    #[test]
    fn open_argument_completes_against_note_titles() {
        let titles = vec!["Opening Scene".to_string(), "Backstory".to_string()];
        assert_eq!(
            completions("open open", &titles, &[], &[], &[], true),
            vec!["Opening Scene"]
        );
        assert_eq!(
            completions("o open", &titles, &[], &[], &[], true),
            vec!["Opening Scene"]
        );
    }

    #[test]
    fn open_argument_completions_are_capped_at_max_suggestions() {
        let titles: Vec<String> = (0..20).map(|n| format!("Note {n:02}")).collect();
        let candidates = completions("open Note", &titles, &[], &[], &[], true);
        assert_eq!(candidates.len(), MAX_SUGGESTIONS);
    }

    #[test]
    fn dmode_argument_completes_against_the_fixed_choices() {
        assert_eq!(completions("dmode d", &[], &[], &[], &[], true), vec!["dark"]);
    }

    fn theme_ids_fixture() -> Vec<String> {
        crate::color_theme::built_in_themes()
            .into_iter()
            .map(|theme| theme.id)
            .collect()
    }

    #[test]
    fn theme_argument_completes_against_known_theme_ids() {
        let theme_ids = theme_ids_fixture();
        assert_eq!(
            completions("theme drac", &[], &[], &theme_ids, &[], true),
            vec!["dracula"]
        );
    }

    #[test]
    fn theme_argument_completion_includes_default() {
        assert_eq!(
            completions("theme def", &[], &[], &[], &[], true),
            vec!["default"]
        );
    }

    #[test]
    fn theme_argument_completion_with_an_empty_query_is_capped_and_leads_with_default() {
        let theme_ids = theme_ids_fixture();
        let candidates = completions("theme ", &[], &[], &theme_ids, &[], true);
        assert_eq!(candidates.len(), MAX_SUGGESTIONS);
        assert_eq!(candidates[0], "default");
    }

    #[test]
    fn new_and_find_arguments_have_no_completions() {
        let titles = vec!["Opening Scene".to_string()];
        assert!(completions("new Open", &titles, &[], &[], &[], true).is_empty());
        assert!(completions("find Open", &titles, &[], &[], &[], true).is_empty());
    }

    #[test]
    fn tag_argument_completes_against_known_project_tags() {
        let tags = vec!["projects/smaragd".to_string(), "personal".to_string()];
        assert_eq!(
            completions("tag proj", &[], &[], &[], &tags, true),
            vec!["projects/smaragd"]
        );
    }

    #[test]
    fn tag_argument_with_no_known_tags_has_no_completions() {
        assert!(completions("tag any", &[], &[], &[], &[], true).is_empty());
    }

    #[test]
    fn unrecognized_command_has_no_argument_completions() {
        assert!(completions("frobnicate a", &[], &[], &[], &[], true).is_empty());
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
