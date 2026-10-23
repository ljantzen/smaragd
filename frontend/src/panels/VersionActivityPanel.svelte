<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { project } from "../stores/project.svelte";

  // Phase 7: dirty files + recent commit log only — the real
  // version_activity_panel.rs also shows a live log of this session's own
  // git actions (auto-commits, push/pull attempts), which needs app-level
  // session state not available to a stateless command. Later refinement.
  type Commit = { hash: string; author: string; relative_date: string; subject: string };
  type VersionActivity = {
    git_available: boolean;
    dirty_files: string[];
    commits: Commit[];
  };

  let activity = $state<VersionActivity | null>(null);
  let error = $state<string | null>(null);

  onMount(refresh);

  async function refresh() {
    try {
      activity = await invoke<VersionActivity>("version_activity", { root: project.value!.root });
    } catch (err) {
      error = String(err);
    }
  }
</script>

<div class="panel">
  <button onclick={refresh}>Refresh</button>
  {#if error}<p class="error">{error}</p>{/if}
  {#if activity}
    {#if !activity.git_available}
      <p class="empty">Git integration isn't turned on for this project.</p>
    {:else}
      <h3>Dirty files ({activity.dirty_files.length})</h3>
      <ul class="dirty">
        {#each activity.dirty_files as file (file)}
          <li>{file}</li>
        {/each}
      </ul>
      <h3>Recent commits</h3>
      <ul class="commits">
        {#each activity.commits as commit (commit.hash)}
          <li>
            <code>{commit.hash.slice(0, 7)}</code>
            <span class="subject">{commit.subject}</span>
            <span class="meta">{commit.author}, {commit.relative_date}</span>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</div>

<style>
  .panel {
    height: 100%;
    overflow: auto;
    padding: 0.5rem;
    box-sizing: border-box;
  }

  h3 {
    font-size: 0.85rem;
    color: #888;
    margin: 0.75rem 0 0.25rem;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: 0.85rem;
  }

  .commits li {
    display: flex;
    gap: 0.5rem;
    align-items: baseline;
    padding: 0.15rem 0;
  }

  .subject {
    flex: 1;
  }

  .meta {
    color: #888;
    font-size: 0.75rem;
  }

  .empty {
    color: #888;
  }

  .error {
    color: #c0392b;
  }
</style>
