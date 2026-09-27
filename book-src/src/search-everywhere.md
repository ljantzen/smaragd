# Search Everywhere

Tap **`Shift`** twice quickly to open **Search Everywhere**, an IntelliJ-style search box that finds four kinds of thing at once:

- **Documents**, by name, fuzzy-matched the same way as **`File > Open Document…`** (`Ctrl+P`).
- **Text** inside every document in the project, case-insensitive. Text search starts once you've typed at least 3 characters. It includes unsaved changes in the open document.
- **Actions**: every command that has a keyboard shortcut (Toggle Preview, New File, Commit, and so on), shown with its current shortcut, plus any `:` commands registered by [plugins](plugins.md).
- **Settings**, by name or related words. For example, "curly quotes" finds *Typewriter quotes in Preview and export*.

You can also open it with **`File > Search Everywhere…`** or **`Ctrl+Shift+A`**.

The tabs along the top are **All**, **Documents**, **Text**, **Actions**, and **Settings**. **All** shows the best few results from each source under a heading. The other tabs show only their own results, with more of them. `Tab` and `Shift+Tab` switch tabs, `Up` and `Down` move the highlight, and `Escape` closes the box.

Press `Enter`, or click a result, to act on it:

| Result | What happens |
|---|---|
| Document | The document opens. |
| Text | The document opens with the cursor at the match. |
| Action | The action runs, as if you'd pressed its shortcut. |
| Setting | Settings opens on the page with that setting. |

A tap only counts when `Shift` is pressed and released on its own. Typing capital letters, `Shift`+click, and shortcuts such as `Ctrl+Shift+…` never open Search Everywhere. Double Shift is also ignored while another dialog is open. To turn the gesture off, clear **Double Shift opens Search Everywhere** under **Settings > Shortcuts**. `Ctrl+Shift+A` keeps working either way.

Two kinds of action aren't listed, because they act on the editor's cursor: **Activate Wikilink** and **Toggle Bookmark**. `:` commands that take an argument, such as `:theme dracula`, still belong in the [command prompt](command-prompt.md).
