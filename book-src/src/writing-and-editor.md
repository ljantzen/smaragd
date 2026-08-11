# Writing and the Editor

The main panel is a plain-text Markdown editor, borderless and filling the whole Editor tab — clicking anywhere in it, including below the last line of a short document, places the cursor there.

- **`Ctrl+S`** (or **`Cmd+S`** on macOS) saves explicitly. The document also saves automatically when it loses focus (e.g. you click into the binder or another panel).
- There's currently no multi-tab editing — opening a document replaces whatever's currently open (saving it first if it has unsaved changes).
- **`File > Open Document…`** (or **`Ctrl+P`**) opens an fzf-style quick-switcher: type a few letters and it fuzzy-matches against every document's path, best match first — a query doesn't need to be a contiguous substring, so e.g. "ch1sc2" can match "Chapter 1/Scene 2". Use `Up`/`Down` to change the highlighted result, `Enter` or a click to open it, `Escape` to cancel.
- **`File > Close Document`** (or **`Ctrl+W`**) saves the current document if it has unsaved changes, then closes it — there's no save/discard/cancel prompt, matching the same silent-autosave behavior as opening a different document.
- Exiting the app (**`File > Exit`**, **`Ctrl+Q`**, or closing the window) is different: if the open document has unsaved changes, or a Story Card editor is open with an uncommitted draft (see [Story Cards](story-cards.md)), a **Save / Discard / Cancel** prompt appears instead of closing immediately. Save writes the document (and the card draft, if any) before exiting; Discard drops both without writing them; Cancel leaves the app open exactly as it was.
