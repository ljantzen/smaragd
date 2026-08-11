# Host Functions Available to a Script

- `smaragd_status(msg)` — show `msg` in the status bar
- `smaragd_document_text()` — returns the open document's current text
- `smaragd_document_basename()` — returns the open document's file name (without its `.md` extension), or an empty string if nothing's open
- `smaragd_document_filename()` — returns the open document's path relative to the project root, `.md` extension included (e.g. `Part 1/Scene 5.md`), or an empty string if nothing's open
- `smaragd_set_document_text(text)` — replaces the open document's text
- `smaragd_run_command(cmd, args)` — runs `cmd` (an array of string `args`) as a subprocess, waits for it to finish, and returns a map with `stdout`, `stderr`, `exit_code`, and `success`. Runs in the open project's root, and blocks the app's UI until the process exits — avoid anything long-running.
- `register_command(name, fn_name)` — called once at script load time to expose a `:` command
- `register_shortcut(name, key_spec)` — called at script load time to give a registered `:` command a default keyboard shortcut, e.g. `register_shortcut("hello", "ctrl+shift+h")`. `key_spec` is `+`-separated modifiers (`ctrl`/`cmd`/`command`, `shift`, `alt`/`option` — case-insensitive) followed by a key name (`k`, `F2`, `Enter`, `Colon`, ...). A bare key with no modifier is rejected unless it's a function key or Escape, same rule as built-in shortcuts.

Whatever shortcut a script asks for is just a *default*: **`File > Settings`** lists every plugin command that registered one, alongside the built-in shortcuts, and lets you remap or unbind it exactly the same way. If a script's requested combo is already in use by a built-in action or another plugin command, it's simply left unbound (with a message explaining why) rather than stealing it — you can still assign it a free combo yourself from Settings.
