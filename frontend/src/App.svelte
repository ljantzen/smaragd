<script lang="ts">
  import ProjectView from "./ProjectView.svelte";
</script>

<main>
  <ProjectView />
</main>

<style>
  :global(html, body) {
    /* Dynamic viewport height — shrinks with the on-screen keyboard on
       mobile WebViews instead of leaving stale space behind it the way
       100vh does. Behaves identically to 100vh on desktop/no-keyboard. */
    height: 100dvh;
    margin: 0;
  }

  :global(#app) {
    height: 100%;
  }

  main {
    font-family: system-ui, sans-serif;
    display: flex;
    justify-content: center;
    height: 100%;
    padding: 1rem;
    box-sizing: border-box;
  }

  /* Phase 9: edge-to-edge on phone-width viewports — MobileShell already
     has its own padding/safe-area handling, and the 1rem gutter here just
     wastes screen space a phone doesn't have to spare. */
  @media (max-width: 819px) {
    main {
      padding: 0;
    }
  }
</style>
