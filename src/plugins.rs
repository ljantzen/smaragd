//! User-contributed plugins: `.rhai` scripts (the [Rhai](https://rhai.rs) embedded
//! scripting language — pure Rust, and with none of Rhai's own no filesystem/
//! network/process APIs registered here, so the sandbox is simply "we never expose
//! one," not something this module has to build or maintain) that can register
//! custom `:` commands and an `on_save` text-transform hook.
//!
//! A plugin's top-level script runs once at load time (see [`load`]) and is
//! expected to call the host-provided `register_command(name, fn_name)` for each
//! `:` command it wants to expose. It may also define a function literally named
//! `on_save(text)`, called before every explicit save; returning a `String`
//! replaces the saved text, returning anything else (typically unit `()`) leaves
//! it unchanged.
//!
//! Scripts talk to the app through three flat, `tachylite_`-prefixed host
//! functions (deliberately not Rhai's module-namespacing system, which would add
//! API surface with no benefit here): `tachylite_status(msg)`,
//! `tachylite_document_text()`, and `tachylite_set_document_text(text)`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rhai::{AST, Dynamic, Engine, Scope};

/// The global, always-loaded plugin directory: `<config_dir>/tachylite/plugins`,
/// the same base path `settings::config_file_path` uses for `tachylite.toml`.
/// `None` if the platform's config directory can't be determined.
pub fn global_plugins_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "tachylite")
        .map(|dirs| dirs.config_dir().join("plugins"))
}

/// The live values a running plugin function reads/writes, shared with the
/// registered host functions via an `Rc<RefCell<_>>` closed over when the
/// `Engine` is built — sidesteps giving a Rhai closure a borrow of live app state
/// across the callback boundary. The caller (`app.rs`) populates `document_text`
/// before a call and drains `status_message`/`set_document_text` after.
#[derive(Default)]
struct PluginIo {
    document_text: Option<String>,
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
}

/// A loaded set of plugins, ready to run commands/hooks against. Build with
/// [`load`].
pub struct PluginEngine {
    engine: Engine,
    plugins: Vec<LoadedPlugin>,
    io: Rc<RefCell<PluginIo>>,
}

impl Default for PluginEngine {
    /// An engine with no plugins loaded — `TachyliteApp::new` needs an initial
    /// value to construct itself with before it knows which directories to load
    /// from; `reload_plugins` replaces this immediately after.
    fn default() -> Self {
        load(&[]).0
    }
}

impl PluginEngine {
    /// The `:` command names every loaded plugin registered, for
    /// `command_prompt.rs`'s parser and tab-completion.
    pub fn command_names(&self) -> impl Iterator<Item = &str> {
        self.plugins
            .iter()
            .flat_map(|plugin| plugin.commands.keys().map(String::as_str))
    }

    /// Run the plugin command registered as `name` with argument `arg`, giving it
    /// `document_text` (the open document's live buffer, if any) to read via
    /// `tachylite_document_text()`. Returns the effects the call produced (status
    /// message / a new document text) plus `Err` if the call itself failed —
    /// callers should show that as a status message and otherwise ignore it: a
    /// broken plugin command must never corrupt app state.
    pub fn run_command(
        &self,
        name: &str,
        arg: &str,
        document_text: Option<&str>,
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
) -> Engine {
    let mut engine = Engine::new();

    let io_for_status = Rc::clone(io);
    engine.register_fn("tachylite_status", move |msg: &str| {
        io_for_status.borrow_mut().status_message = Some(msg.to_string());
    });

    let io_for_read = Rc::clone(io);
    engine.register_fn("tachylite_document_text", move || -> String {
        io_for_read
            .borrow()
            .document_text
            .clone()
            .unwrap_or_default()
    });

    let io_for_write = Rc::clone(io);
    engine.register_fn("tachylite_set_document_text", move |text: &str| {
        io_for_write.borrow_mut().set_document_text = Some(text.to_string());
    });

    let commands = Rc::clone(pending_commands);
    engine.register_fn("register_command", move |name: &str, fn_name: &str| {
        commands
            .borrow_mut()
            .push((name.to_string(), fn_name.to_string()));
    });

    engine
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
pub fn load(dirs: &[&Path]) -> (PluginEngine, Vec<String>) {
    let io = Rc::new(RefCell::new(PluginIo::default()));
    let pending_commands = Rc::new(RefCell::new(Vec::new()));
    let engine = new_engine(&io, &pending_commands);

    let mut plugins = Vec::new();
    let mut errors = Vec::new();
    let mut command_owners: HashMap<String, String> = HashMap::new();

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

            plugins.push(LoadedPlugin {
                name,
                ast,
                commands,
                has_on_save,
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
                    tachylite_status("Hello, " + arg + "!");
                }
                register_command("hello", "say_hello");
            "#,
        );

        let (engine, errors) = load(&[dir.path()]);
        assert!(errors.is_empty(), "unexpected load errors: {errors:?}");
        assert_eq!(engine.command_names().collect::<Vec<_>>(), vec!["hello"]);

        let (effects, result) = engine.run_command("hello", "world", None);
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
                    tachylite_set_document_text(tachylite_document_text().to_upper());
                }
                register_command("shout", "shout");
            "#,
        );

        let (engine, _) = load(&[dir.path()]);
        let (effects, result) = engine.run_command("shout", "", Some("hello there"));
        assert!(result.is_ok());
        assert_eq!(effects.set_document_text.as_deref(), Some("HELLO THERE"));
    }

    #[test]
    fn running_an_unregistered_command_is_an_error() {
        let (engine, _) = load(&[]);
        let (_, result) = engine.run_command("nope", "", None);
        assert!(result.is_err());
    }

    #[test]
    fn two_plugins_racing_for_the_same_command_name_keeps_the_first() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(
            dir.path(),
            "a_first",
            r#"
                fn one(arg) { tachylite_status("first"); }
                register_command("dup", "one");
            "#,
        );
        write_plugin(
            dir.path(),
            "b_second",
            r#"
                fn two(arg) { tachylite_status("second"); }
                register_command("dup", "two");
            "#,
        );

        let (engine, errors) = load(&[dir.path()]);
        assert!(errors.iter().any(|e| e.contains("already registered")));

        let (effects, result) = engine.run_command("dup", "", None);
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
                fn ok_fn(arg) { tachylite_status("ok"); }
                register_command("ok", "ok_fn");
            "#,
        );

        let (engine, errors) = load(&[dir.path()]);
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

        let (engine, errors) = load(&[dir.path()]);
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
                    tachylite_status("saved");
                }
            "#,
        );

        let (engine, _) = load(&[dir.path()]);
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

        let (engine, _) = load(&[dir.path()]);
        let (text, errors) = engine.run_on_save("unchanged");
        assert_eq!(text, "unchanged");
        assert!(errors.iter().any(|e| e.contains("boom")));
    }

    #[test]
    fn a_directory_with_no_rhai_files_loads_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, errors) = load(&[dir.path()]);
        assert!(errors.is_empty());
        assert_eq!(engine.command_names().count(), 0);
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        let (engine, errors) = load(&[Path::new("/does/not/exist")]);
        assert!(errors.is_empty());
        assert_eq!(engine.command_names().count(), 0);
    }
}
