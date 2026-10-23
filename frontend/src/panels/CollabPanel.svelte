<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  // Phase 6: real push-event collab, not a per-frame poll — see
  // app-tauri/src/collab.rs for the wake-driven drain-thread architecture.
  // Deliberately a standalone shared textarea, not wired into the Editor
  // panel's CodeMirror buffer: two-way sync between CodeMirror's own change
  // stream and remote collab updates (without each fighting the other) is a
  // real design problem on its own, left for later once this phase's actual
  // target — proving the push-event architecture — is settled.
  type CollabUpdate =
    | { kind: "HostReady"; code: string }
    | { kind: "TextChanged"; new_text: string }
    | { kind: "PeerConnected"; fingerprint: string }
    | { kind: "Reconnecting" }
    | { kind: "PeerDisconnected" }
    | { kind: "Error"; message: string };

  type CollabStatus = {
    active: boolean;
    role: "host" | "joiner" | null;
    code: string | null;
    peer_connected: boolean;
    reconnecting: boolean;
    peer_fingerprint: string | null;
    session_ended: boolean;
  };

  let status = $state<CollabStatus>({
    active: false,
    role: null,
    code: null,
    peer_connected: false,
    reconnecting: false,
    peer_fingerprint: null,
    session_ended: false,
  });
  let joinCode = $state("");
  let sharedText = $state("");
  let error = $state<string | null>(null);

  async function refreshStatus() {
    status = await invoke<CollabStatus>("collab_status");
  }

  let unlisten: (() => void) | undefined;

  onMount(async () => {
    await refreshStatus();
    unlisten = await listen<CollabUpdate>("collab://event", (event) => {
      const update = event.payload;
      switch (update.kind) {
        case "HostReady":
          status = { ...status, code: update.code };
          break;
        case "TextChanged":
          sharedText = update.new_text;
          break;
        case "PeerConnected":
          status = {
            ...status,
            peer_connected: true,
            reconnecting: false,
            peer_fingerprint: update.fingerprint,
          };
          break;
        case "Reconnecting":
          status = { ...status, peer_connected: false, reconnecting: true };
          break;
        case "PeerDisconnected":
          status = { ...status, peer_connected: false, reconnecting: false, session_ended: true };
          break;
        case "Error":
          error = update.message;
          break;
      }
    });
  });

  onDestroy(() => {
    unlisten?.();
  });

  async function host() {
    error = null;
    try {
      await invoke("collab_host");
      status = { ...status, active: true, role: "host" };
    } catch (err) {
      error = String(err);
    }
  }

  async function join() {
    error = null;
    try {
      await invoke("collab_join", { code: joinCode });
      status = { ...status, active: true, role: "joiner" };
    } catch (err) {
      error = String(err);
    }
  }

  async function end() {
    await invoke("collab_end");
    status = {
      active: false,
      role: null,
      code: null,
      peer_connected: false,
      reconnecting: false,
      peer_fingerprint: null,
      session_ended: false,
    };
    sharedText = "";
  }

  let editTimer: ReturnType<typeof setTimeout> | undefined;
  function onEdit() {
    clearTimeout(editTimer);
    editTimer = setTimeout(() => {
      invoke("collab_sync_local_edit", { text: sharedText });
    }, 300);
  }
</script>

<div class="collab">
  {#if !status.active}
    <div class="start">
      <button onclick={host}>Host a session</button>
      <div class="join">
        <input type="text" placeholder="Paste a connection code" bind:value={joinCode} />
        <button onclick={join} disabled={!joinCode}>Join</button>
      </div>
    </div>
  {:else}
    <div class="status-bar">
      <span>Role: {status.role}</span>
      {#if status.code}
        <span>Code: <code>{status.code}</code></span>
      {/if}
      {#if status.peer_connected}
        <span class="ok">Peer connected{status.peer_fingerprint ? ` (${status.peer_fingerprint})` : ""}</span>
      {:else if status.reconnecting}
        <span class="warn">Reconnecting…</span>
      {:else if status.session_ended}
        <span class="warn">Session ended</span>
      {:else}
        <span>Waiting for peer…</span>
      {/if}
      <button onclick={end}>End session</button>
    </div>
    <textarea bind:value={sharedText} oninput={onEdit} placeholder="Shared text — edits sync to your peer"></textarea>
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
</div>

<style>
  .collab {
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.5rem;
    box-sizing: border-box;
  }

  .start {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    align-items: flex-start;
  }

  .join {
    display: flex;
    gap: 0.5rem;
  }

  .status-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
    align-items: center;
    font-size: 0.85rem;
  }

  .ok {
    color: #2e7d32;
  }

  .warn {
    color: #b8860b;
  }

  textarea {
    flex: 1;
    font-family: inherit;
    padding: 0.5rem;
  }

  .error {
    color: #c0392b;
  }
</style>
