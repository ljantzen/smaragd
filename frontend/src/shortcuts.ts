import type { KeyCombo } from "./stores/settings.svelte";
import { settingsStore, saveSettingsDebounced } from "./stores/settings.svelte";
import { dockLayoutRegistry } from "./stores/dockLayoutRegistry.svelte";
import { notImplemented } from "./stores/toast.svelte";
import { project, saveDocument } from "./stores/project.svelte";

// Port of the real egui app's ShortcutAction system (app/src/shortcuts.rs,
// 63 variants) — same action ids/labels/categories/default bindings, kept
// as the frontend's own catalog (not round-tripped through Rust) since
// dispatch has to run client-side for responsiveness: a Tauri IPC round
// trip per keystroke would be the thing the whole push-event architecture
// (Phase 6) exists to avoid. Only the *bindings* (which combo each action
// is mapped to) are persisted, via core::settings::Settings.shortcuts.
export type ShortcutCategory =
  | "Application"
  | "Project"
  | "FilesAndFolders"
  | "Editing"
  | "View"
  | "Git"
  | "Tools";

export type ShortcutActionDef = {
  id: string;
  label: string;
  category: ShortcutCategory;
  default: KeyCombo;
  /** Reserved for an editor-cursor-local action (wikilink-follow, bookmark-
   * toggle, add-note-at-cursor) in the real app — none of those have a real
   * implementation here yet (no cursor-position API from CodeMirror into
   * the backend), so this only keeps their default bindings reserved
   * (protected from being shadowed by a global action with an overlapping,
   * less-specific combo) rather than actually dispatching anything. */
  inlineConsumed?: boolean;
};

const cmd = (key: string): KeyCombo => ({ key, ctrl_or_cmd: true, shift: false, alt: false });
const cmdShift = (key: string): KeyCombo => ({ key, ctrl_or_cmd: true, shift: true, alt: false });
const cmdAlt = (key: string): KeyCombo => ({ key, ctrl_or_cmd: true, shift: false, alt: true });
const cmdAltShift = (key: string): KeyCombo => ({ key, ctrl_or_cmd: true, shift: true, alt: true });
const altOnly = (key: string): KeyCombo => ({ key, ctrl_or_cmd: false, shift: false, alt: true });
const bare = (key: string): KeyCombo => ({ key, ctrl_or_cmd: false, shift: false, alt: false });

// prettier-ignore
export const SHORTCUT_ACTIONS: ShortcutActionDef[] = [
  { id: "new_project", label: "New Project", category: "Project", default: cmdAlt("n") },
  { id: "open_project", label: "Open Project", category: "Project", default: cmd("o") },
  { id: "close_project", label: "Close Project", category: "Project", default: cmdShift("w") },
  { id: "open_settings", label: "Settings", category: "Application", default: cmd(",") },
  { id: "open_project_settings", label: "Project Settings", category: "Project", default: bare("f4") },
  { id: "exit", label: "Exit", category: "Application", default: cmd("q") },
  { id: "toggle_preview", label: "Toggle Preview", category: "View", default: cmdShift("p") },
  { id: "save", label: "Save", category: "Editing", default: cmd("s") },
  { id: "new_file", label: "New File", category: "FilesAndFolders", default: cmd("n") },
  { id: "new_folder", label: "New Folder", category: "FilesAndFolders", default: cmdShift("f") },
  { id: "rename", label: "Rename", category: "FilesAndFolders", default: bare("f2") },
  { id: "delete", label: "Delete", category: "FilesAndFolders", default: cmdShift("backspace") },
  { id: "restore", label: "Restore", category: "FilesAndFolders", default: cmdShift("r") },
  { id: "undo_last_move", label: "Undo Last Rename/Move", category: "FilesAndFolders", default: cmdAlt("z") },
  { id: "toggle_dark_mode", label: "Toggle Dark/Light Mode", category: "View", default: cmdShift("d") },
  { id: "toggle_fullscreen", label: "Toggle Full Screen", category: "View", default: bare("f11") },
  { id: "find_replace", label: "Find and Replace", category: "Editing", default: cmd("f") },
  { id: "toggle_corkboard", label: "Toggle Corkboard", category: "View", default: cmdShift("k") },
  { id: "toggle_story_grid", label: "Toggle Story Grid", category: "View", default: cmdShift("g") },
  { id: "toggle_belief_timeline", label: "Toggle Belief Timeline", category: "View", default: cmdShift("e") },
  { id: "toggle_backlinks", label: "Toggle Backlinks", category: "FilesAndFolders", default: cmdShift("b") },
  { id: "toggle_tags", label: "Toggle Tags", category: "View", default: cmdShift("t") },
  { id: "command_prompt", label: "Command Prompt", category: "Tools", default: cmdShift(";") },
  { id: "git_commit", label: "Commit (Git)", category: "Git", default: cmdAlt("c") },
  { id: "git_push", label: "Push (Git)", category: "Git", default: cmdAlt("p") },
  { id: "git_pull", label: "Pull (Git)", category: "Git", default: cmdAlt("l") },
  { id: "git_commit_and_push", label: "Commit and Push (Git)", category: "Git", default: cmdAltShift("c") },
  { id: "edit_metadata", label: "Metadata", category: "Editing", default: cmdShift("m") },
  { id: "toggle_binder_focus", label: "Toggle Binder/Editor Focus", category: "View", default: bare("f6") },
  { id: "toggle_focus_mode", label: "Toggle Focus Mode", category: "View", default: bare("f9") },
  { id: "open_document", label: "Open Document", category: "FilesAndFolders", default: cmd("p") },
  { id: "close_document", label: "Close Document", category: "FilesAndFolders", default: cmd("w") },
  { id: "recent_files", label: "Recent Files", category: "FilesAndFolders", default: cmdShift("o") },
  { id: "go_back", label: "Go Back", category: "FilesAndFolders", default: altOnly("arrowleft") },
  { id: "go_forward", label: "Go Forward", category: "FilesAndFolders", default: altOnly("arrowright") },
  { id: "activate_wikilink", label: "Activate Wikilink", category: "Editing", default: cmd("enter"), inlineConsumed: true },
  { id: "toggle_pomodoro", label: "Toggle Pomodoro Timer", category: "Tools", default: cmdAlt("t") },
  { id: "toggle_word_count", label: "Toggle Word Count", category: "Tools", default: cmdAlt("w") },
  { id: "refresh_word_count", label: "Refresh Word Count", category: "Tools", default: bare("f5") },
  { id: "toggle_collab_panel", label: "Toggle Collaboration Panel", category: "Tools", default: cmdShift("l") },
  { id: "toggle_streak", label: "Toggle Streak Tracking", category: "Tools", default: cmdAlt("s") },
  { id: "toggle_dashboard", label: "Toggle Dashboard", category: "Tools", default: cmdAlt("g") },
  { id: "cycle_binder_color_mode", label: "Cycle Binder Color Mode", category: "View", default: cmdShift("c") },
  { id: "toggle_bookmark", label: "Toggle Bookmark", category: "Editing", default: cmd("f2"), inlineConsumed: true },
  { id: "toggle_bookmarks_panel", label: "Toggle Bookmarks", category: "Tools", default: cmdAlt("b") },
  { id: "next_bookmark", label: "Next Bookmark", category: "FilesAndFolders", default: altOnly("arrowdown") },
  { id: "previous_bookmark", label: "Previous Bookmark", category: "FilesAndFolders", default: altOnly("arrowup") },
  { id: "add_note_at_cursor", label: "Add Note at Cursor", category: "Editing", default: cmdShift("j"), inlineConsumed: true },
  { id: "toggle_notes_panel", label: "Toggle Notes", category: "Tools", default: cmdShift("n") },
  { id: "next_note", label: "Next Note", category: "FilesAndFolders", default: cmdAlt("arrowdown") },
  { id: "previous_note", label: "Previous Note", category: "FilesAndFolders", default: cmdAlt("arrowup") },
  { id: "toggle_document_stats", label: "Toggle Document Stats in Binder", category: "View", default: cmdAlt("d") },
  { id: "preview_zoom_in", label: "Zoom In Preview", category: "View", default: cmd("equal") },
  { id: "preview_zoom_out", label: "Zoom Out Preview", category: "View", default: cmd("minus") },
  { id: "preview_zoom_reset", label: "Reset Preview Zoom", category: "View", default: cmd("0") },
  { id: "toggle_spell_check", label: "Toggle Spell Check", category: "Editing", default: bare("f7") },
  { id: "search_everywhere", label: "Search Everywhere", category: "FilesAndFolders", default: cmdShift("a") },
  { id: "toggle_sync_panel", label: "Toggle Sync Panel", category: "Tools", default: cmdShift("y") },
  { id: "sync_now", label: "Sync Now", category: "Tools", default: cmdAlt("y") },
  { id: "toggle_version_activity", label: "Toggle Version Activity", category: "Git", default: cmdAlt("v") },
  { id: "toggle_editor_view_mode", label: "Toggle Source/Reading View", category: "View", default: cmd("e") },
  { id: "toggle_inline_backlinks", label: "Toggle Backlinks in Document", category: "View", default: cmd("b") },
  { id: "open_daily_note", label: "Open Today's Daily Note", category: "Tools", default: bare("f8") },
];

export const SHORTCUT_CATEGORIES: ShortcutCategory[] = [
  "Application",
  "Project",
  "FilesAndFolders",
  "Editing",
  "View",
  "Git",
  "Tools",
];

/** How many modifiers a combo requires — used to check the more specific of
 * two overlapping bindings first (see matchesCombo's doc comment). */
function specificity(combo: KeyCombo): number {
  return (combo.ctrl_or_cmd ? 1 : 0) + (combo.shift ? 1 : 0) + (combo.alt ? 1 : 0);
}

/** Resolved effective binding for `action`: the user's override from
 * Settings if one exists (including an explicit unbind, represented as
 * `undefined` in the map — see the Settings dialog), else its default. */
export function effectiveCombo(action: ShortcutActionDef): KeyCombo | undefined {
  const override = settingsStore.value.shortcuts[action.id];
  if (override) return override;
  if (action.id in settingsStore.value.shortcuts) return undefined; // explicitly unbound
  return action.default;
}

export function setShortcut(actionId: string, combo: KeyCombo | null) {
  if (combo) {
    // Enforce 1:1: strip this combo from whatever other action currently
    // owns it, same as the real app's ShortcutMap::set.
    for (const [id, bound] of Object.entries(settingsStore.value.shortcuts)) {
      if (id !== actionId && combosEqual(bound, combo)) {
        delete settingsStore.value.shortcuts[id];
      }
    }
    settingsStore.value.shortcuts[actionId] = combo;
  } else {
    // An explicit unbind still needs a recorded marker distinguishing it
    // from "no override" — see effectiveCombo. A sentinel empty-key combo
    // serves that role without needing a second map.
    settingsStore.value.shortcuts[actionId] = { key: "", ctrl_or_cmd: false, shift: false, alt: false };
  }
  settingsStore.value = { ...settingsStore.value };
  saveSettingsDebounced();
}

function combosEqual(a: KeyCombo, b: KeyCombo): boolean {
  return a.key === b.key && a.ctrl_or_cmd === b.ctrl_or_cmd && a.shift === b.shift && a.alt === b.alt;
}

const CODE_TO_KEY: Record<string, string> = {
  Backspace: "backspace",
  Enter: "enter",
  Escape: "escape",
  ArrowLeft: "arrowleft",
  ArrowRight: "arrowright",
  ArrowUp: "arrowup",
  ArrowDown: "arrowdown",
  Equal: "equal",
  Minus: "minus",
  Semicolon: ";",
  Comma: ",",
  Period: ".",
  Digit0: "0",
};
for (let i = 1; i <= 9; i++) CODE_TO_KEY[`Digit${i}`] = String(i);
for (let i = 1; i <= 12; i++) CODE_TO_KEY[`F${i}`] = `f${i}`;
for (const letter of "abcdefghijklmnopqrstuvwxyz") {
  CODE_TO_KEY[`Key${letter.toUpperCase()}`] = letter;
}

function eventToKey(event: KeyboardEvent): string | undefined {
  return CODE_TO_KEY[event.code];
}

/** True if `event` satisfies `combo` — using physical `event.code` (not
 * `.key`, which changes under Shift) for the base key, so a combo's `shift`
 * flag is the only thing that determines whether Shift is required. Extra
 * Shift/Alt beyond what the combo requires is tolerated, not rejected —
 * matching egui's `Modifiers::matches_logically` semantics the real app's
 * shortcuts rely on (see shortcuts.rs's `sorted_by_specificity` doc
 * comment): a bare "Cmd+N" pattern also matches a physical Cmd+Alt+N press.
 * This is *why* specificity-sorting matters — see dispatchShortcut. */
function matchesCombo(event: KeyboardEvent, combo: KeyCombo): boolean {
  const key = eventToKey(event);
  if (key === undefined || key !== combo.key) return false;
  const ctrlOrCmd = isMac() ? event.metaKey : event.ctrlKey;
  if (ctrlOrCmd !== combo.ctrl_or_cmd) return false;
  if (combo.shift && !event.shiftKey) return false;
  if (combo.alt && !event.altKey) return false;
  return true;
}

function isMac(): boolean {
  return typeof navigator !== "undefined" && /mac/i.test(navigator.platform ?? navigator.userAgent ?? "");
}

export type ShortcutHandler = () => void;

/** Registered by App.svelte once real handlers exist for an action; an
 * action with no entry here falls through to notImplemented(). Keeping
 * this separate from SHORTCUT_ACTIONS keeps the catalog itself pure data,
 * shared as-is with the MenuBar and the Settings dialog's remapping UI. */
const handlers: Record<string, ShortcutHandler> = {};

export function registerShortcutHandler(actionId: string, handler: ShortcutHandler) {
  handlers[actionId] = handler;
}

export function runShortcutAction(actionId: string) {
  const action = SHORTCUT_ACTIONS.find((a) => a.id === actionId);
  const handler = handlers[actionId];
  if (handler) {
    handler();
  } else {
    notImplemented(action?.label ?? actionId);
  }
}

/** The global keydown handler — install once, on `window`, for the whole
 * app's lifetime (desktop only; see App.svelte). Checks every bound,
 * non-inline-consumed action's effective combo against the event, most-
 * specific-first, same ordering the real app's `sorted_by_specificity`
 * exists to enforce (see matchesCombo's doc comment for why order matters:
 * without it, a 1-modifier binding could silently steal a keypress meant
 * for an overlapping 2-modifier one). */
export function handleGlobalKeydown(event: KeyboardEvent) {
  if (!dockLayoutRegistry.api && !project.value) return; // no project open yet — most actions are meaningless
  const candidates = SHORTCUT_ACTIONS.filter((a) => !a.inlineConsumed)
    .map((a) => ({ action: a, combo: effectiveCombo(a) }))
    .filter((c): c is { action: ShortcutActionDef; combo: KeyCombo } => !!c.combo && c.combo.key !== "")
    .sort((a, b) => specificity(b.combo) - specificity(a.combo));

  for (const { action, combo } of candidates) {
    if (matchesCombo(event, combo)) {
      event.preventDefault();
      runShortcutAction(action.id);
      return;
    }
  }
}

// --- Built-in handlers for actions with a real implementation already ---
// Dock-tab toggles all go through the same DockLayout.openPanel (which
// focuses an already-open panel or opens it fresh) — toggle-closed isn't
// implemented (openPanel has no "close" half), same gap the real app's own
// toggle actions don't have but this port's dock integration doesn't
// support cleanly yet; opening is still the common case.
const DOCK_TAB_ACTIONS: Record<string, string> = {
  toggle_preview: "preview",
  toggle_corkboard: "corkboard",
  toggle_story_grid: "storygrid",
  toggle_belief_timeline: "belieftimeline",
  toggle_backlinks: "backlinks",
  toggle_tags: "tags",
  edit_metadata: "metadata",
  toggle_pomodoro: "pomodoro",
  toggle_word_count: "wordcount",
  toggle_collab_panel: "collab",
  toggle_streak: "streak",
  toggle_dashboard: "dashboard",
  toggle_bookmarks_panel: "bookmarks",
  toggle_notes_panel: "notes",
  toggle_sync_panel: "sync",
  toggle_version_activity: "versionactivity",
};

export function installBuiltinShortcutHandlers() {
  for (const [actionId, panelId] of Object.entries(DOCK_TAB_ACTIONS)) {
    registerShortcutHandler(actionId, () => dockLayoutRegistry.api?.openPanel(panelId));
  }
  registerShortcutHandler("save", () => saveDocument());
}
