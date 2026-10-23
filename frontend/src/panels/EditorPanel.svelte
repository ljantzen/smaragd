<script lang="ts">
  import MarkdownEditor from "../MarkdownEditor.svelte";
  import { project, onEditorChange, registerEditor } from "../stores/project.svelte";

  // Phase 5: Editor is now its own dockview panel, separate from Preview
  // (they were fused side-by-side in Phase 3's single document pane) —
  // matching the real app's DockTab enum, where Editor and Preview are
  // independently dockable tabs a user can split, stack, or float apart.
  let editor = $state<MarkdownEditor>();

  $effect(() => {
    registerEditor(editor);
    return () => registerEditor(undefined);
  });
</script>

{#if project.selectedPath}
  <MarkdownEditor bind:this={editor} value={project.documentText} onChange={onEditorChange} />
{:else}
  <p class="empty">Select a document in the Binder.</p>
{/if}

<style>
  .empty {
    padding: 1rem;
    color: #888;
  }
</style>
