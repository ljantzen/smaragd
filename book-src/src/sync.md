# Sync (experimental)

> **Sync is experimental.** It's new, and its data format and server protocol may still change in ways that require re-creating vaults. Keep [backups](backups.md) on and don't rely on it as the only copy of anything you care about.

**Sync** keeps a project identical across your own devices — your laptop and your desktop, say — in the background, even when only one of them is open at a time. It works through a small **sync server that you host yourself**, and everything is **end-to-end encrypted**: your text is encrypted on your device before it is uploaded, and the server only ever stores data it cannot read. This protects the copy on the server and the trip there; the files on your own devices are ordinary, unencrypted Markdown, exactly as before.

Sync is one of three ways Smaragd can move a project around, and they do different jobs:

| | What it's for | Needs a server? | Keeps history? |
|---|---|---|---|
| **Sync** (this chapter) | The same project on *your own* devices, always up to date | Yes — one you host | No (see [Sync is not a backup](#sync-is-not-a-backup)) |
| [Collaboration](collaboration.md) | Two people editing one document together, live | No — peer-to-peer | No |
| [Git](git-integration.md) | Deliberate, named versions and sharing through a git host | A git remote, if you push | Yes |
| [Backups](backups.md) | Restorable zipped snapshots on this machine | No | Yes |

Sync is available in the desktop app only; the [browser edition](browser-edition.md) doesn't have it.

## What you need

- **A sync server.** It's a single small program (or Docker container) you run on a machine you control — a home server, a VPS, a Raspberry Pi. Setting one up is covered in the server's own [self-hosting guide](https://github.com/ljantzen/smaragd/tree/main/crates/smaragd-sync-server); it takes a few minutes with Docker. If someone else runs one for you, you just need its address.
- **A passphrase** you choose. It encrypts your data, and you enter the *same* one on every device.

## Setting it up

### 1. Turn it on and tell Smaragd where the server is

Open **`File > Settings > Sync`** and fill in:

- **Enable sync** — the master switch. Nothing syncs while it's off.
- **Host** — the server's name, like `sync.example.com` (just the name: no `https://`, no path).
- **Port** — leave at `0` to use the default (443 with HTTPS, 8080 without), or enter the one your server uses.
- **Path** — only if a reverse proxy serves the server under a sub-path such as `https://example.com/smaragd/`; usually blank.
- **Use HTTPS (TLS)** — on by default. Turn it off only for a server on a network you trust; plain HTTP sends your device's access token unencrypted (your text stays encrypted either way).
- **Passphrase** — see [Your passphrase](#your-passphrase) below. **Show** reveals what you typed.
- **Name** (under *This device*) — how this device appears in the vault's device list. Blank uses "Smaragd on *your OS*".

**Test Connection** contacts the server and tells you whether it's reachable, so you can catch a typo before going further. If something needed is missing, the page says what.

### 2. Start syncing a project

Open the project, then open the Sync tab with **`Tools > Sync Panel`**. A project that isn't syncing yet offers two choices:

- **Create Vault…** — for the *first* device. This makes a new, empty vault on the server for this project and starts syncing straight away. If the server only lets its administrator create vaults (the usual setup), Smaragd asks for the server's **admin token** — the one whoever set the server up chose. It's needed only for this step: Smaragd doesn't store it, other devices join with a pairing ticket instead, and if the server's admin token is changed later your existing vaults keep syncing.
- **Join Vault…** — for every *other* device, see below.

### 3. Add your other devices

On a device that's already syncing, open the Sync panel and press **Make Pairing Ticket**. Copy the ticket it shows and get it to your other device — by chat, email, whatever suits. **A ticket works once and expires after 10 minutes.**

On the other device: install Smaragd, do step 1 (the same passphrase!), open a project folder (an empty one is fine, or a copy of the project), open the Sync panel, press **Join Vault…** and paste the ticket. Smaragd pairs the device and the project begins syncing.

## The Sync panel

The panel shows where things stand:

- **Up to date — synced 2 min ago** — everything is in step.
- **Syncing…** — a pass is running.
- **Offline** — the server can't be reached. Your edits are saved locally and sync as soon as it's reachable again; nothing is lost.
- A red message with **Retry** — sync stopped and needs you. Typically the passphrase doesn't match (fix it in Settings and sync restarts by itself), or this device was removed from the vault.

Below the status:

- **Sync Now** runs a pass immediately. (Smaragd also syncs about every 10 seconds, and right after you save with `Ctrl+S`. **`Tools > Sync Now`** does the same from the menu.)
- **Make Pairing Ticket** — see above.
- **Devices** lists every device in the vault and when it was last seen. **Refresh** updates the list; **Revoke** removes another device — it stops syncing immediately. Use this if a device is lost or retired.
- **Stop syncing this project** (expand it) removes this device from the vault and stops syncing this project. Your files stay exactly as they are here, and your other devices keep their copies. If it was the vault's *last* device, the server deletes the vault's encrypted copy after a while (30 days by default, set by whoever runs the server); your own files are never affected.

## What syncs

- **Your documents** — every `.md` file — and **your folders**, including empty ones. Edits, new files, renames, moves and deletions all follow you between devices.
- **Project settings** — the binder order, folder roles, story cards, bookmarks, colors, word-count targets, the book title and the project's title/logline/synopsis — everything in [Project Metadata](project-metadata.md) and its neighbours **except** the per-device parts described next.

**What stays on each device:** whether git support is switched on, whether this project's plugins are enabled, the running session word count and daily history, and the Writing Streak switch and schedule. Those are yours alone on each device. In particular, **turning on plugins is never synced** — a plugin runs code, so you decide device by device.

**What doesn't sync at all:** anything that isn't a Markdown document — images or other attachments in the project folder, your plugin scripts, your backups, and hidden or git-ignored files.

## How edits from two devices are merged

Sync doesn't ask you to pick "mine" or "theirs". Each document is merged automatically, the same way live [collaboration](collaboration.md) works, so you can edit on your laptop on the train and on your desktop at home and the results combine:

- **Two edits to the same file** — even in the same paragraph — are both kept.
- **A rename and an edit** to the same file both survive: the file ends up under its new name with the edit in it.
- **A delete and an edit:** the edit wins — the file comes back with the change rather than losing your work.
- **Frontmatter** (`status`, `pov`, …) merges field by field, so changing `status` on one device and `pov` on the other keeps both and never produces a half-merged, broken block. If both change the *same* field, one value wins on both devices.
- **A rename of a whole folder** moves the folder everywhere, and its settings (role, order, metadata) and bookmarks come with it.
- **Two devices creating a file at the same path** keeps both: one gets a name like `Scene (conflict 3f9a1c2e).md`.

### The file you're editing

While a file has **unsaved edits** in the editor, Smaragd holds off writing other devices' changes to it, so your typing is never disturbed. When you save, your edit is merged with whatever arrived, and the merged text appears. A file you have open but haven't changed simply reloads when another device changes it, like any file changed outside the editor.

### Joining with a project that already has files

Setting up a second device with a copy of the project (from a backup, a zip, a cloud folder) is fine:

- Files that are **identical** to the vault's are simply recognised — nothing is duplicated.
- If a file **differs** from the vault's, there's no way to know which is newer, so **both are kept**: the vault's version takes the original name, and your local version is saved next to it as `name (conflict copy).md`. Smaragd tells you when this happens. Compare the two and merge by hand.
- If your `project.json` differs, the vault's settings are used and your previous file is kept as `.smaragd/project.json.before-sync`.

## Your passphrase

- It's chosen by you, entered in **Settings > Sync**, and **must be identical on every device**. Choose something long — it's the only thing protecting your data if someone gets the server's files.
- **It never leaves your device** — the server never sees it — and **it cannot be recovered.** If you lose it, the copy of your project *on the server* can't be decrypted, by anyone, including whoever runs the server. Your own files are not affected: they are plain Markdown on your devices, so nothing is lost as long as one device still has them. Keep the passphrase somewhere safe, like a password manager.
- **Changing it makes existing vaults unreadable** (the server's copy was encrypted with the old one); your local files are untouched. If sync stops with a message about the passphrase, the passphrase on this device doesn't match the one the vault was created with. To switch to a new passphrase, choose *Stop syncing this project* on each device, then create a fresh vault from one of them and join the others to it.
- Like your other settings, it's kept as plain text in Smaragd's settings file (`smaragd.toml`), at the same trust level as the project files on this disk. A full-disk encryption setup is a good idea on a laptop.

## What the server can and can't see

**It can't see:** file names, folder structure, your text, your project settings — all of it is encrypted with a key built from your passphrase on your device. It also can't swap one document's data for another's without the change being detected.

**It can see** that a vault exists, how many documents it has (as random ids), how big and how frequent the updates are, which of your devices made them, and your IP address. Device access tokens and pairing codes are stored on the server only as hashes.

**It can do harm only by withholding or deleting data** (so keep your [backups](backups.md) on) — it can't read or forge your content.

**On your own devices nothing is encrypted by Sync.** Your Markdown files are plain files, and Smaragd's local sync bookkeeping (in its data folder, next to the backup folder) contains readable copies of your document text, including earlier revisions, so it deserves the same protection as the project itself — full-disk encryption is the right tool for that.

Files on your side: a small, non-secret file `.smaragd/sync.json` in the project says which vault it belongs to (it's safe to commit or copy); this device's access token and sync bookkeeping live in Smaragd's data folder, **outside** the project, so they never end up in [git](git-integration.md) or a [backup](backups.md).

## Sync is not a backup

Sync copies *every* change, including a mistake or a deletion, to all your devices. It keeps no history you can go back to. Keep [backups](backups.md) (and optionally [git](git-integration.md)) switched on as well. Sync and git coexist without trouble: git commits whatever is currently on disk, and sync moves whatever is on disk between devices.

## If you run the server

The server's own [self-hosting guide](https://github.com/ljantzen/smaragd/tree/main/crates/smaragd-sync-server) has the details; the short version of what to expect:

- **It looks after itself.** It clears out expired pairing codes, deletes vaults whose last device left 30 days ago (adjustable), and keeps its database file small, all in the background. `smaragd-sync-server admin list` shows every vault, and `admin delete-vault`, `purge-empty` and `vacuum` clean up by hand; run them with `docker exec` while the server is running.
- **You still need to** back up its data volume, install updates, and watch disk space. Deleted files' encrypted history stays on the server until its vault is deleted.

## Good to know

- **The vault stays compact.** As a file accumulates changes, Smaragd periodically replaces the old ones on the server with a single snapshot, so the vault doesn't grow without bound and a new device catches up quickly. There's also a per-vault size limit on the server (1 GiB by default).
- **Bookmarks** point to a line number, so if two devices edit above a bookmarked line at the same time, the bookmark may end up a line or two off.
- **One vault per project**, and a project remembers the server it was paired with; the server in Settings is used when you create or join.
- Sync needs a project **folder on disk** — it isn't available in the browser edition.

## Troubleshooting

- **"The sync passphrase doesn't match this vault"** — the passphrase here differs from the one used when the vault was created. Correct it in Settings > Sync; sync restarts by itself.
- **"This device was removed from the vault"** — another device revoked it, or the vault was deleted. Choose *Stop syncing this project*, then join again with a new ticket if you want.
- **Create Vault says the server only lets its administrator create vaults** — enter the server's admin token when asked (it's set by whoever runs the server). If you run the server yourself and have lost the token, the server's [self-hosting guide](https://github.com/ljantzen/smaragd/tree/main/crates/smaragd-sync-server#troubleshooting) shows how to look it up or set a new one; existing vaults are unaffected.
- **"That doesn't look like a pairing ticket"** — copy the whole ticket, with nothing added or missing. Tickets are single-use and expire after 10 minutes; make a new one if in doubt.
- **Offline** — check the host, port and HTTPS setting with **Test Connection**, and that the server is running.
- **A `(conflict copy)` or `(conflict …)` file appeared** — two devices made different changes that couldn't be told apart; open both files, keep what you want, and delete the other.
