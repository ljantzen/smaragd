<script lang="ts">
  import { onMount, onDestroy, mount, unmount } from "svelte";
  import {
    createDockview,
    themeLight,
    type DockviewApi,
    type IContentRenderer,
  } from "dockview-core";
  // Self-injects dockview's default (styled) CSS as a <style> tag on import —
  // this build ships styled/.noStyle pairs rather than a separate stylesheet
  // to import; see the Phase 5 commit message for how that was confirmed.
  import "dockview-core/dist/dockview-core.js";

  import StubPanel from "./panels/StubPanel.svelte";
  import { PANEL_TITLES, PANEL_COMPONENTS } from "./panelRegistry";

  const STORAGE_KEY = "smaragd.dock-layout.v1";

  // Panel set/titles now live in panelRegistry.ts, shared with
  // MobileShell.svelte (Phase 9) — Settings and Daily Notes stay out of it
  // for the same reason noted there: neither is a DockTab in the real app.
  // StubPanel stays imported as the dockview fallback renderer for any
  // future tab added before its real content exists.
  const REAL_COMPONENTS = PANEL_COMPONENTS;

  let container: HTMLDivElement;
  let api: DockviewApi | undefined;
  let persistTimer: ReturnType<typeof setTimeout> | undefined;
  // Mirrors which panel ids are currently open, purely so the Window menu's
  // "open" highlight re-renders — dockview's own panel list isn't itself a
  // Svelte $state, so reading api.getPanel(id) directly in the template
  // wouldn't react to add/remove events.
  let openIds = $state<Set<string>>(new Set());

  function panelIds(): string[] {
    return Object.keys(PANEL_TITLES);
  }

  function svelteContentRenderer(id: string): IContentRenderer {
    const element = document.createElement("div");
    element.style.height = "100%";
    element.style.width = "100%";
    element.style.overflow = "auto";
    element.style.boxSizing = "border-box";
    let instance: object | undefined;
    const Component = REAL_COMPONENTS[id];
    return {
      element,
      init() {
        instance = Component
          ? mount(Component, { target: element })
          : mount(StubPanel, { target: element, props: { title: PANEL_TITLES[id] ?? id } });
      },
      dispose() {
        if (instance) unmount(instance);
      },
    };
  }

  function defaultLayout() {
    if (!api) return;
    // Mirrors `default_dock_state()` in app/src/app/dock.rs: a narrow Binder
    // column on the left, Editor filling the middle, Preview on the right —
    // everything else starts closed, reachable from the Window menu.
    api.addPanel({ id: "editor", component: "editor", title: PANEL_TITLES.editor });
    api.addPanel({
      id: "binder",
      component: "binder",
      title: PANEL_TITLES.binder,
      position: { referencePanel: "editor", direction: "left" },
    });
    api.addPanel({
      id: "preview",
      component: "preview",
      title: PANEL_TITLES.preview,
      position: { referencePanel: "editor", direction: "right" },
    });
  }

  export function resetLayout() {
    if (!api) return;
    api.clear();
    defaultLayout();
    persist();
  }

  export function openPanel(id: string) {
    if (!api) return;
    const existing = api.getPanel(id);
    if (existing) {
      existing.api.setActive();
      return;
    }
    api.addPanel({ id, component: id, title: PANEL_TITLES[id] ?? id });
  }

  function persist() {
    clearTimeout(persistTimer);
    persistTimer = setTimeout(() => {
      if (!api) return;
      localStorage.setItem(STORAGE_KEY, JSON.stringify(api.toJSON()));
    }, 250);
  }

  onMount(() => {
    api = createDockview(container, {
      theme: themeLight,
      createComponent: (options) => svelteContentRenderer(options.name),
    });
    api.onDidLayoutChange(() => persist());
    api.onDidAddPanel((panel) => {
      openIds.add(panel.id);
      openIds = new Set(openIds);
    });
    api.onDidRemovePanel((panel) => {
      openIds.delete(panel.id);
      openIds = new Set(openIds);
    });

    const saved = localStorage.getItem(STORAGE_KEY);
    let restored = false;
    if (saved) {
      try {
        api.fromJSON(JSON.parse(saved));
        restored = true;
      } catch {
        // Falls through to defaultLayout() below — a corrupt or
        // incompatible (e.g. pre-Phase-5) saved layout shouldn't leave the
        // user with no Editor tab at all.
      }
    }
    if (!restored) defaultLayout();
  });

  onDestroy(() => {
    api?.clear();
  });
</script>

<div class="dock-shell">
  <div class="window-menu">
    <span>Window:</span>
    {#each panelIds() as id (id)}
      <button class:open={openIds.has(id)} onclick={() => openPanel(id)}>
        {PANEL_TITLES[id]}
      </button>
    {/each}
    <button class="reset" onclick={resetLayout}>Reset Layout</button>
  </div>
  <div class="dock-container" bind:this={container}></div>
</div>

<style>
  .dock-shell {
    display: flex;
    flex-direction: column;
    height: 100%;
    width: 100%;
  }

  .window-menu {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.3rem;
    padding: 0.3rem 0.5rem;
    border-bottom: 1px solid #ccc;
    font-size: 0.8rem;
  }

  .window-menu button {
    font-size: 0.75rem;
    padding: 0.15rem 0.4rem;
  }

  .window-menu button.open {
    background: #557;
    color: white;
  }

  .window-menu .reset {
    margin-left: auto;
  }

  .dock-container {
    flex: 1;
    min-height: 0;
  }
</style>
