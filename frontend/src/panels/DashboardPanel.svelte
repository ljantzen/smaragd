<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { project } from "../stores/project.svelte";

  // Phase 7: summary numbers only — the real dashboard_panel.rs's
  // activity-by-weekday/by-hour bar charts are egui_plot-specific and would
  // need a charting library on the frontend (see the migration plan's §3
  // notes on not porting custom-painted panels wholesale). Later refinement.
  type Dashboard = {
    session_count: number;
    total_minutes: number;
    total_words: number;
    documents_created: number;
  };

  let dashboard = $state<Dashboard | null>(null);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      dashboard = await invoke<Dashboard>("dashboard_summary", { root: project.value!.root });
    } catch (err) {
      error = String(err);
    }
  });

  function formatMinutes(minutes: number): string {
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    return hours > 0 ? `${hours}h ${rest}m` : `${rest}m`;
  }
</script>

<div class="panel">
  {#if error}<p class="error">{error}</p>{/if}
  {#if dashboard}
    <dl>
      <dt>Sessions</dt>
      <dd>{dashboard.session_count}</dd>
      <dt>Total writing time</dt>
      <dd>{formatMinutes(dashboard.total_minutes)}</dd>
      <dt>Words written across sessions</dt>
      <dd>{dashboard.total_words.toLocaleString()}</dd>
      <dt>Documents created (all time)</dt>
      <dd>{dashboard.documents_created}</dd>
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
    gap: 0.4rem 1rem;
  }

  dt {
    color: #888;
    font-size: 0.85rem;
  }

  dd {
    margin: 0;
    font-weight: bold;
  }

  .error {
    color: #c0392b;
  }
</style>
