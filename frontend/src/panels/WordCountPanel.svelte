<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { project } from "../stores/project.svelte";

  type WordCount = {
    manuscript_only: number;
    everything_except_trash: number;
    draft_target_words: number | null;
    today_target_words: number | null;
    today_words_so_far: number;
  };

  let counts = $state<WordCount | null>(null);
  let error = $state<string | null>(null);

  onMount(refresh);

  async function refresh() {
    try {
      counts = await invoke<WordCount>("word_count", { root: project.value!.root });
    } catch (err) {
      error = String(err);
    }
  }
</script>

<div class="panel">
  <button onclick={refresh}>Refresh</button>
  {#if error}<p class="error">{error}</p>{/if}
  {#if counts}
    <dl>
      <dt>Manuscript</dt>
      <dd>{counts.manuscript_only.toLocaleString()} words</dd>
      <dt>Everything (except Trash)</dt>
      <dd>{counts.everything_except_trash.toLocaleString()} words</dd>
      <dt>Today so far</dt>
      <dd>
        {counts.today_words_so_far.toLocaleString()}
        {#if counts.today_target_words}/ {counts.today_target_words.toLocaleString()} target{/if}
      </dd>
      {#if counts.draft_target_words}
        <dt>Draft target</dt>
        <dd>{counts.draft_target_words.toLocaleString()}</dd>
      {/if}
    </dl>
  {/if}
</div>

<style>
  .panel {
    height: 100%;
    overflow: auto;
    padding: 0.5rem;
    box-sizing: border-box;
  }

  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.3rem 1rem;
    margin-top: 0.75rem;
  }

  dt {
    color: #888;
    font-size: 0.85rem;
  }

  dd {
    margin: 0;
  }

  .error {
    color: #c0392b;
  }
</style>
