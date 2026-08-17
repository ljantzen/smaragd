# Collaboration

The **`Collaborate`** menu (and its dockable **Collaboration Panel**, `Ctrl+Shift+L`) lets two people edit the same document together in real time, peer-to-peer — no server, no account, no third-party service ever holds the manuscript text.

## Hosting a session

1. Open the document you want to collaborate on.
2. **`Collaborate > Host Session`** (needs a document open; disabled otherwise).
3. Smaragd generates a one-time **connection code** and shows it in the Collaboration Panel. **Copy** it and send it to your collaborator through whatever channel you'd already trust with the document itself — chat, email, whatever.
4. The panel shows "Waiting for a peer to join…" until they do.

## Joining a session

1. **`Collaborate > Join Session…`** (or **Join Session…** in the panel itself) — needs *no* document currently open, since the shared document a join receives replaces whatever was there, not merges with one of your own files. Close your current document first if one's open.
2. Paste the code your collaborator sent you and confirm.
3. Once paired, the host's document appears in your editor and either side can type — edits from both sides merge automatically.

## While connected

Both sides just type normally in the Editor tab; there's no separate "collaboration mode" to the editing experience itself. Under the hood, each side's edits are diffed against a shared baseline and merged with a CRDT (the same category of algorithm behind Google Docs/Yjs), so concurrent edits from both people — even to the same paragraph — converge to the same result on both sides without overwriting each other or needing a manual conflict resolution step. When a remote edit comes in, your local cursor position is adjusted to stay put relative to the surrounding text rather than jumping.

The panel shows **Connected to peer `<fingerprint>`** once pairing completes — a short id derived from the peer's network identity, useful for confirming you're connected to who you think you are, not a name either side chooses. **End Session** stops collaborating; the document itself is unaffected and stays open normally afterward.

What opening a different document does depends on which side you're on. If you're **hosting**, switching to another document keeps the session running — your collaborator's view follows along to the new document automatically, with a status message ("Your collaborator switched documents") to explain why their editor content just changed. If you're the one who **joined**, opening one of your own documents has nowhere to put the shared one, so you're asked to confirm first: decline and the shared document keeps showing with the session still live, confirm and the session ends before your document opens. Either side **closing** the current document still ends the session immediately.

## Reconnecting after a dropped connection

Your collaborator's connection dropping — network loss, a laptop going to sleep and waking back up, a phone switching networks — doesn't end the session outright. The panel shows **"Lost connection to your collaborator — trying to reconnect…"**, and Smaragd keeps trying to get them back for about a minute before giving up. Nothing needs pasting again: reconnection reuses the exact same connection code, so once the network comes back, the two sides just pair up again on their own — no fresh **Host Session**/**Join Session…** needed. If the joiner's whole app closed rather than just its network dropping, pasting the same code into a new **Join Session…** reaches the still-waiting host the same way. Anything typed on either side while disconnected isn't lost — it's queued and sent the moment the connection comes back.

If about a minute passes with no luck, the panel falls back to **"Lost connection to your collaborator"** and the session is over for good — start a fresh **Host Session**/**Join Session…** to resume, the same as before. You can also give up early with the panel's **Cancel** button instead of waiting out the full window.

## Privacy and security

- **No server holds your text.** Peers connect directly to each other via [iroh](https://iroh.computer) (falling back to iroh's relay infrastructure only to help establish that direct connection when needed, the same way most peer-to-peer / video-call tools do) — the manuscript itself is never uploaded anywhere or stored by a third party.
- **End-to-end encrypted**, on top of iroh's own transport encryption: every edit exchanged between peers is additionally encrypted with a key derived from a secret that exists only inside the connection code itself, so even iroh's own relay infrastructure can't read the content it's helping relay.
- **The connection code is the credential.** Whoever holds it can join the session — treat it like a password for as long as the session is open, and don't post it somewhere public. Joining requires proving you hold the secret from the code before the other side ever reports you as connected, so a stranger who reaches the host's network endpoint without the code can neither read the session nor block the real collaborator from pairing.
- Each session's encryption keys are freshly derived per session and tied to that specific connection code — an old code from a past session can't be reused to rejoin a new one.
