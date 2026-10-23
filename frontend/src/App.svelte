<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  // Phase 1 proof-of-concept: the first real IPC round trip (one command,
  // one push event) through the Tauri shell, backed by the real
  // smaragd_core::pomodoro state machine — see app-tauri/src/pomodoro.rs.
  type PomodoroSnapshot = {
    phase: string;
    remaining_secs: number;
    running: boolean;
    completed_work_sessions: number;
  };

  let snapshot: PomodoroSnapshot | null = $state(null);

  function formatRemaining(totalSeconds: number): string {
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${minutes}:${seconds.toString().padStart(2, "0")}`;
  }

  onMount(() => {
    invoke<PomodoroSnapshot>("pomodoro_snapshot").then((s) => (snapshot = s));

    const unlisten = listen<PomodoroSnapshot>("pomodoro://tick", (event) => {
      snapshot = event.payload;
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  });
</script>

<main>
  <h1>Smaragd (Tauri shell — Phase 1 scaffold)</h1>

  {#if snapshot}
    <section class="pomodoro">
      <p class="phase">{snapshot.phase}</p>
      <p class="remaining">{formatRemaining(snapshot.remaining_secs)}</p>
      <p class="sessions">{snapshot.completed_work_sessions} work sessions completed</p>
      <div class="controls">
        {#if snapshot.running}
          <button onclick={() => invoke("pomodoro_pause")}>Pause</button>
        {:else}
          <button onclick={() => invoke("pomodoro_start")}>Start</button>
        {/if}
        <button onclick={() => invoke("pomodoro_skip")}>Skip</button>
      </div>
    </section>
  {:else}
    <p>Loading…</p>
  {/if}
</main>

<style>
  main {
    font-family: system-ui, sans-serif;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1rem;
    padding: 2rem;
  }

  .pomodoro {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
  }

  .remaining {
    font-size: 3rem;
    font-variant-numeric: tabular-nums;
    margin: 0;
  }

  .phase {
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: #888;
    margin: 0;
  }

  .controls {
    display: flex;
    gap: 0.5rem;
  }

  button {
    padding: 0.5rem 1.25rem;
    font-size: 1rem;
    cursor: pointer;
  }
</style>
