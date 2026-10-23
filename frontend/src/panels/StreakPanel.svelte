<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { project } from "../stores/project.svelte";

  type Streak = {
    enabled: boolean;
    status: "disabled" | "insufficient_data" | "green" | "yellow" | "red";
    actual_this_week: number;
    target_this_week: number;
  };

  let streak = $state<Streak | null>(null);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      streak = await invoke<Streak>("streak_status", { root: project.value!.root });
    } catch (err) {
      error = String(err);
    }
  });

  const labels: Record<Streak["status"], string> = {
    disabled: "Streak tracking is off for this project.",
    insufficient_data: "Not enough history yet.",
    green: "On track!",
    yellow: "Behind, but recoverable.",
    red: "Streak broken.",
  };
</script>

<div class="panel">
  {#if error}<p class="error">{error}</p>{/if}
  {#if streak}
    <p class="status {streak.status}">{labels[streak.status]}</p>
    {#if streak.enabled}
      <p>
        This week: {streak.actual_this_week.toLocaleString()} /
        {streak.target_this_week.toLocaleString()} words
      </p>
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

  .status {
    font-weight: bold;
  }

  .status.green {
    color: #2e7d32;
  }

  .status.yellow {
    color: #b8860b;
  }

  .status.red {
    color: #c0392b;
  }

  .error {
    color: #c0392b;
  }
</style>
