<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { project, openDocument } from "../stores/project.svelte";

  type TagGroup = { tag: string; documents: [string, string][] };

  let tags = $state<string[]>([]);
  let selectedTag = $state<string | null>(null);
  let group = $state<TagGroup | null>(null);
  let error = $state<string | null>(null);

  onMount(refreshTags);

  async function refreshTags() {
    try {
      tags = await invoke<string[]>("all_tags", { root: project.value!.root });
    } catch (err) {
      error = String(err);
    }
  }

  async function selectTag(tag: string) {
    selectedTag = tag;
    try {
      group = await invoke<TagGroup>("documents_with_tag", { root: project.value!.root, tag });
    } catch (err) {
      error = String(err);
    }
  }
</script>

<div class="panel">
  {#if error}<p class="error">{error}</p>{/if}
  <div class="columns">
    <ul class="tag-list">
      {#each tags as tag (tag)}
        <li>
          <button class:selected={selectedTag === tag} onclick={() => selectTag(tag)}>
            #{tag}
          </button>
        </li>
      {/each}
    </ul>
    <div class="documents">
      {#if group}
        {#each group.documents as [path, title] (path)}
          <button class="doc" onclick={() => openDocument(path)}>{title}</button>
        {/each}
      {:else}
        <p class="empty">Select a tag.</p>
      {/if}
    </div>
  </div>
</div>

<style>
  .panel {
    height: 100%;
    box-sizing: border-box;
    padding: 0.5rem;
    overflow: auto;
  }

  .columns {
    display: grid;
    grid-template-columns: 10rem 1fr;
    gap: 1rem;
    height: 100%;
  }

  .tag-list {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: auto;
  }

  .tag-list button,
  .doc {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    padding: 0.2rem 0.3rem;
    cursor: pointer;
  }

  .tag-list button.selected {
    background: #eef;
  }

  .documents {
    overflow: auto;
  }

  .empty {
    color: #888;
  }

  .error {
    color: #c0392b;
  }
</style>
