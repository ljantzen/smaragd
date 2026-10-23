<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import DockLayout from "./DockLayout.svelte";
  import MobileShell from "./MobileShell.svelte";
  import EmptyStateArt from "./EmptyStateArt.svelte";
  import { project, closeProject, openProject } from "./stores/project.svelte";
  import { viewport } from "./stores/viewport.svelte";

  // Phase 5: the Binder/Editor/Preview/Corkboard/Story Grid views (and the
  // document-pane plumbing) that used to live here directly moved out to
  // the shared project store + DockLayout's dockview panels — this
  // component is now just the project-open screen and the dock shell host.
  //
  // Phase 7: the folder picker below is the rfd -> @tauri-apps/plugin-dialog
  // swap from the plan's §4 list (rfd's own call sites included exactly
  // this kind of project-folder picker). "Reveal in file manager" is the
  // opener -> @tauri-apps/plugin-opener swap (the plan's own note that
  // this action has no mobile equivalent — fine here, this is the desktop
  // shell). The raw text-input fallback this screen used until a real
  // picker existed is gone — once Browse… actually opened a native
  // dialog, typing a path by hand had nothing left to justify it (same
  // reasoning that made File > Open Project's own text-input placeholder
  // redundant once it got the same Browse… flow).
  let dockLayout = $state<DockLayout>();

  async function pickFolder() {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") await openProject(selected);
  }

  function revealProjectFolder() {
    if (project.value) revealItemInDir(project.value.root);
  }
</script>

<section class="project">
  {#if !project.value}
    <div class="empty-state">
      <EmptyStateArt />
      <div class="open-screen">
        <h2>Open a project</h2>
        <button onclick={pickFolder}>Browse…</button>
        {#if project.openError}
          <p class="error">{project.openError}</p>
        {/if}
      </div>
    </div>
  {:else if viewport.isMobile}
    <!-- Phase 9: a minimal top bar, not the desktop toolbar — "Reveal in
         file manager" has no mobile equivalent (the plan's own note on
         that action), but Close still needs to live somewhere. A real
         mobile app bar (with a proper back gesture) is a later
         refinement. -->
    <div class="mobile-toolbar">
      <strong>{project.value.book_title ?? project.value.root}</strong>
      <button onclick={closeProject}>Close</button>
    </div>
    <div class="mobile-host">
      <MobileShell />
    </div>
  {:else}
    <div class="toolbar">
      <strong>{project.value.book_title ?? project.value.root}</strong>
      <button onclick={revealProjectFolder}>Reveal in file manager</button>
      <button onclick={closeProject}>Close</button>
    </div>
    <div class="dock-host">
      <DockLayout bind:this={dockLayout} />
    </div>
  {/if}
</section>

<style>
  .project {
    width: 100%;
    height: 100%;
    text-align: left;
    display: flex;
    flex-direction: column;
  }

  .empty-state {
    flex: 1;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .empty-state :global(.empty-state-art) {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    pointer-events: none;
  }

  .open-screen {
    position: relative;
    width: 100%;
    max-width: 40rem;
    margin: 2rem auto;
    padding: 0 1rem;
    box-sizing: border-box;
    text-align: center;
  }

  .toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 1rem;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid #ccc;
  }

  .mobile-toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem;
    border-bottom: 1px solid #ccc;
    font-size: 0.85rem;
  }

  .mobile-toolbar strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mobile-host {
    flex: 1;
    min-height: 0;
  }

  .dock-host {
    flex: 1;
    min-height: 30rem;
    border: 1px solid #ccc;
  }

  .error {
    color: #c0392b;
  }
</style>
