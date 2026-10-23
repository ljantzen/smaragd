<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { project } from "./stores/project.svelte";
  import { showToast } from "./stores/toast.svelte";

  // Commit (and optionally Push right after) — message pre-filled from
  // Settings::git_commit_message_template, same {{date}}/{{numFiles}}/
  // {{fileList}}/etc. placeholders the real app's commit prompt fills in,
  // editable before confirming.
  let { onClose, andPush = false }: { onClose: () => void; andPush?: boolean } = $props();

  let message = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);

  onMount(async () => {
    try {
      message = await invoke<string>("git_commit_message_preview", { root: project.value!.root });
    } catch (err) {
      error = String(err);
    }
  });

  async function confirm() {
    busy = true;
    error = null;
    try {
      await invoke("git_commit", { root: project.value!.root, message });
      showToast("Committed");
      if (andPush) {
        const result = await invoke<{ files: string[] }>("git_push", { root: project.value!.root });
        showToast(`Pushed (${result.files.length} file(s))`);
      }
      onClose();
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && !busy && onClose()} />

<div
  class="backdrop"
  onclick={() => !busy && onClose()}
  onkeydown={(e) => e.key === "Escape" && !busy && onClose()}
  role="presentation"
>
  <div
    class="modal"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <h2>{andPush ? "Commit and Push" : "Commit"}</h2>
    <textarea bind:value={message} rows="6"></textarea>
    {#if error}<p class="error">{error}</p>{/if}
    <div class="footer">
      <button onclick={onClose} disabled={busy}>Cancel</button>
      <button onclick={confirm} disabled={busy}>{busy ? "Working…" : "Commit"}</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.3);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal {
    width: 28rem;
    background: white;
    border-radius: 6px;
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.3);
    padding: 1.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  h2 {
    margin: 0;
    font-size: 1.1rem;
  }

  textarea {
    font-family: ui-monospace, monospace;
    font-size: 0.85rem;
    padding: 0.5rem;
  }

  .footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }

  .error {
    color: #c0392b;
    font-size: 0.8rem;
  }
</style>
