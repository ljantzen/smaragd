# Browser Edition

An experimental build of Smaragd that runs entirely in a web browser via WebAssembly — try it at [ljantzen.github.io/smaragd/app/](https://ljantzen.github.io/smaragd/app/), no install required. It's a preview for kicking the tires, not a replacement for the native app: expect rough edges, and don't treat it as your only copy of anything you care about.

## Storage: local to this browser, not a server

A browser-edition project isn't a folder on disk — it lives in this browser's own [IndexedDB](https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API) storage, synced on every save. Nothing is uploaded anywhere; the app has no server component and no account.

That cuts both ways:

- Your writing never leaves your machine.
- There's no cloud backup. Clearing this browser's site data, using a private/incognito window, or opening the app in a different browser or on a different device all mean starting from an empty project — the previous one doesn't come with you.
- Use **File > Export** (DOCX/EPUB/PDF) regularly, the same as in the native app, if you want a copy that survives outside the browser.

## What's missing compared to the native app

- **No git integration.** Git needs a real folder on disk to operate on; a browser-storage project doesn't have one. This isn't planned for a future version — it's structurally out of scope for this edition.
- **No peer-to-peer collaboration.** Real-time collaborative editing depends on raw networking (QUIC) that browsers don't expose to WebAssembly the same way.
- **No Scrivener import.** Importing a `.scriv` project requires picking a folder, which has no browser equivalent (DOCX/EPUB/PDF import all work, since those are single-file picks).
- **No native desktop notifications** and **no plugin scripts that shell out to external programs** — both OS-level capabilities a browser sandbox doesn't grant.

Everything else — the binder, editor, dockable panels, story cards, tags, wikilinks, export/import for DOCX/EPUB/PDF, spell check, themes — works the same as the native app.

## Opening and saving projects

There's only one project per browser at a time — the browser edition has no folder picker to point at a second one. **File > New Project** creates it from a template, same choices as the native app. **File > Open Project** doesn't browse anywhere; it just re-loads whatever's currently persisted in this browser's storage, which is only useful after closing a project without opening another. Saving is automatic (on blur, same as native), synced to IndexedDB on every change, and reloaded automatically the next time you open the app in this browser.
