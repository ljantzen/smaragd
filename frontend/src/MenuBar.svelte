<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { project, closeProject } from "./stores/project.svelte";
  import { dockLayoutRegistry } from "./stores/dockLayoutRegistry.svelte";
  import { settingsStore, saveSettingsDebounced } from "./stores/settings.svelte";
  import { SHORTCUT_ACTIONS, effectiveCombo, runShortcutAction } from "./shortcuts";
  import { notImplemented } from "./stores/toast.svelte";

  // Desktop menu bar matching the real egui app's 8-menu structure
  // (app/src/app/menu_bar.rs) as closely as this port's actual
  // functionality allows — every item from that file is represented here,
  // but many call notImplemented() rather than a real action: Export,
  // Find & Replace, Command Prompt, Search Everywhere, real git commit/
  // push/pull (Version Activity is still read-only — Phase 7), Reload
  // Plugins (no plugin engine), saved named dock Layouts (Reset Layout only
  // restores the hardcoded default, same as Phase 5), About, docx/epub/PDF/
  // Scrivener import. These are listed for structural fidelity, not
  // because they work — same honesty convention as StubPanel elsewhere in
  // this app. Submenus (Recent Projects, Import, Theme, Color Binder By,
  // Layouts, multi-folder Export) are flattened to single items rather than
  // true nested flyouts — a scope cut, not a design choice; Theme
  // specifically gets a flat one-level list since color_theme.ts is real.
  let openMenu = $state<string | null>(null);

  type Item =
    | { separator: true }
    | {
        label: string;
        shortcutId?: string;
        action: () => void;
        disabled?: boolean;
        hidden?: boolean;
      };

  function toggle(menu: string) {
    openMenu = openMenu === menu ? null : menu;
  }

  function close() {
    openMenu = null;
  }

  function run(action: () => void) {
    action();
    close();
  }

  function dock(id: string) {
    dockLayoutRegistry.api?.openPanel(id);
  }

  function shortcutLabel(actionId: string): string | undefined {
    const action = SHORTCUT_ACTIONS.find((a) => a.id === actionId);
    if (!action) return undefined;
    const combo = effectiveCombo(action);
    if (!combo || combo.key === "") return undefined;
    const mod = combo.ctrl_or_cmd ? "Ctrl+" : "";
    const alt = combo.alt ? "Alt+" : "";
    const shift = combo.shift ? "Shift+" : "";
    return `${mod}${alt}${shift}${combo.key.toUpperCase()}`;
  }

  const hasProject = $derived(!!project.value);

  const menus: { id: string; label: string; items: () => Item[] }[] = [
    {
      id: "file",
      label: "File",
      items: () => [
        { label: "New Project…", action: () => notImplemented("New Project") },
        { label: "Open Project…", action: () => notImplemented("Open Project (use the open screen)") },
        { label: "Recent Projects", action: () => notImplemented("Recent Projects") },
        { label: "Close Project", action: closeProject, disabled: !hasProject },
        { separator: true },
        { label: "Open Document…", action: () => notImplemented("Open Document") },
        { label: "Search Everywhere…", action: () => runShortcutAction("search_everywhere") },
        { label: "Close Document", action: () => runShortcutAction("close_document") },
        { label: "Go Back", action: () => runShortcutAction("go_back") },
        { label: "Go Forward", action: () => runShortcutAction("go_forward") },
        { separator: true },
        { label: "Import…", action: () => notImplemented("Import") },
        { label: "Export Manuscript…", action: () => notImplemented("Export Manuscript") },
        { separator: true },
        { label: "Settings…", action: () => (showSettings = true) },
        {
          label: "Project Settings…",
          action: () => notImplemented("Project Settings"),
          disabled: !hasProject,
        },
        { separator: true },
        { label: "Exit", action: () => notImplemented("Exit") },
      ],
    },
    {
      id: "edit",
      label: "Edit",
      items: () => [
        { label: "Cut", action: () => document.execCommand("cut") },
        { label: "Copy", action: () => document.execCommand("copy") },
        { label: "Paste", action: () => document.execCommand("paste") },
        { separator: true },
        { label: "Find and Replace", action: () => runShortcutAction("find_replace") },
      ],
    },
    {
      id: "view",
      label: "View",
      items: () => [
        { label: "Editor", action: () => dock("editor") },
        { label: "Preview", shortcutId: "toggle_preview", action: () => dock("preview") },
        { label: "Corkboard", shortcutId: "toggle_corkboard", action: () => dock("corkboard") },
        { label: "Story Grid", shortcutId: "toggle_story_grid", action: () => dock("storygrid") },
        {
          label: "Belief Timeline",
          shortcutId: "toggle_belief_timeline",
          action: () => dock("belieftimeline"),
        },
        { separator: true },
        { label: "Binder", action: () => dock("binder") },
        { label: "Metadata", shortcutId: "edit_metadata", action: () => dock("metadata") },
        { label: "Backlinks", shortcutId: "toggle_backlinks", action: () => dock("backlinks") },
        { label: "Tags", shortcutId: "toggle_tags", action: () => dock("tags") },
        { separator: true },
        ...themeItems(),
      ],
    },
    {
      id: "tools",
      label: "Tools",
      items: () => [
        { label: "Focus Mode", action: () => runShortcutAction("toggle_focus_mode") },
        { label: "Toggle Spell Check", action: () => runShortcutAction("toggle_spell_check") },
        { separator: true },
        { label: "Command Prompt", action: () => runShortcutAction("command_prompt") },
        { label: "Pomodoro Timer", action: () => dock("pomodoro") },
        { label: "Word Count", action: () => dock("wordcount") },
        { label: "Streak", action: () => dock("streak") },
        { label: "Dashboard", action: () => dock("dashboard") },
        { label: "Bookmarks", action: () => dock("bookmarks") },
        { label: "Notes", action: () => dock("notes") },
        { separator: true },
        { label: "Sync Panel", action: () => dock("sync") },
        { label: "Sync Now", action: () => runShortcutAction("sync_now") },
      ],
    },
    {
      id: "versions",
      label: "Versions",
      items: () => [
        { label: "Commit", action: () => notImplemented("Git Commit") },
        { label: "Commit and Push", action: () => notImplemented("Git Commit and Push") },
        { label: "Push", action: () => notImplemented("Git Push") },
        { label: "Pull", action: () => notImplemented("Git Pull") },
        { separator: true },
        { label: "Version Activity", action: () => dock("versionactivity") },
      ],
    },
    {
      id: "collaborate",
      label: "Collaborate",
      items: () => [
        { label: "Host Session", action: () => dock("collab") },
        { label: "Join Session…", action: () => dock("collab") },
        { label: "End Session", action: () => dock("collab") },
        { separator: true },
        { label: "Collaboration Panel", action: () => dock("collab") },
      ],
    },
    {
      id: "window",
      label: "Window",
      items: () => [
        { label: "Save Current Layout…", action: () => notImplemented("Save Current Layout") },
        { label: "Layouts", action: () => notImplemented("Saved Layouts") },
        { separator: true },
        {
          label: "Restore Default Layout",
          action: () => dockLayoutRegistry.api?.resetLayout(),
        },
      ],
    },
    {
      id: "help",
      label: "Help",
      items: () => [
        {
          label: "User Manual",
          action: () => openUrl("https://ljantzen.github.io/smaragd/manual/"),
        },
        { separator: true },
        { label: "About", action: () => notImplemented("About") },
      ],
    },
  ];

  function themeItems(): Item[] {
    const items: Item[] = [{ label: "Theme: Default", action: () => setTheme(null) }];
    for (const theme of settingsStore.colorThemes) {
      items.push({ label: `Theme: ${theme.label}`, action: () => setTheme(theme.id) });
    }
    return items;
  }

  function setTheme(id: string | null) {
    settingsStore.value.color_theme = id;
    settingsStore.value = { ...settingsStore.value };
    saveSettingsDebounced();
  }

  let showSettings = $state(false);

  function handleWindowClick(event: MouseEvent) {
    if (!(event.target as HTMLElement).closest(".menu-bar")) close();
  }
</script>

<svelte:window onclick={handleWindowClick} onkeydown={(e) => e.key === "Escape" && close()} />

<nav class="menu-bar">
  {#each menus as menu (menu.id)}
    <div class="menu">
      <button class="menu-label" class:open={openMenu === menu.id} onclick={() => toggle(menu.id)}>
        {menu.label}
      </button>
      {#if openMenu === menu.id}
        <div class="dropdown">
          {#each menu.items() as item, i (i)}
            {#if "separator" in item}
              <div class="separator"></div>
            {:else}
              <button
                class="item"
                disabled={item.disabled}
                onclick={() => run(item.action)}
              >
                <span>{item.label}</span>
                {#if item.shortcutId}
                  <span class="shortcut">{shortcutLabel(item.shortcutId) ?? ""}</span>
                {/if}
              </button>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  {/each}
</nav>

{#if showSettings}
  {#await import("./SettingsDialog.svelte") then { default: SettingsDialog }}
    <SettingsDialog onClose={() => (showSettings = false)} />
  {/await}
{/if}

<style>
  .menu-bar {
    display: flex;
    background: #f3f3f3;
    border-bottom: 1px solid #ccc;
    font-size: 0.85rem;
    user-select: none;
    position: relative;
    z-index: 100;
  }

  .menu {
    position: relative;
  }

  .menu-label {
    background: none;
    border: none;
    padding: 0.35rem 0.7rem;
    cursor: pointer;
  }

  .menu-label.open {
    background: #557;
    color: white;
  }

  .dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    min-width: 16rem;
    background: white;
    border: 1px solid #ccc;
    box-shadow: 0 4px 10px rgba(0, 0, 0, 0.15);
    display: flex;
    flex-direction: column;
    padding: 0.25rem 0;
    max-height: 70vh;
    overflow: auto;
  }

  .item {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    background: none;
    border: none;
    text-align: left;
    padding: 0.35rem 0.8rem;
    cursor: pointer;
    white-space: nowrap;
  }

  .item:hover:not(:disabled) {
    background: #eef;
  }

  .item:disabled {
    color: #aaa;
    cursor: default;
  }

  .shortcut {
    color: #888;
    font-size: 0.75rem;
  }

  .separator {
    height: 1px;
    background: #ddd;
    margin: 0.25rem 0.5rem;
  }
</style>
