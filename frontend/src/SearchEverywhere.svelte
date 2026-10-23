<script lang="ts">
  import { project, openDocument } from "./stores/project.svelte";
  import type { BinderNodeDto } from "./stores/project.svelte";

  // The real Search Everywhere is a modal over documents/text/actions/
  // settings all at once. This is document-title search only — full-text
  // search would mean a new core command scanning every document's
  // contents, out of scope for "wire up the menu item" on its own; the
  // Command Prompt (shortcuts.ts's catalog) already covers the "actions"
  // half of the real modal.
  let { onClose }: { onClose: () => void } = $props();

  let query = $state("");
  let selected = $state(0);

  function collectDocuments(node: BinderNodeDto, out: { name: string; path: string }[]) {
    if (node.kind === "Document") {
      out.push({ name: node.name, path: node.path });
    } else {
      for (const child of node.children) collectDocuments(child, out);
    }
  }

  const allDocuments = $derived.by(() => {
    const out: { name: string; path: string }[] = [];
    if (project.value) collectDocuments(project.value.tree, out);
    return out;
  });

  const results = $derived(
    allDocuments.filter((d) => d.name.toLowerCase().includes(query.toLowerCase())).slice(0, 30),
  );

  function select(path: string) {
    openDocument(path);
    onClose();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      selected = Math.min(selected + 1, results.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
    } else if (e.key === "Enter" && results[selected]) {
      select(results[selected].path);
    }
  }
</script>

<div class="backdrop" onclick={onClose} role="presentation">
  <div
    class="palette"
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <!-- svelte-ignore a11y_autofocus -->
    <input
      type="text"
      placeholder="Search documents…"
      bind:value={query}
      oninput={() => (selected = 0)}
      autofocus
    />
    <ul>
      {#each results as doc, i (doc.path)}
        <li>
          <button class:selected={i === selected} onclick={() => select(doc.path)}>
            {doc.name}
          </button>
        </li>
      {/each}
      {#if results.length === 0}
        <li class="empty">No matching documents</li>
      {/if}
    </ul>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.2);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 15vh;
    z-index: 1000;
  }

  .palette {
    width: 32rem;
    max-width: 90vw;
    max-height: 60vh;
    background: white;
    border-radius: 6px;
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.3);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  input {
    padding: 0.6rem 0.8rem;
    border: none;
    border-bottom: 1px solid #ddd;
    font-size: 0.95rem;
  }

  input:focus {
    outline: none;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0.25rem 0;
    overflow: auto;
  }

  button {
    display: block;
    width: 100%;
    background: none;
    border: none;
    text-align: left;
    padding: 0.4rem 0.8rem;
    cursor: pointer;
    font-size: 0.85rem;
  }

  button.selected {
    background: #eef;
  }

  .empty {
    padding: 0.5rem 0.8rem;
    color: #888;
    font-size: 0.85rem;
  }
</style>
