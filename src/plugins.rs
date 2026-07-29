//! User-contributed plugins: `.rhai` scripts (the [Rhai](https://rhai.rs) embedded
//! scripting language — pure Rust, with no filesystem/network access of its own)
//! that can register custom `:` commands and an `on_save` text-transform hook.
//!
//! A plugin's top-level script runs once at load time (see [`load`]) and is
//! expected to call the host-provided `register_command(name, fn_name)` for each
//! `:` command it wants to expose. It may also define a function literally named
//! `on_save(text)`, called before every explicit save; returning a `String`
//! replaces the saved text, returning anything else (typically unit `()`) leaves
//! it unchanged.
//!
//! Scripts talk to the app through flat, `smaragd_`-prefixed host functions
//! (deliberately not Rhai's module-namespacing system, which would add API
//! surface with no benefit here): `smaragd_status(msg)`,
//! `smaragd_document_text()`, `smaragd_document_basename()`,
//! `smaragd_document_filename()`, `smaragd_set_document_text(text)`, and
//! `smaragd_run_command(cmd, args)`, which shells out to an arbitrary program
//! on `PATH`. That last one means a loaded plugin has the same reach as anything
//! else run under the user's own account — the trust boundary is loading the
//! plugin at all (see [`load`]'s docs on the global vs. project directories), not
//! anything enforced per call.
//!
//! A registered `:` command can also ask for a default keyboard shortcut via
//! `register_shortcut(name, key_spec)` (e.g. `"ctrl+shift+k"` — see
//! [`parse_shortcut_spec`]), which the app's Settings window then lets the user
//! remap or unbind exactly like a built-in shortcut — see `app.rs`'s
//! `compute_effective_plugin_shortcuts`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use egui::{Key, KeyboardShortcut, Modifiers};
use rhai::{AST, Array, Dynamic, Engine, EvalAltResult, Map, Scope};

use crate::shortcuts::is_safe_binding;

/// The global, always-loaded plugin directory: `<config_dir>/smaragd/plugins`,
/// the same base path `settings::config_file_path` uses for `smaragd.toml`.
/// `None` if the platform's config directory can't be determined.
pub fn global_plugins_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "smaragd").map(|dirs| dirs.config_dir().join("plugins"))
}

/// The live values a running plugin function reads/writes, shared with the
/// registered host functions via an `Rc<RefCell<_>>` closed over when the
/// `Engine` is built — sidesteps giving a Rhai closure a borrow of live app state
/// across the callback boundary. The caller (`app.rs`) populates `document_text`
/// before a call and drains `status_message`/`set_document_text` after.
#[derive(Default)]
struct PluginIo {
    document_text: Option<String>,
    /// The open document's file name, stripped of its `.md` extension (`None` if
    /// none is open) — backs `smaragd_document_basename()`. Deliberately just
    /// the name, not a full path: a plugin has no way to interpret an absolute
    /// path meaningfully anyway (it can't do path arithmetic — Rhai has no path
    /// library registered), and the name is what a user-facing use like a log
    /// entry actually wants.
    document_basename: Option<String>,
    /// The open document's path relative to the project root, `.md` extension
    /// included (`None` if none is open) — backs `smaragd_document_filename()`.
    /// Relative to the *project*, not the filesystem root, for the same reason
    /// `document_basename` isn't a full absolute path: it's the only form of
    /// "full name" a plugin could meaningfully do anything with (log it, compare
    /// it against another project-relative path, etc.).
    document_filename: Option<String>,
    status_message: Option<String>,
    set_document_text: Option<String>,
}

/// What a plugin call produced, handed back to the caller to apply to real app
/// state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginEffects {
    pub status_message: Option<String>,
    pub set_document_text: Option<String>,
}

struct LoadedPlugin {
    /// The file's stem (no `.rhai`), used in error/conflict messages.
    name: String,
    ast: AST,
    /// `:` command name -> the script function it invokes.
    commands: HashMap<String, String>,
    has_on_save: bool,
    /// `:` command name -> the default shortcut it asked for via
    /// `register_shortcut`, if any and if it didn't lose a conflict at load time.
    shortcuts: HashMap<String, KeyboardShortcut>,
}

/// A loaded set of plugins, ready to run commands/hooks against. Build with
/// [`load`].
pub struct PluginEngine {
    engine: Engine,
    plugins: Vec<LoadedPlugin>,
    io: Rc<RefCell<PluginIo>>,
}

impl Default for PluginEngine {
    /// An engine with no plugins loaded — `SmaragdApp::new` needs an initial
    /// value to construct itself with before it knows which directories to load
    /// from; `reload_plugins` replaces this immediately after.
    fn default() -> Self {
        load(&[], None).0
    }
}

impl PluginEngine {
    /// Every loaded plugin command's default keyboard shortcut, as (command name,
    /// shortcut) pairs — `:` command names are already globally unique across
    /// loaded plugins (see `load`'s conflict handling), so no plugin-name
    /// qualification is needed here. `app.rs`'s `compute_effective_plugin_shortcuts`
    /// layers the user's Settings overrides and built-in-shortcut conflicts on top
    /// of this to get what's actually active.
    pub fn shortcut_defaults(&self) -> impl Iterator<Item = (&str, KeyboardShortcut)> {
        self.plugins.iter().flat_map(|plugin| {
            plugin
                .shortcuts
                .iter()
                .map(|(name, shortcut)| (name.as_str(), *shortcut))
        })
    }

    /// The `:` command names every loaded plugin registered, for
    /// `command_prompt.rs`'s parser and tab-completion.
    pub fn command_names(&self) -> impl Iterator<Item = &str> {
        self.plugins
            .iter()
            .flat_map(|plugin| plugin.commands.keys().map(String::as_str))
    }

    /// Run the plugin command registered as `name` with argument `arg`, giving it
    /// `document_text` (the open document's live buffer, if any) to read via
    /// `smaragd_document_text()`, `document_basename` (that document's file
    /// name minus its `.md` extension) via `smaragd_document_basename()`, and
    /// `document_filename` (its path relative to the project root, `.md`
    /// included) via `smaragd_document_filename()`. Returns the effects the
    /// call produced (status message / a new document text) plus `Err` if the
    /// call itself failed — callers should show that as a status message and
    /// otherwise ignore it: a broken plugin command must never corrupt app
    /// state.
    pub fn run_command(
        &self,
        name: &str,
        arg: &str,
        document_text: Option<&str>,
        document_basename: Option<&str>,
        document_filename: Option<&str>,
    ) -> (PluginEffects, Result<(), String>) {
        let Some(plugin) = self
            .plugins
            .iter()
            .find(|plugin| plugin.commands.contains_key(name))
        else {
            return (
                PluginEffects::default(),
                Err(format!("No plugin registered \":{name}\"")),
            );
        };
        let fn_name = &plugin.commands[name];

        *self.io.borrow_mut() = PluginIo {
            document_text: document_text.map(str::to_string),
            document_basename: document_basename.map(str::to_string),
            document_filename: document_filename.map(str::to_string),
            ..Default::default()
        };

        let mut scope = Scope::new();
        let result = self
            .engine
            .call_fn::<Dynamic>(&mut scope, &plugin.ast, fn_name, (arg.to_string(),))
            .map(|_| ())
            .map_err(|err| format!("{}: {err}", plugin.name));

        let io = self.io.take();
        (
            PluginEffects {
                status_message: io.status_message,
                set_document_text: io.set_document_text,
            },
            result,
        )
    }

    /// Run every loaded plugin's `on_save(text)`, in load order, threading the
    /// text through each — a plugin that doesn't define `on_save` is skipped, and
    /// one whose call errors just leaves the text as that plugin received it
    /// (appending a message to the returned error list) rather than blocking the
    /// save or corrupting the buffer.
    pub fn run_on_save(&self, text: &str) -> (String, Vec<String>) {
        let mut text = text.to_string();
        let mut errors = Vec::new();

        for plugin in &self.plugins {
            if !plugin.has_on_save {
                continue;
            }
            let mut scope = Scope::new();
            match self.engine.call_fn::<Dynamic>(
                &mut scope,
                &plugin.ast,
                "on_save",
                (text.clone(),),
            ) {
                Ok(result) => {
                    if let Ok(new_text) = result.into_string() {
                        text = new_text;
                    }
                    // Anything else (typically unit `()`) means "unchanged".
                }
                Err(err) => errors.push(format!("{}: on_save failed: {err}", plugin.name)),
            }
        }

        (text, errors)
    }
}

fn new_engine(
    io: &Rc<RefCell<PluginIo>>,
    pending_commands: &Rc<RefCell<Vec<(String, String)>>>,
    pending_shortcuts: &Rc<RefCell<Vec<(String, String)>>>,
    working_dir: Option<PathBuf>,
) -> Engine {
    let mut engine = Engine::new();

    let io_for_status = Rc::clone(io);
    engine.register_fn("smaragd_status", move |msg: &str| {
        io_for_status.borrow_mut().status_message = Some(msg.to_string());
    });

    let io_for_read = Rc::clone(io);
    engine.register_fn("smaragd_document_text", move || -> String {
        io_for_read
            .borrow()
            .document_text
            .clone()
            .unwrap_or_default()
    });

    let io_for_basename = Rc::clone(io);
    engine.register_fn("smaragd_document_basename", move || -> String {
        io_for_basename
            .borrow()
            .document_basename
            .clone()
            .unwrap_or_default()
    });

    let io_for_filename = Rc::clone(io);
    engine.register_fn("smaragd_document_filename", move || -> String {
        io_for_filename
            .borrow()
            .document_filename
            .clone()
            .unwrap_or_default()
    });

    let io_for_write = Rc::clone(io);
    engine.register_fn("smaragd_set_document_text", move |text: &str| {
        io_for_write.borrow_mut().set_document_text = Some(text.to_string());
    });

    let commands = Rc::clone(pending_commands);
    engine.register_fn("register_command", move |name: &str, fn_name: &str| {
        commands
            .borrow_mut()
            .push((name.to_string(), fn_name.to_string()));
    });

    let shortcuts = Rc::clone(pending_shortcuts);
    engine.register_fn("register_shortcut", move |name: &str, key_spec: &str| {
        shortcuts
            .borrow_mut()
            .push((name.to_string(), key_spec.to_string()));
    });

    engine.register_fn(
        "smaragd_run_command",
        move |cmd: &str, args: Array| -> Result<Map, Box<EvalAltResult>> {
            run_command(cmd, args, working_dir.as_deref())
        },
    );

    engine
}

/// Backs the `smaragd_run_command(cmd, args)` host function: runs `cmd` with
/// `args` (each coerced to a string; a non-string element is a script bug, so that
/// errors out like any other type mismatch) in `working_dir` (the open project's
/// root, if any — `None` inherits the app's own working directory), waiting for it
/// to exit. Returns a map with `stdout`/`stderr` (lossily decoded, untrimmed) and
/// `exit_code`/`success`, mirroring `std::process::Output` rather than trying to
/// interpret failure for the caller: a non-zero exit is a normal, scriptable
/// outcome. Only spawning itself (e.g. `cmd` not found) raises a Rhai error, since
/// that's the one failure a script can't meaningfully branch on.
fn run_command(
    cmd: &str,
    args: Array,
    working_dir: Option<&Path>,
) -> Result<Map, Box<EvalAltResult>> {
    let args = args
        .into_iter()
        .map(|arg| arg.into_string())
        .collect::<Result<Vec<_>, _>>()?;

    let mut command = Command::new(cmd);
    command.args(args);
    if let Some(dir) = working_dir {
        command.current_dir(dir);
    }

    let output = command
        .output()
        .map_err(|err| format!("couldn't run '{cmd}': {err}"))?;

    let mut result = Map::new();
    result.insert(
        "stdout".into(),
        String::from_utf8_lossy(&output.stdout).into_owned().into(),
    );
    result.insert(
        "stderr".into(),
        String::from_utf8_lossy(&output.stderr).into_owned().into(),
    );
    result.insert(
        "exit_code".into(),
        (output.status.code().unwrap_or(-1) as i64).into(),
    );
    result.insert("success".into(), output.status.success().into());
    Ok(result)
}

/// Parses a `register_shortcut` key spec like `"ctrl+shift+k"` into a
/// `KeyboardShortcut`: `+`-separated tokens, modifiers first, the key name last.
/// Modifier names are case-insensitive (`ctrl`/`cmd`/`command` all map to
/// `Modifiers::COMMAND` — Ctrl on Windows/Linux, Cmd on macOS — matching the
/// convention `shortcuts::ShortcutAction::default_shortcut` already uses;
/// `shift`, `alt`/`option`). The key name matches `egui::Key::from_name` (e.g.
/// `"K"`, `"F2"`, `"Enter"`, `"Colon"`), tried both as given and capitalized, so a
/// script can write either `"Enter"` or `"enter"`.
fn parse_shortcut_spec(spec: &str) -> Result<KeyboardShortcut, String> {
    let mut parts: Vec<&str> = spec.split('+').map(str::trim).collect();
    let Some(key_part) = parts.pop().filter(|part| !part.is_empty()) else {
        return Err(format!("empty shortcut spec: {spec:?}"));
    };

    let mut modifiers = Modifiers::NONE;
    for part in parts {
        modifiers |= match part.to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" | "command" => Modifiers::COMMAND,
            "shift" => Modifiers::SHIFT,
            "alt" | "option" => Modifiers::ALT,
            other => return Err(format!("unknown modifier {other:?} in shortcut {spec:?}")),
        };
    }

    let capitalized = {
        let mut chars = key_part.chars();
        match chars.next() {
            Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str().to_lowercase()),
            None => String::new(),
        }
    };
    let key = Key::from_name(key_part)
        .or_else(|| Key::from_name(&capitalized))
        .ok_or_else(|| format!("unknown key {key_part:?} in shortcut {spec:?}"))?;

    Ok(KeyboardShortcut::new(modifiers, key))
}

/// Load every `*.rhai` file found directly inside each of `dirs` (flat, not
/// recursive; a missing directory is silently skipped, not an error — global
/// plugins usually won't exist until a user creates the folder). Directories are
/// scanned in the order given and files within a directory in sorted-name order,
/// so load (and therefore command-conflict-resolution) order is deterministic.
///
/// Never fails outright: a plugin that doesn't compile, doesn't run, or tries to
/// register a `:` command another already-loaded plugin owns is skipped, with a
/// message describing why appended to the returned list, rather than the whole
/// load failing.
///
/// `working_dir` is where `smaragd_run_command` runs a plugin's shell commands
/// (the open project's root, or `None` to inherit the app's own working
/// directory) — it doesn't affect where `.rhai` files themselves are read from,
/// that's `dirs`.
pub fn load(dirs: &[&Path], working_dir: Option<&Path>) -> (PluginEngine, Vec<String>) {
    let io = Rc::new(RefCell::new(PluginIo::default()));
    let pending_commands = Rc::new(RefCell::new(Vec::new()));
    let pending_shortcuts = Rc::new(RefCell::new(Vec::new()));
    let engine = new_engine(
        &io,
        &pending_commands,
        &pending_shortcuts,
        working_dir.map(Path::to_path_buf),
    );

    let mut plugins = Vec::new();
    let mut errors = Vec::new();
    let mut command_owners: HashMap<String, String> = HashMap::new();
    let mut shortcut_owners: HashMap<KeyboardShortcut, String> = HashMap::new();

    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let mut paths: Vec<_> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rhai"))
            .collect();
        paths.sort();

        for path in paths {
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("plugin")
                .to_string();

            let source = match fs::read_to_string(&path) {
                Ok(source) => source,
                Err(err) => {
                    errors.push(format!("{name}: couldn't read file: {err}"));
                    continue;
                }
            };
            let ast = match engine.compile(&source) {
                Ok(ast) => ast,
                Err(err) => {
                    errors.push(format!("{name}: {err}"));
                    continue;
                }
            };
            let has_on_save = ast
                .iter_functions()
                .any(|meta| meta.name == "on_save" && meta.params.len() == 1);

            pending_commands.borrow_mut().clear();
            pending_shortcuts.borrow_mut().clear();
            let mut scope = Scope::new();
            if let Err(err) = engine.run_ast_with_scope(&mut scope, &ast) {
                errors.push(format!("{name}: {err}"));
                continue;
            }

            let mut commands = HashMap::new();
            for (command_name, fn_name) in pending_commands.borrow_mut().drain(..) {
                if let Some(owner) = command_owners.get(&command_name) {
                    errors.push(format!(
                        "{name}: \":{command_name}\" is already registered by {owner}, skipping"
                    ));
                    continue;
                }
                command_owners.insert(command_name.clone(), name.clone());
                commands.insert(command_name, fn_name);
            }

            let mut shortcuts = HashMap::new();
            for (command_name, key_spec) in pending_shortcuts.borrow_mut().drain(..) {
                if !commands.contains_key(&command_name) {
                    errors.push(format!(
                        "{name}: register_shortcut(\"{command_name}\", ..) doesn't match a command \
                         this plugin registered, skipping"
                    ));
                    continue;
                }
                let shortcut = match parse_shortcut_spec(&key_spec) {
                    Ok(shortcut) => shortcut,
                    Err(err) => {
                        errors.push(format!("{name}: {err}"));
                        continue;
                    }
                };
                if !is_safe_binding(&shortcut) {
                    errors.push(format!(
                        "{name}: shortcut {key_spec:?} for \":{command_name}\" needs Ctrl, Alt, or \
                         Shift (function keys and Escape are exempt), skipping"
                    ));
                    continue;
                }
                if let Some(owner) = shortcut_owners.get(&shortcut) {
                    errors.push(format!(
                        "{name}: shortcut {key_spec:?} for \":{command_name}\" is already used by \
                         {owner}, leaving \":{command_name}\" unbound"
                    ));
                    continue;
                }
                shortcut_owners.insert(shortcut, format!("{name}:{command_name}"));
                shortcuts.insert(command_name, shortcut);
            }

            plugins.push(LoadedPlugin {
                name,
                ast,
                commands,
                has_on_save,
                shortcuts,
            });
        }
    }

    (
        PluginEngine {
            engine,
            plugins,
            io,
        },
        errors,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_plugin(dir: &Path, name: &str, source: &str) {
        fs::write(dir.join(format!("{name}.rhai")), source).unwrap();
    }

    #[test]
    fn a_plugin_can_register_and_run_a_command() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "greet",
            r#"
                fn say_hello(arg) {
                    smaragd_status("Hello, " + arg + "!");
                }
                register_command("hello", "say_hello");
            "#,
        );

        let (engine, errors) = load(&[dir.path()], None);
        assert!(errors.is_empty(), "unexpected load errors: {errors:?}");
        assert_eq!(engine.command_names().collect::<Vec<_>>(), vec!["hello"]);

        let (effects, result) = engine.run_command("hello", "world", None, None, None);
        assert!(result.is_ok());
        assert_eq!(effects.status_message.as_deref(), Some("Hello, world!"));
    }

    #[test]
    fn a_command_can_read_and_replace_the_document_text() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "shout",
            r#"
                fn shout(arg) {
                    smaragd_set_document_text(smaragd_document_text().to_upper());
                }
                register_command("shout", "shout");
            "#,
        );

        let (engine, _) = load(&[dir.path()], None);
        let (effects, result) = engine.run_command("shout", "", Some("hello there"), None, None);
        assert!(result.is_ok());
        assert_eq!(effects.set_document_text.as_deref(), Some("HELLO THERE"));
    }

    #[test]
    fn a_command_can_read_the_document_basename_and_filename() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "whoami",
            r#"
                fn whoami(arg) {
                    smaragd_status(smaragd_document_basename() + "|" + smaragd_document_filename());
                }
                register_command("whoami", "whoami");
            "#,
        );

        let (engine, _) = load(&[dir.path()], None);
        let (effects, result) = engine.run_command(
            "whoami",
            "",
            None,
            Some("Scene 5"),
            Some("Part 1/Scene 5.md"),
        );
        assert!(result.is_ok());
        assert_eq!(
            effects.status_message.as_deref(),
            Some("Scene 5|Part 1/Scene 5.md")
        );
    }

    #[test]
    fn the_document_basename_and_filename_are_empty_when_no_document_is_open() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "whoami",
            r#"
                fn whoami(arg) {
                    smaragd_status(smaragd_document_basename() + "|" + smaragd_document_filename());
                }
                register_command("whoami", "whoami");
            "#,
        );

        let (engine, _) = load(&[dir.path()], None);
        let (effects, result) = engine.run_command("whoami", "", None, None, None);
        assert!(result.is_ok());
        assert_eq!(effects.status_message.as_deref(), Some("|"));
    }

    #[test]
    fn running_an_unregistered_command_is_an_error() {
        let (engine, _) = load(&[], None);
        let (_, result) = engine.run_command("nope", "", None, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn two_plugins_racing_for_the_same_command_name_keeps_the_first() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "a_first",
            r#"
                fn one(arg) { smaragd_status("first"); }
                register_command("dup", "one");
            "#,
        );
        write_plugin(
            dir.path(),
            "b_second",
            r#"
                fn two(arg) { smaragd_status("second"); }
                register_command("dup", "two");
            "#,
        );

        let (engine, errors) = load(&[dir.path()], None);
        assert!(errors.iter().any(|e| e.contains("already registered")));

        let (effects, result) = engine.run_command("dup", "", None, None, None);
        assert!(result.is_ok());
        assert_eq!(effects.status_message.as_deref(), Some("first"));
    }

    #[test]
    fn a_syntax_error_in_one_plugin_does_not_prevent_another_from_loading() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(dir.path(), "broken", "fn oops( {");
        write_plugin(
            dir.path(),
            "fine",
            r#"
                fn ok_fn(arg) { smaragd_status("ok"); }
                register_command("ok", "ok_fn");
            "#,
        );

        let (engine, errors) = load(&[dir.path()], None);
        assert!(errors.iter().any(|e| e.starts_with("broken:")));
        assert_eq!(engine.command_names().collect::<Vec<_>>(), vec!["ok"]);
    }

    #[test]
    fn on_save_transforms_the_saved_text() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "trim",
            r#"
                fn on_save(text) {
                    text.trim();
                    text + "\n"
                }
            "#,
        );

        let (engine, errors) = load(&[dir.path()], None);
        assert!(errors.is_empty());
        let (text, errors) = engine.run_on_save("  hello world  \n\n");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(text, "hello world\n");
    }

    #[test]
    fn on_save_returning_unit_leaves_the_text_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "noop",
            r#"
                fn on_save(text) {
                    smaragd_status("saved");
                }
            "#,
        );

        let (engine, _) = load(&[dir.path()], None);
        let (text, errors) = engine.run_on_save("unchanged");
        assert!(errors.is_empty());
        assert_eq!(text, "unchanged");
    }

    #[test]
    fn on_save_erroring_leaves_the_text_as_that_plugin_received_it() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "broken_hook",
            r#"
                fn on_save(text) {
                    throw "boom";
                }
            "#,
        );

        let (engine, _) = load(&[dir.path()], None);
        let (text, errors) = engine.run_on_save("unchanged");
        assert_eq!(text, "unchanged");
        assert!(errors.iter().any(|e| e.contains("boom")));
    }

    #[test]
    fn a_directory_with_no_rhai_files_loads_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, errors) = load(&[dir.path()], None);
        assert!(errors.is_empty());
        assert_eq!(engine.command_names().count(), 0);
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        let (engine, errors) = load(&[Path::new("/does/not/exist")], None);
        assert!(errors.is_empty());
        assert_eq!(engine.command_names().count(), 0);
    }

    #[test]
    fn a_command_can_run_a_shell_command_and_read_its_output() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "shell",
            r#"
                fn run(arg) {
                    let result = smaragd_run_command("printf", ["hello %s", arg]);
                    smaragd_status(result.stdout);
                }
                register_command("shell", "run");
            "#,
        );

        let (engine, errors) = load(&[dir.path()], None);
        assert!(errors.is_empty(), "unexpected load errors: {errors:?}");

        let (effects, result) = engine.run_command("shell", "world", None, None, None);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(effects.status_message.as_deref(), Some("hello world"));
    }

    #[test]
    fn a_shell_command_reports_a_non_zero_exit_without_erroring() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "shell",
            r#"
                fn run(arg) {
                    let result = smaragd_run_command("false", []);
                    smaragd_status(`exit=${result.exit_code} success=${result.success}`);
                }
                register_command("shell", "run");
            "#,
        );

        let (engine, _) = load(&[dir.path()], None);
        let (effects, result) = engine.run_command("shell", "", None, None, None);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            effects.status_message.as_deref(),
            Some("exit=1 success=false")
        );
    }

    #[test]
    fn a_shell_command_that_cannot_spawn_is_a_script_error() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "shell",
            r#"
                fn run(arg) {
                    smaragd_run_command("definitely-not-a-real-binary", []);
                }
                register_command("shell", "run");
            "#,
        );

        let (engine, _) = load(&[dir.path()], None);
        let (_, result) = engine.run_command("shell", "", None, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn a_shell_command_runs_in_the_configured_working_directory() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "shell",
            r#"
                fn run(arg) {
                    let result = smaragd_run_command("pwd", []);
                    let out = result.stdout;
                    out.trim();
                    smaragd_status(out);
                }
                register_command("shell", "run");
            "#,
        );

        let (engine, _) = load(&[dir.path()], Some(dir.path()));
        let (effects, result) = engine.run_command("shell", "", None, None, None);
        assert!(result.is_ok(), "{result:?}");
        // Canonicalize both sides: on macOS `pwd` reports a `/private/...`-resolved
        // path for a tempdir under a symlinked `/tmp`.
        assert_eq!(
            effects.status_message.map(PathBuf::from),
            Some(dir.path().canonicalize().unwrap())
        );
    }

    #[test]
    fn parse_shortcut_spec_accepts_modifiers_and_a_key_in_any_case() {
        assert_eq!(
            parse_shortcut_spec("ctrl+shift+k").unwrap(),
            KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::K)
        );
        assert_eq!(
            parse_shortcut_spec("Alt+Enter").unwrap(),
            KeyboardShortcut::new(Modifiers::ALT, Key::Enter)
        );
        assert_eq!(
            parse_shortcut_spec("F2").unwrap(),
            KeyboardShortcut::new(Modifiers::NONE, Key::F2)
        );
    }

    #[test]
    fn parse_shortcut_spec_rejects_an_unknown_modifier_or_key() {
        assert!(parse_shortcut_spec("hyper+k").is_err());
        assert!(parse_shortcut_spec("ctrl+not_a_key").is_err());
    }

    #[test]
    fn a_command_can_register_a_default_shortcut() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "wordcount",
            r#"
                fn run(arg) { smaragd_status("ran"); }
                register_command("wordcount", "run");
                register_shortcut("wordcount", "ctrl+shift+w");
            "#,
        );

        let (engine, errors) = load(&[dir.path()], None);
        assert!(errors.is_empty(), "unexpected load errors: {errors:?}");
        assert_eq!(
            engine.shortcut_defaults().collect::<Vec<_>>(),
            vec![(
                "wordcount",
                KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::W)
            )]
        );
    }

    #[test]
    fn a_shortcut_for_an_unregistered_command_is_a_load_error() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "broken",
            r#"
                fn run(arg) { }
                register_command("real", "run");
                register_shortcut("typo", "ctrl+k");
            "#,
        );

        let (engine, errors) = load(&[dir.path()], None);
        assert!(errors.iter().any(|e| e.contains("typo")));
        assert_eq!(engine.shortcut_defaults().count(), 0);
    }

    #[test]
    fn an_unsafe_shortcut_is_rejected_but_the_command_still_works() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "unsafe_shortcut",
            r#"
                fn run(arg) { smaragd_status("ran"); }
                register_command("run_it", "run");
                register_shortcut("run_it", "k");
            "#,
        );

        let (engine, errors) = load(&[dir.path()], None);
        assert!(errors.iter().any(|e| e.contains("Ctrl, Alt, or Shift")));
        assert_eq!(engine.shortcut_defaults().count(), 0);

        let (_, result) = engine.run_command("run_it", "", None, None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn two_plugins_racing_for_the_same_shortcut_keeps_the_first() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "a_first",
            r#"
                fn run(arg) { }
                register_command("first_cmd", "run");
                register_shortcut("first_cmd", "ctrl+k");
            "#,
        );
        write_plugin(
            dir.path(),
            "b_second",
            r#"
                fn run(arg) { }
                register_command("second_cmd", "run");
                register_shortcut("second_cmd", "ctrl+k");
            "#,
        );

        let (engine, errors) = load(&[dir.path()], None);
        assert!(errors.iter().any(|e| e.contains("already used by")));
        assert_eq!(
            engine.shortcut_defaults().collect::<Vec<_>>(),
            vec![(
                "first_cmd",
                KeyboardShortcut::new(Modifiers::COMMAND, Key::K)
            )]
        );
    }
}
