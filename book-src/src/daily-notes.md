# Daily Notes

Off by default — turn it on from **`File > Project Settings… > Daily Notes`** with the **Enable Daily Notes** checkbox. While off, both the shortcut and `:daily` do nothing but show a toast pointing at that checkbox.

Once enabled, **`F8`** (or **`:daily`** in the [Command Prompt](command-prompt.md)) opens today's note, creating it first if it doesn't exist yet. Pressing it again later the same day reopens the same note without touching its content.

The rest of the same **Daily Notes** settings page:

- **Folder**: where daily notes are created, relative to the project root. Left blank, they go in the project root itself. Unlike most other folder settings in smaragd, this one doesn't need to already exist — it's created automatically the first time a daily note is actually written there, the same way Obsidian's own Daily Notes/Periodic Notes plugins work. It's also a [strftime](https://en.wikipedia.org/wiki/Strftime) pattern (same mini-language as **Filename format** below): a plain name like `Journal` passes through unchanged, but something like `Journal/%Y/%m` nests notes a per-year, per-month subfolder at a time — the preview underneath shows what today resolves to.
- **Filename format**: a strftime pattern for the note's filename (without `.md`), e.g. `%Y-%m-%d` for `2026-07-28.md`. Defaults to `%Y-%m-%d` when left blank.

  Both fields share an **"Example date formats"** list right in the dialog — expand it for a handful of ready-made patterns with a live preview. For the full set of individual codes (`%A` for the weekday name, `%-d` for a day number with no leading zero, and so on), see [Template Variables](folder-roles.md#template-variables).
- **Template**: an optional document from the project's [Templates-role folder](folder-roles.md) to seed the note's content from, the same substitution as "New From Template" — see [Template Variables](folder-roles.md#template-variables). `${{previous_day}}`/`${{next_day}}` render using *this* filename format, so a template like:

  ```markdown
  # ${{date:%A, %B %d, %Y}}

  [[${{previous_day}}]] | [[${{next_day}}]]

  ${{cursor}}
  ```

  produces a heading in a friendlier format than the filename itself, wikilinks to yesterday's and tomorrow's notes (by their actual filenames), and leaves the cursor ready to start typing. If the configured template document is later deleted, daily notes just fall back to being created blank — it isn't an error.

With no project open, both the shortcut and `:daily` do nothing but show a toast saying so (checked before the enabled toggle, since there's no project settings to have enabled it on in the first place).
