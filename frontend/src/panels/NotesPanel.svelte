<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { project, openDocument } from "../stores/project.svelte";

  // Phase 7: list + delete only — creating a note needs a specific editor
  // cursor position (`upsert_note`'s line/column), which this phase's
  // Editor panel doesn't expose yet. Later refinement.
  type Note = {
    id: string;
    path: string;
    line: number;
    column: number;
    text: string;
    document_stem: string | null;
  };

  let notes = $state<Note[]>([]);
  let error = $state<string | null>(null);

  onMount(refresh);

  async function refresh() {
    try {
      notes = await invoke<Note[]>("notes_list", { root: project.value!.root });
    } catch (err) {
      error = String(err);
    }
  }

  async function remove(id: string) {
    try {
      await invoke("note_delete", { root: project.value!.root, id });
      await refresh();
    } catch (err) {
      error = String(err);
    }
  }
</script>

<div class="panel">
  {#if error}<p class="error">{error}</p>{/if}
  {#if notes.length === 0}
    <p class="empty">No notes yet.</p>
  {:else}
    <ul>
      {#each notes as note (note.id)}
        <li>
          <button
            class="goto"
            disabled={!note.document_stem}
            onclick={() => openDocument(note.path)}
          >
            {note.document_stem ?? "(not found)"}:{note.line}
          </button>
          <p class="text">{note.text}</p>
          <button class="delete" onclick={() => remove(note.id)}>Delete</button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .panel {
    height: 100%;
    overflow: auto;
    padding: 0.5rem;
    box-sizing: border-box;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    padding: 0.3rem 0;
    border-bottom: 1px solid #eee;
  }

  .goto {
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    text-decoration: underline;
    font-weight: bold;
  }

  .text {
    margin: 0.2rem 0;
    font-size: 0.85rem;
  }

  .empty {
    color: #888;
  }

  .error {
    color: #c0392b;
  }
</style>
