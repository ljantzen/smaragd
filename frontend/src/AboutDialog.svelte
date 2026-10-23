<script lang="ts">
  let { onClose }: { onClose: () => void } = $props();
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onClose()} />

<div
  class="backdrop"
  onclick={onClose}
  onkeydown={(e) => e.key === "Escape" && onClose()}
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
    <h2>Smaragd</h2>
    <p>A novel-writing and manuscript editor.</p>
    <p class="note">
      Tauri desktop + mobile shell (in progress) — the egui-based
      io.github.ljantzen.smaragd release remains current and stable.
    </p>
    <div class="footer">
      <button onclick={onClose}>OK</button>
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
    width: 320px;
    background: white;
    border-radius: 6px;
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.3);
    padding: 1.5rem;
    text-align: center;
  }

  h2 {
    margin: 0 0 0.5rem;
  }

  .note {
    font-size: 0.8rem;
    color: #888;
  }

  .footer {
    margin-top: 1rem;
  }
</style>
