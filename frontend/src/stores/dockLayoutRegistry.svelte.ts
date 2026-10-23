// DockLayout.svelte's openPanel/resetLayout are component-instance exports,
// only reachable via bind:this from whatever mounted it (ProjectView). The
// menu bar and global keyboard shortcuts both live above that in the tree
// (App.svelte), so DockLayout registers itself here on mount instead of
// being passed down — the same "shared module state across components that
// don't have a direct parent/child relationship" pattern stores/project.
// svelte.ts already uses, just for an imperative API instead of data.
type DockLayoutApi = {
  openPanel: (id: string) => void;
  resetLayout: () => void;
  listSavedLayouts: () => string[];
  saveCurrentLayoutAs: (name: string) => void;
  loadSavedLayout: (name: string) => void;
};

export const dockLayoutRegistry = $state<{ api: DockLayoutApi | null }>({ api: null });

export function registerDockLayout(api: DockLayoutApi) {
  dockLayoutRegistry.api = api;
}

export function unregisterDockLayout() {
  dockLayoutRegistry.api = null;
}
