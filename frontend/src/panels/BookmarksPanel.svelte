<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { project, openDocument } from "../stores/project.svelte";

  type Bookmark = { id: string; path: string; line: number; document_stem: string | null };

  let bookmarks = $state<Bookmark[]>([]);
  let error = $state<string | null>(null);

  onMount(refresh);

  async function refresh() {
    try {
      bookmarks = await invoke<Bookmark[]>("bookmarks_list", { root: project.value!.root });
    } catch (err) {
      error = String(err);
    }
  }

  async function remove(id: string) {
    try {
      await invoke("bookmark_delete", { root: project.value!.root, id });
      await refresh();
    } catch (err) {
      error = String(err);
    }
  }
</script>

<div class="panel">
  {#if error}<p class="error">{error}</p>{/if}
  {#if bookmarks.length === 0}
    <p class="empty">No bookmarks yet.</p>
  {:else}
    <ul>
      {#each bookmarks as bookmark (bookmark.id)}
        <li>
          <button
            class="goto"
            disabled={!bookmark.document_stem}
            onclick={() => openDocument(bookmark.path)}
          >
            {bookmark.document_stem ?? "(not found)"}:{bookmark.line}
          </button>
          <button class="delete" onclick={() => remove(bookmark.id)}>Delete</button>
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
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.2rem 0;
  }

  .goto {
    background: none;
    border: none;
    text-align: left;
    cursor: pointer;
    text-decoration: underline;
  }

  .empty {
    color: #888;
  }

  .error {
    color: #c0392b;
  }
</style>
