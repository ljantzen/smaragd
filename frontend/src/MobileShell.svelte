<script lang="ts">
  import StubPanel from "./panels/StubPanel.svelte";
  import { PANEL_TITLES, PANEL_COMPONENTS, MOBILE_PRIMARY_PANELS } from "./panelRegistry";

  // Phase 9: "single-panel-at-a-time navigation, not simultaneous dock
  // panes" per the migration plan's Phase 9 row — dockview's split/float/
  // drag mechanics (DockLayout.svelte) are a desktop-mouse interaction
  // model that doesn't map onto a phone screen at all, so mobile gets its
  // own much simpler shell: one panel visible full-screen, switched via a
  // bottom tab bar, reusing the exact same panel components (they're
  // zero-prop, reading the shared project store directly, since Phase 5 —
  // no new panel code needed for mobile at all).
  let activePanel = $state("binder");
  let showMore = $state(false);

  const otherPanels = Object.keys(PANEL_TITLES).filter(
    (id) => !MOBILE_PRIMARY_PANELS.includes(id),
  );

  function select(id: string) {
    activePanel = id;
    showMore = false;
  }

  let ActiveComponent = $derived(PANEL_COMPONENTS[activePanel] ?? StubPanel);
</script>

<div class="mobile-shell">
  <div class="panel-host">
    <ActiveComponent title={PANEL_TITLES[activePanel] ?? activePanel} />
  </div>

  {#if showMore}
    <div class="more-sheet">
      {#each otherPanels as id (id)}
        <button class:active={activePanel === id} onclick={() => select(id)}>
          {PANEL_TITLES[id]}
        </button>
      {/each}
    </div>
  {/if}

  <nav class="tab-bar">
    {#each MOBILE_PRIMARY_PANELS as id (id)}
      <button class:active={activePanel === id} onclick={() => select(id)}>
        {PANEL_TITLES[id]}
      </button>
    {/each}
    <button
      class:active={showMore || otherPanels.includes(activePanel)}
      onclick={() => (showMore = !showMore)}
    >
      More
    </button>
  </nav>
</div>

<style>
  .mobile-shell {
    display: flex;
    flex-direction: column;
    /* Dynamic viewport height, not 100% — shrinks with the on-screen
       keyboard in supporting mobile WebViews (WKWebView/Chromium) so the
       tab bar and active panel stay above it rather than being pushed
       off-screen or obscured. Not verified on a real device/simulator —
       see this phase's commit message. */
    height: 100dvh;
    width: 100%;
    position: relative;
  }

  .panel-host {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .more-sheet {
    position: absolute;
    bottom: 100%;
    left: 0;
    right: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    padding: 0.5rem;
    background: white;
    border-top: 1px solid #ccc;
    box-shadow: 0 -2px 8px rgba(0, 0, 0, 0.08);
  }

  .more-sheet button {
    padding: 0.4rem 0.6rem;
  }

  .tab-bar {
    display: flex;
    border-top: 1px solid #ccc;
    /* iOS home indicator / Android gesture bar safe area. */
    padding-bottom: env(safe-area-inset-bottom, 0);
  }

  .tab-bar button {
    flex: 1;
    padding: 0.6rem 0.3rem;
    font-size: 0.75rem;
    background: none;
    border: none;
    border-radius: 0;
  }

  .tab-bar button.active,
  .more-sheet button.active {
    background: #557;
    color: white;
  }
</style>
