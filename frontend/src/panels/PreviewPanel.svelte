<script lang="ts">
  import { project } from "../stores/project.svelte";
</script>

{#if project.selectedPath}
  <div class="preview">{@html project.previewHtml}</div>
{:else}
  <p class="empty">Select a document in the Binder.</p>
{/if}

<style>
  .preview {
    height: 100%;
    overflow: auto;
    padding: 0.5rem 0.75rem;
    box-sizing: border-box;
  }

  .empty {
    padding: 1rem;
    color: #888;
  }
</style>
