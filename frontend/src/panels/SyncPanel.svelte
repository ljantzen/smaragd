<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { project } from "../stores/project.svelte";

  // Phase 6: no pairing UI exists anywhere in the migration plan, so there's
  // no real vault to connect to yet — this runs app-tauri's sync_demo_*
  // commands, a real SyncRunner/SyncEngine against an in-memory fake
  // transport (see core/src/sync/demo.rs), to prove the same push-event
  // architecture collab uses. "Demo" in the label is deliberate: this is
  // not what pairing a real device will look like once that UI exists.
  type SyncEvent =
    | { kind: "Syncing" }
    | {
        kind: "Synced";
        pushed_updates: number;
        pulled_updates: number;
        files_written: number;
        files_created: number;
        files_removed: number;
      }
    | { kind: "Offline"; message: string }
    | { kind: "Failed"; message: string }
    | { kind: "Halted"; reason: string };

  let running = $state(false);
  let lastEvent = $state<SyncEvent | null>(null);
  let history = $state<SyncEvent[]>([]);
  let error = $state<string | null>(null);

  let unlisten: (() => void) | undefined;

  onMount(async () => {
    unlisten = await listen<SyncEvent>("sync://event", (event) => {
      lastEvent = event.payload;
      history = [event.payload, ...history].slice(0, 10);
    });
  });

  onDestroy(() => {
    unlisten?.();
    if (running) invoke("sync_demo_stop");
  });

  async function start() {
    error = null;
    try {
      await invoke("sync_demo_start", { root: project.value!.root });
      running = true;
    } catch (err) {
      error = String(err);
    }
  }

  async function stop() {
    await invoke("sync_demo_stop");
    running = false;
  }

  function syncNow() {
    invoke("sync_demo_now");
  }

  function summarize(event: SyncEvent): string {
    switch (event.kind) {
      case "Syncing":
        return "Syncing…";
      case "Synced":
        return `Synced (pushed ${event.pushed_updates}, pulled ${event.pulled_updates}, wrote ${event.files_written}, created ${event.files_created})`;
      case "Offline":
        return `Offline: ${event.message}`;
      case "Failed":
        return `Failed: ${event.message}`;
      case "Halted":
        return `Halted: ${event.reason}`;
    }
  }
</script>

<div class="sync">
  <p class="note">Demo mode — simulates one device syncing against an in-memory vault.</p>
  <div class="controls">
    {#if !running}
      <button onclick={start}>Start demo sync</button>
    {:else}
      <button onclick={stop}>Stop</button>
      <button onclick={syncNow}>Sync now</button>
    {/if}
  </div>
  {#if error}<p class="error">{error}</p>{/if}
  {#if lastEvent}
    <p class="current">{summarize(lastEvent)}</p>
  {/if}
  <ul class="history">
    {#each history as event, i (i)}
      <li>{summarize(event)}</li>
    {/each}
  </ul>
</div>

<style>
  .sync {
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.5rem;
    box-sizing: border-box;
    overflow: auto;
  }

  .note {
    font-size: 0.8rem;
    color: #888;
    margin: 0;
  }

  .controls {
    display: flex;
    gap: 0.5rem;
  }

  .current {
    font-weight: bold;
  }

  .history {
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: 0.85rem;
    color: #666;
  }

  .error {
    color: #c0392b;
  }
</style>
