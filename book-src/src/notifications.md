# Notifications

Smaragd tells you about things two different ways, depending on how much attention they need:

- **Toasts** — a stack of boxes in the top-right corner of the window, each with its own **×** to dismiss early, that fade away on their own after a few seconds otherwise. Used for anything that represents an actual problem: a failed save, export, or git operation; invalid frontmatter YAML (see [Document Metadata](document-metadata.md)); a plugin error; and so on. Several can stack up at once if more than one thing goes wrong in quick succession.
- **The status bar**, at the bottom of the window — a single line for routine confirmations that don't need to grab your attention: "Committed", "Exported to ...", "Replaced 3 occurrence(s)", and the like. It now clears itself automatically after a few seconds, rather than sitting there until the next unrelated status update happens to overwrite it.

Both durations are configurable — see **Notifications** under [Settings](settings.md) below.
