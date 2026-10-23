<script lang="ts">
  import ProjectView from "./ProjectView.svelte";
</script>

<main>
  <ProjectView />
</main>

<style>
  :global(html, body) {
    height: 100%;
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
</style>
