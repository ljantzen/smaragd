# Examples

## A custom `:` command

```rhai
fn say_hello(arg) {
    smaragd_status("Hello, " + arg + "!");
}
register_command("hello", "say_hello");
register_shortcut("hello", "ctrl+shift+h");
```

Typing `:hello world` in the command prompt calls `say_hello("world")` and shows "Hello, world!" in the status bar. Everything after the command name is passed as a single string argument. Pressing `Ctrl+Shift+H` runs the same command with an empty argument.

## Shelling out to a tool

```rhai
fn wordcount(arg) {
    let result = smaragd_run_command("wc", ["-w"]);
    smaragd_status("Words: " + result.stdout);
}
register_command("wordcount", "wordcount");
```

## An `on_save` hook

```rhai
fn on_save(text) {
    text.trim() + "\n"
}
```

This runs before every explicit save (`:w` / `Ctrl+S` / `:wq`), in plugin-load order, each hook's output feeding the next. Return a `String` to replace the saved text; return anything else (typically nothing) to leave it unchanged. If a hook throws, that plugin's change is dropped and an error is shown — a broken plugin can never block a save.

Note: `on_save` only runs on those explicit save actions — not the focus-loss autosave, and not the save-before-switching-documents path.
