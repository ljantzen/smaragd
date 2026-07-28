// Logs a quick entry to today's Obsidian daily note via the `obsidian-logging`
// CLI (~/.config/obsidian-logging/obsidian-logging.yaml controls the vault,
// format, and any predefined phrases — nothing about that is configured here).
//
// :log Started revising Chapter 3   -> adds "Started revising Chapter 3"
// :log (or Ctrl+Shift+L)            -> adds the currently open document's name,
//                                      or "no file" if nothing is open
fn log_entry(arg) {
    let entry = if arg == "" {
        let name = tachylite_document_filename();
        if name == "" { "no file" } else {"Working in tachylite with: " + name }
    } else {
        arg
    };
    let result = tachylite_run_command("obsidian-logging", ["-s", entry]);
    if result.success {
        tachylite_status("Logged: " + entry);
    } else {
        // Rhai's `trim()` mutates in place and returns `()` — it has to be its
        // own statement on a variable, not chained in an expression.
        let err = result.stderr;
        err.trim();
        tachylite_status("obsidian-logging failed: " + err);
    }
}

register_command("log", "log_entry");
register_shortcut("log", "ctrl+shift+l");
