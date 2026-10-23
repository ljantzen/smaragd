<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { project, openDocument } from "../stores/project.svelte";

  // Phase 7: real data via Project::backlinks, re-fetched whenever the
  // selected document changes (same "recompute fresh from disk" convention
  // every other Project query command already uses).
  type Backlink = { source_path: string; source_title: string; snippet: string };

  let backlinks = $state<Backlink[]>([]);
  let error = $state<string | null>(null);

  $effect(() => {
    const path = project.selectedPath;
    if (!path) {
      backlinks = [];
      return;
    }
    invoke<Backlink[]>("backlinks_for", { root: project.value!.root, path })
      .then((result) => (backlinks = result))
      .catch((err) => (error = String(err)));
  });
</script>

<div class="panel">
  {#if !project.selectedPath}
    <p class="empty">Select a document in the Binder.</p>
  {:else if error}
    <p class="error">{error}</p>
  {:else if backlinks.length === 0}
    <p class="empty">No documents link here.</p>
  {:else}
    <ul>
      {#each backlinks as link (link.source_path + link.snippet)}
        <li>
          <button onclick={() => openDocument(link.source_path)}>{link.source_title}</button>
          <p class="snippet">{link.snippet}</p>
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
    margin-bottom: 0.5rem;
  }

  li button {
    background: none;
    border: none;
    padding: 0;
    font-weight: bold;
    cursor: pointer;
    text-decoration: underline;
  }

  .snippet {
    margin: 0.15rem 0 0;
    color: #666;
    font-size: 0.85rem;
  }

  .empty {
    color: #888;
  }

  .error {
    color: #c0392b;
  }
</style>
