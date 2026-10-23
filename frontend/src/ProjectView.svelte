<script lang="ts">
  import DockLayout from "./DockLayout.svelte";
  import { project, closeProject, openProject } from "./stores/project.svelte";

  // Phase 5: the Binder/Editor/Preview/Corkboard/Story Grid views (and the
  // document-pane plumbing) that used to live here directly moved out to
  // the shared project store + DockLayout's dockview panels — this
  // component is now just the project-open screen and the dock shell host.
  let rootInput = $state("");

  let dockLayout = $state<DockLayout>();
</script>

<section class="project">
  {#if !project.value}
    <h2>Open a project</h2>
    <form onsubmit={(e) => { e.preventDefault(); openProject(rootInput); }}>
      <input type="text" placeholder="/path/to/project" bind:value={rootInput} />
      <button type="submit">Open</button>
    </form>
    {#if project.openError}
      <p class="error">{project.openError}</p>
    {/if}
  {:else}
    <div class="toolbar">
      <strong>{project.value.book_title ?? project.value.root}</strong>
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

  .dock-host {
    flex: 1;
    min-height: 30rem;
    border: 1px solid #ccc;
  }

  .error {
    color: #c0392b;
  }
</style>
