<script lang="ts">
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { project, closeProject, openProject, openFindReplace } from "./stores/project.svelte";
  import AboutDialog from "./AboutDialog.svelte";
  import { dockLayoutRegistry } from "./stores/dockLayoutRegistry.svelte";
  import { settingsStore, saveSettingsDebounced } from "./stores/settings.svelte";
  import { SHORTCUT_ACTIONS, effectiveCombo, runShortcutAction, registerShortcutHandler } from "./shortcuts";
  import { notImplemented, showToast } from "./stores/toast.svelte";

  // Desktop menu bar matching the real egui app's 8-menu structure
  // (app/src/app/menu_bar.rs) as closely as this port's actual
  // functionality allows. Every item from that file is represented; most
  // now call a real action — Export, Find & Replace, Command Prompt,
  // Search Everywhere, git Commit/Push/Pull/Commit+Push, saved named
  // Layouts, About, and docx/epub/PDF/Scrivener Import were all stubbed
  // notImplemented() placeholders until this pass, listed for structural
  // fidelity first and wired for real once a user asked for them.
  //
  // Still genuinely stubbed, deliberately: New Project, Recent Projects,
  // Open Document (project-lifecycle UI beyond a folder picker, out of
  // this pass's scope), Project Settings (a whole second dialog, not built),
  // Exit (no Tauri equivalent wired), Focus Mode / Toggle Spell Check
  // (features that don't exist in this build), and Reload Plugins/Enable
  // Project Plugins — that one is different in kind, not just unfinished:
  // there is no plugin engine anywhere in this Tauri port (no Rhai
  // scripting, no :command registration), so there's nothing to reload.
  // It's left as a clear "no plugin engine in this build" message rather
  // than faked as if a real action were just missing its wiring.
  //
  // Submenus (Recent Projects, Import's per-format choice, Color Binder
  // By) are flattened or replaced with a native file-picker's own format
  // filter rather than built as true nested flyouts — a scope cut.
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

  // "Use the open screen" was a dead end: it isn't visible once a project
  // is already open, which is exactly when someone wants File > Open
  // Project to switch to a different one. Same folder-picker + openProject
  // flow ProjectView.svelte's own "Browse…" button uses.
  async function pickAndOpenProject() {
    const selected = await openFileDialog({ directory: true, multiple: false });
    if (typeof selected === "string") await openProject(selected);
  }

  async function pickAndImport(format: "docx" | "epub" | "pdf") {
    if (!project.value) return;
    const extensions: Record<string, string[]> = {
      docx: ["docx"],
      epub: ["epub"],
      pdf: ["pdf"],
    };
    const path = await openFileDialog({
      multiple: false,
      filters: [{ name: format.toUpperCase(), extensions: extensions[format] }],
    });
    if (typeof path !== "string") return;
    try {
      await invoke("import_file", {
        root: project.value.root,
        parent: project.value.root,
        format,
        filePath: path,
      });
      showToast("Imported");
    } catch (err) {
      showToast(`Import failed: ${err}`);
    }
  }

  async function pickAndImportScrivener() {
    if (!project.value) return;
    const dir = await openFileDialog({ directory: true, multiple: false });
    if (typeof dir !== "string") return;
    try {
      await invoke("import_scrivener", {
        root: project.value.root,
        parent: project.value.root,
        scrivenerDir: dir,
      });
      showToast("Imported");
    } catch (err) {
      showToast(`Import failed: ${err}`);
    }
  }

  function reloadPlugins() {
    showToast("No plugin engine in this build — there's nothing to reload yet.");
  }

  async function saveCurrentLayout() {
    const name = window.prompt("Save current layout as:");
    if (!name) return;
    dockLayoutRegistry.api?.saveCurrentLayoutAs(name);
    showToast(`Layout "${name}" saved`);
  }

  function layoutItems(): Item[] {
    const names = dockLayoutRegistry.api?.listSavedLayouts() ?? [];
    if (names.length === 0) {
      return [{ label: "No saved layouts", action: () => {}, disabled: true }];
    }
    return names.map((name) => ({
      label: name,
      action: () => dockLayoutRegistry.api?.loadSavedLayout(name),
    }));
  }

  const menus: { id: string; label: string; items: () => Item[] }[] = [
    {
      id: "file",
      label: "File",
      items: () => [
        { label: "New Project…", action: () => notImplemented("New Project") },
        { label: "Open Project…", action: pickAndOpenProject },
        { label: "Recent Projects", action: () => notImplemented("Recent Projects") },
        { label: "Close Project", action: closeProject, disabled: !hasProject },
        { separator: true },
        { label: "Open Document…", action: () => notImplemented("Open Document") },
        { label: "Search Everywhere…", action: () => runShortcutAction("search_everywhere") },
        { label: "Close Document", action: () => runShortcutAction("close_document") },
        { label: "Go Back", action: () => runShortcutAction("go_back") },
        { label: "Go Forward", action: () => runShortcutAction("go_forward") },
        { separator: true },
        { label: "Import Word Document (.docx)…", action: () => pickAndImport("docx") },
        { label: "Import EPUB (.epub)…", action: () => pickAndImport("epub") },
        { label: "Import PDF (.pdf)…", action: () => pickAndImport("pdf") },
        { label: "Import Scrivener Project…", action: pickAndImportScrivener },
        { label: "Export Manuscript…", action: () => (showExport = true), disabled: !hasProject },
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
        { separator: true },
        { label: "Reload Plugins", action: reloadPlugins },
        { label: "Enable Project Plugins", action: reloadPlugins },
      ],
    },
    {
      id: "versions",
      label: "Versions",
      items: () => [
        { label: "Commit", action: () => runShortcutAction("git_commit"), disabled: !hasProject },
        {
          label: "Commit and Push",
          action: () => runShortcutAction("git_commit_and_push"),
          disabled: !hasProject,
        },
        { label: "Push", action: () => runShortcutAction("git_push"), disabled: !hasProject },
        { label: "Pull", action: () => runShortcutAction("git_pull"), disabled: !hasProject },
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
        { label: "Save Current Layout…", action: saveCurrentLayout },
        { label: "Layouts", action: () => {}, disabled: true },
        ...layoutItems(),
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
        { label: "About", action: () => (showAbout = true) },
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
  let showAbout = $state(false);
  let showCommandPrompt = $state(false);
  let showSearchEverywhere = $state(false);
  let showExport = $state(false);
  let showGitCommit = $state(false);
  let gitCommitAndPush = $state(false);

  onMount(() => {
    registerShortcutHandler("find_replace", openFindReplace);
    registerShortcutHandler("command_prompt", () => (showCommandPrompt = true));
    registerShortcutHandler("search_everywhere", () => (showSearchEverywhere = true));
    registerShortcutHandler("git_commit", () => {
      gitCommitAndPush = false;
      showGitCommit = true;
    });
    registerShortcutHandler("git_commit_and_push", () => {
      gitCommitAndPush = true;
      showGitCommit = true;
    });
    registerShortcutHandler("git_push", async () => {
      if (!project.value) return;
      try {
        const result = await invoke<{ files: string[] }>("git_push", { root: project.value.root });
        showToast(`Pushed (${result.files.length} file(s))`);
      } catch (err) {
        showToast(`Push failed: ${err}`);
      }
    });
    registerShortcutHandler("git_pull", async () => {
      if (!project.value) return;
      try {
        const result = await invoke<{ files: string[] }>("git_pull", { root: project.value.root });
        showToast(`Pulled (${result.files.length} file(s))`);
      } catch (err) {
        showToast(`Pull failed: ${err}`);
      }
    });
  });

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

{#if showAbout}
  <AboutDialog onClose={() => (showAbout = false)} />
{/if}

{#if showCommandPrompt}
  {#await import("./CommandPrompt.svelte") then { default: CommandPrompt }}
    <CommandPrompt onClose={() => (showCommandPrompt = false)} />
  {/await}
{/if}

{#if showSearchEverywhere}
  {#await import("./SearchEverywhere.svelte") then { default: SearchEverywhere }}
    <SearchEverywhere onClose={() => (showSearchEverywhere = false)} />
  {/await}
{/if}

{#if showExport}
  {#await import("./ExportDialog.svelte") then { default: ExportDialog }}
    <ExportDialog onClose={() => (showExport = false)} />
  {/await}
{/if}

{#if showGitCommit}
  {#await import("./GitCommitDialog.svelte") then { default: GitCommitDialog }}
    <GitCommitDialog onClose={() => (showGitCommit = false)} andPush={gitCommitAndPush} />
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
