<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    type BinderNodeDto,
    project,
    openDocument,
    onTreeChange,
  } from "./stores/project.svelte";

  // Phase 4: the real Binder, replacing Phase 2's read-only recursive list —
  // create/rename/delete and drag-and-drop move/reorder, all round-tripping
  // through core's Project via the binder_* commands. Context actions are
  // plain hover-revealed buttons rather than a native context menu/dialog
  // system (not built yet), and new-item naming uses the browser's own
  // prompt() rather than an in-app modal — both deliberate simplifications
  // to keep this phase's scope to "the tree + DnD interaction, working
  // end-to-end", not the full binder_panel.rs feature set (templates, role
  // badges, picklist-source toggles, status colors, trash restore, ...).
  //
  // Phase 5: reads/writes the shared project store directly rather than
  // taking root/tree/selectedPath/onSelect/onTreeChange as props — this
  // component is now mounted standalone inside a dockview panel, with no
  // parent to pass them down from.
  const root = $derived(project.value!.root);
  const tree = $derived(project.value!.tree);

  let error = $state<string | null>(null);
  let dragOverPath = $state<string | null>(null);

  async function run<T>(promise: Promise<T>): Promise<T | undefined> {
    error = null;
    try {
      return await promise;
    } catch (err) {
      error = String(err);
      return undefined;
    }
  }

  async function newDocument(parent: string) {
    const name = window.prompt("New document name:", "Untitled");
    if (!name) return;
    const result = await run(
      invoke<{ tree: BinderNodeDto; path: string }>("binder_create_document", {
        root,
        parent,
        name,
      }),
    );
    if (result) {
      onTreeChange(result.tree);
      openDocument(result.path);
    }
  }

  async function newFolder(parent: string) {
    const name = window.prompt("New folder name:", "New Folder");
    if (!name) return;
    const newTree = await run(
      invoke<BinderNodeDto>("binder_create_folder", { root, parent, name }),
    );
    if (newTree) onTreeChange(newTree);
  }

  async function rename(path: string, currentName: string) {
    const newName = window.prompt("Rename to:", currentName);
    if (!newName || newName === currentName) return;
    const newTree = await run(
      invoke<BinderNodeDto>("binder_rename", { root, path, newName }),
    );
    if (newTree) onTreeChange(newTree);
  }

  async function remove(path: string, name: string) {
    if (!window.confirm(`Delete "${name}"?`)) return;
    const newTree = await run(invoke<BinderNodeDto>("binder_delete", { root, path }));
    if (newTree) onTreeChange(newTree);
  }

  // Phase 9: a non-drag way to move an item. HTML5 drag-and-drop (the
  // existing onDropOnFolder/onDropOnDocument handlers below) simply never
  // fires on touch — iOS Safari/WKWebView and Android Chrome don't
  // implement the HTML5 DnD spec for touch input at all, only mouse — so
  // without this, moving anything would be impossible on mobile, not just
  // inconvenient. A folder-path prompt rather than a visual picker, same
  // "ship the interaction, not the polish" tradeoff every other action
  // here already makes.
  async function moveTo(path: string, name: string) {
    const destination = window.prompt(`Move "${name}" to (absolute folder path):`, root);
    if (!destination) return;
    const newTree = await run(
      invoke<BinderNodeDto>("binder_move", { root, path, newParent: destination }),
    );
    if (newTree) onTreeChange(newTree);
  }

  function onDragStart(e: DragEvent, path: string) {
    e.dataTransfer?.setData("text/plain", path);
    e.dataTransfer!.effectAllowed = "move";
  }

  function onDragOver(e: DragEvent, path: string) {
    e.preventDefault();
    dragOverPath = path;
  }

  function onDragLeave(path: string) {
    if (dragOverPath === path) dragOverPath = null;
  }

  async function onDropOnFolder(e: DragEvent, folderPath: string) {
    e.preventDefault();
    dragOverPath = null;
    const dragged = e.dataTransfer?.getData("text/plain");
    if (!dragged || dragged === folderPath) return;
    const newTree = await run(
      invoke<BinderNodeDto>("binder_move", { root, path: dragged, newParent: folderPath }),
    );
    if (newTree) onTreeChange(newTree);
  }

  async function onDropOnDocument(e: DragEvent, beforePath: string) {
    e.preventDefault();
    dragOverPath = null;
    const dragged = e.dataTransfer?.getData("text/plain");
    if (!dragged || dragged === beforePath) return;
    const newTree = await run(
      invoke<BinderNodeDto>("binder_move_before", { root, path: dragged, before: beforePath }),
    );
    if (newTree) onTreeChange(newTree);
  }
</script>

{#snippet node(n: BinderNodeDto)}
  {#if n.kind === "Folder"}
    <details open>
      <summary
        class:drag-over={dragOverPath === n.path}
        draggable="true"
        ondragstart={(e) => onDragStart(e, n.path)}
        ondragover={(e) => onDragOver(e, n.path)}
        ondragleave={() => onDragLeave(n.path)}
        ondrop={(e) => onDropOnFolder(e, n.path)}
      >
        <span class="name">{n.name}</span>
        <span class="actions">
          <button title="New document" onclick={() => newDocument(n.path)}>+file</button>
          <button title="New folder" onclick={() => newFolder(n.path)}>+folder</button>
          <button title="Rename" onclick={() => rename(n.path, n.name)}>rename</button>
          <button title="Move" onclick={() => moveTo(n.path, n.name)}>move</button>
          <button title="Delete" onclick={() => remove(n.path, n.name)}>del</button>
        </span>
      </summary>
      <ul>
        {#each n.children as child (child.path)}
          <li>{@render node(child)}</li>
        {/each}
      </ul>
    </details>
  {:else}
    <div
      class="doc-row"
      role="group"
      class:drag-over={dragOverPath === n.path}
      ondragover={(e) => onDragOver(e, n.path)}
      ondragleave={() => onDragLeave(n.path)}
      ondrop={(e) => onDropOnDocument(e, n.path)}
    >
      <button
        class="doc"
        class:selected={project.selectedPath === n.path}
        draggable="true"
        ondragstart={(e) => onDragStart(e, n.path)}
        onclick={() => openDocument(n.path)}
      >
        {n.name}
      </button>
      <span class="actions">
        <button title="Rename" onclick={() => rename(n.path, n.name)}>rename</button>
        <button title="Move" onclick={() => moveTo(n.path, n.name)}>move</button>
        <button title="Delete" onclick={() => remove(n.path, n.name)}>del</button>
      </span>
    </div>
  {/if}
{/snippet}

<nav class="tree">
  {@render node(tree)}
  {#if error}<p class="error">{error}</p>{/if}
</nav>

<style>
  .tree {
    border: 1px solid #ccc;
    padding: 0.5rem;
    overflow: auto;
  }

  .tree ul {
    list-style: none;
    margin: 0;
    padding-left: 1rem;
  }

  summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    cursor: pointer;
  }

  .doc-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .doc-row .doc {
    flex: 1;
    text-align: left;
    background: none;
    border: none;
    padding: 0.2rem 0.3rem;
    cursor: pointer;
  }

  .doc-row .doc.selected {
    background: #eef;
  }

  .drag-over {
    outline: 2px dashed #557;
  }

  .actions {
    display: none;
    gap: 0.25rem;
    white-space: nowrap;
  }

  /* Phase 9: hover-reveal only where hover actually exists. A touch
     device has no hover state at all, so these buttons were simply
     unreachable there before this — always-visible is the fix, not a
     tap-to-reveal step, since that would just move the same problem one
     level up (the first tap would need *something* to be visible). */
  @media (hover: hover) {
    summary:hover .actions,
    .doc-row:hover .actions {
      display: inline-flex;
    }
  }

  @media (hover: none) {
    .actions {
      display: inline-flex;
    }
  }

  .actions button {
    font-size: 0.7rem;
    padding: 0.1rem 0.3rem;
  }

  .error {
    color: #c0392b;
  }
</style>
