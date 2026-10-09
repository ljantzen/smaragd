# Writing and the Editor

The main panel is a plain-text Markdown editor, borderless and filling the whole Editor tab — clicking anywhere in it, including below the last line of a short document, places the cursor there. The open document's title (its filename, without the `.md` extension) is shown as a heading above the text, so it's always clear which document you're looking at even if the Editor tab itself is just labeled "Editor".

- **`Ctrl+S`** (or **`Cmd+S`** on macOS) saves explicitly. With multi-tab editing off (the default — see below), the document also saves automatically when it loses focus (e.g. you click into the binder or another panel).
- By default, opening a document replaces whatever's currently open (saving it first if it has unsaved changes) — there's no multi-tab editing unless you turn it on; see [Multi-tab editing](#multi-tab-editing) below.
- **`File > Open Document…`** (or **`Ctrl+P`**) opens an fzf-style quick-switcher: type a few letters and it fuzzy-matches against every document's path, best match first — a query doesn't need to be a contiguous substring, so e.g. "ch1sc2" can match "Chapter 1/Scene 2". Use `Up`/`Down` to change the highlighted result, `Enter` or a click to open it, `Escape` to cancel.
- **`File > Close Document`** (or **`Ctrl+W`**) saves the current document if it has unsaved changes, then closes it — there's no save/discard/cancel prompt, matching the same silent-autosave behavior as opening a different document. With multi-tab editing on, this closes the active tab specifically (see below).
- smaragd remembers every document you open, along with where your cursor was in each — **`File > Go Back`**/**`Go Forward`** (or **`Alt+Left`**/**`Alt+Right`**) step through that history, browser-style, and jump straight back to the cursor position you left. Opening a document you're already viewing doesn't add a duplicate entry, and navigating to a new document after going back drops whatever was ahead of you, the same way a browser tab does. With multi-tab editing on, this history is per-tab rather than shared — see below.
- **`Ctrl+Shift+O`** opens the Recent Files switcher — another fzf-style quick-switcher like `Ctrl+P`'s, but scoped to this session's recent documents rather than the whole project. The first press shows documents you've *edited* this session, most-recently-edited first; pressing `Ctrl+Shift+O` again while the popup is still open switches to documents you've *opened* this session instead (edited or not), and pressing it again switches back — so you can quickly reach either "what have I been writing" or "what have I been looking at" without retyping a query. `Up`/`Down`, `Enter`/click, and `Escape` work the same as Open Document above.
- Exiting the app (**`File > Exit`**, **`Ctrl+Q`**, or closing the window) is different: if the open document (or, with multi-tab editing on, any open tab) has unsaved changes, or a Story Card editor is open with an uncommitted draft (see [Story Cards](story-cards.md)), a **Save / Discard / Cancel** prompt appears instead of closing immediately. Save writes every unsaved document (and the card draft, if any) before exiting; Discard drops all of it without writing anything; Cancel leaves the app open exactly as it was.

## Multi-tab editing

Off by default (**Settings > Editor > Allow multiple documents open at once**, `Ctrl+,`; see [Settings](settings.md)). With it on, several documents can stay open at once, each in its own tab across the top of the Editor pane — a dirty dot next to a tab's filename marks unsaved changes, and a **×** closes it.

A few things work differently from the single-document default while this is on:

- Each tab keeps its own buffer fully resident in memory. Switching tabs is instant and never touches disk — a tab is only written out when you close it, press `Ctrl+S` on it, or exit the app.
- Opening a document (from the Binder, a wikilink, search, or any quick-switcher) focuses its tab if it's already open, rather than opening a second one for the same file.
- **Go Back**/**Go Forward** belong to whichever tab is active, not the app as a whole: each tab remembers its own trail of documents opened *from* it. Since opening a document always focuses or creates a tab rather than changing an existing tab's content, Forward often has nothing to go to right after a Back — switching tabs directly is the normal way to move between documents that are already open.
- Joining a [Collaboration](collaboration.md) session opens the shared document in its own tab too, so you don't need to close what you're already working on first.

Turning the setting off collapses back down to one tab immediately, saving any other dirty tabs first.

## Picking up external changes

smaragd periodically notices files added, removed, or edited outside the app — a sync tool like Dropbox or Syncthing, a `git pull` run from a terminal, or hand-editing a file in another program — and reacts automatically, without needing a manual "Reload" action or a restart:

- The Binder rescans, so files added or removed elsewhere show up (or disappear) on their own.
- If the *open* document changed on disk and you have no unsaved edits to it, it's silently reloaded to match.
- If the open document changed on disk *and* you have unsaved edits, smaragd doesn't guess: a prompt lets you choose **Keep Mine** (your edits stay; the on-disk change is otherwise ignored) or **Reload from Disk** (your unsaved edits are discarded in favor of the external version).

## The ☰ menu

A **☰** button sits in the Editor tab's top-right corner:

- **Backlinks in document** — toggles a resizable panel below the editor's own content showing every document that links here (see [Backlinks](backlinks.md)), independent of the separate Backlinks dock tab. Also bound to **`Ctrl+B`**.
- **Source mode** / **Reading view** — switches the Editor tab itself between the plain-text editor and a rendered markdown preview, without needing the separate Preview tab. Also bound to **`Ctrl+E`**. Switching back to Source mode restores the cursor to where it was left.
- **Rename…** / **Move file to…** / **Delete file** — the same actions as the Binder's own context menu, for whichever document is currently open. Move file to… opens an in-app, fuzzy-searchable folder picker rather than an OS file dialog.
- **Find & Replace…** — opens [Find and Replace](find-and-replace.md) scoped to the current file.
- **Open in default app** / **Show in system explorer** — hands the file to the OS's default application, or reveals it in the platform's file manager.
- **Reveal file in navigation** — expands every collapsed ancestor folder in the Binder and focuses the file's row there.

The last six rows only appear while a document is open.

## Pasting and dropping attachments

Pasting (`Ctrl+V`) or drag-and-dropping an image or other file onto the editor saves a copy into the project and inserts a reference at the cursor, an image becomes a `![[attachment.png]]` embed, anything else a `[attachment.pdf](<attachment.pdf>)` link. A clipboard image with no filename (e.g. a browser screenshot copy) gets a generated one.

Where the file is saved is a per-project setting (**`File > Project Settings > Attachments`**): next to the document that receives it, or in a single configurable folder for the whole project. An optional size cap can reject oversized clipboard images instead of silently saving them.

## Line numbers

A gutter down the left edge of the Editor can show each line's number — off by default, turned on with **Show line numbers** under **Settings > Editor** (`Ctrl+,`; see [Settings](settings.md)). Numbers count logical lines (real line breaks in the file), not wrapped visual rows, so a long paragraph that wraps across several rows only gets numbered once, at the row where it starts — the same convention word-wrap-aware code editors use. Two strips to the left of the numbers show per-line markers: a bookmarked line's diamond (see [Bookmarks](bookmarks.md)) and, to its left, a noted line's dot (see [Notes](notes.md)).
