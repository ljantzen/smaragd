# The Command Prompt

**`Tools > Command Prompt`** opens a Vim/Helix-style `:` command line. Arguments tab-complete where it makes sense (note titles, theme ids, tag names, plugin command names).

| Command | Effect |
|---|---|
| `:w` / `:write` | Save |
| `:q` / `:quit` | Quit |
| `:wq` / `:x` | Save and quit |
| `:o <title>` / `:open <title>` | Open a document by title |
| `:new <title>` | Create a new document |
| `:dmode <dark\|light\|system>` | Set the dark/light/system appearance |
| `:theme <id>` | Apply a color theme (see [Themes](themes.md)) — no argument clears back to plain dark/light |
| `:find <text>` | Open Find and Replace pre-filled with `<text>` |
| `:tag <name>` | Open [Tags](tags.md) pre-filtered to documents carrying `<name>` |
| `:git enable` | Turn on git support for this project |
| `:git commit [message]` | Commit; prompts for a message if omitted |
| `:git push` | Push |
| `:git pull` | Pull |
| `:git backup [message]` | Commit and push in one step |

Any `:` command a loaded plugin has registered also works here (see [Plugins](plugins.md)) — plugin commands can never override a built-in name.

Every `:git` command (and the `git` entry in this prompt's own autocomplete) does nothing, with a status message pointing at Settings, whenever [git integration](git-integration.md) is turned off app-wide.
