<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import DockLayout from "./DockLayout.svelte";
  import MobileShell from "./MobileShell.svelte";
  import { project, closeProject, openProject } from "./stores/project.svelte";
  import { viewport } from "./stores/viewport.svelte";

  // Phase 5: the Binder/Editor/Preview/Corkboard/Story Grid views (and the
  // document-pane plumbing) that used to live here directly moved out to
  // the shared project store + DockLayout's dockview panels — this
  // component is now just the project-open screen and the dock shell host.
  //
  // Phase 7: the folder picker below is the rfd -> @tauri-apps/plugin-dialog
  // swap from the plan's §4 list (rfd's own call sites included exactly
  // this kind of project-folder picker) — replacing the raw text input
  // Phase 2 used as a placeholder until a real picker existed. "Reveal in
  // file manager" is the opener -> @tauri-apps/plugin-opener swap (the
  // plan's own note that this action has no mobile equivalent — fine here,
  // this is the desktop shell).
  let rootInput = $state("");

  let dockLayout = $state<DockLayout>();

  async function pickFolder() {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      rootInput = selected;
      await openProject(selected);
    }
  }

  function revealProjectFolder() {
    if (project.value) revealItemInDir(project.value.root);
  }
</script>

<section class="project">
  {#if !project.value}
    <h2>Open a project</h2>
    <form onsubmit={(e) => { e.preventDefault(); openProject(rootInput); }}>
      <input type="text" placeholder="/path/to/project" bind:value={rootInput} />
      <button type="button" onclick={pickFolder}>Browse…</button>
      <button type="submit">Open</button>
    </form>
    {#if project.openError}
      <p class="error">{project.openError}</p>
    {/if}
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
    max-width: 70rem;
    height: 100%;
    text-align: left;
    display: flex;
    flex-direction: column;
  }

  form {
    display: flex;
    gap: 0.5rem;
  }

  form input {
    flex: 1;
    padding: 0.4rem;
  }

  .toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 1rem;
    margin-bottom: 0.5rem;
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
