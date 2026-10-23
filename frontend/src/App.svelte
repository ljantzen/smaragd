<script lang="ts">
  import { onMount } from "svelte";
  import ProjectView from "./ProjectView.svelte";
  import MenuBar from "./MenuBar.svelte";
  import { viewport } from "./stores/viewport.svelte";
  import { loadSettings } from "./stores/settings.svelte";
  import { toast } from "./stores/toast.svelte";
  import { handleGlobalKeydown, installBuiltinShortcutHandlers } from "./shortcuts";

  // Menu bar + global keyboard shortcuts are desktop-only — mobile already
  // has its own navigation (MobileShell.svelte, Phase 9) and no keyboard to
  // bind against most of these to.
  onMount(() => {
    loadSettings();
    installBuiltinShortcutHandlers();
    window.addEventListener("keydown", handleGlobalKeydown);
    return () => window.removeEventListener("keydown", handleGlobalKeydown);
  });
</script>

<main>
  {#if !viewport.isMobile}
    <MenuBar />
  {/if}
  <div class="body">
    <ProjectView />
  </div>
  {#if toast.message}
    <div class="toast">{toast.message}</div>
  {/if}
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
    flex-direction: column;
    height: 100%;
    box-sizing: border-box;
    position: relative;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    justify-content: center;
    padding: 1rem;
    box-sizing: border-box;
  }

  /* Phase 9: edge-to-edge on phone-width viewports — MobileShell already
     has its own padding/safe-area handling, and the 1rem gutter here just
     wastes screen space a phone doesn't have to spare. */
  @media (max-width: 819px) {
    .body {
      padding: 0;
    }
  }

  .toast {
    position: fixed;
    bottom: 1rem;
    left: 50%;
    transform: translateX(-50%);
    background: #333;
    color: white;
    padding: 0.5rem 1rem;
    border-radius: 4px;
    font-size: 0.85rem;
    z-index: 2000;
  }
</style>
