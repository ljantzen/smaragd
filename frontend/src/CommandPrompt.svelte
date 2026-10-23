<script lang="ts">
  import { SHORTCUT_ACTIONS, effectiveCombo, runShortcutAction } from "./shortcuts";

  // The real app's Command Prompt is a `:`-prefixed colon-command bar
  // (`:theme gruvbox`, `:git commit`, `:w`, ...) with its own parser. This
  // is a simplified substitute: a fuzzy filter over the same action
  // catalog shortcuts.ts already has, not a command-syntax interpreter —
  // same trade every other "do it for real but not byte-for-byte" item in
  // this pass makes.
  let { onClose }: { onClose: () => void } = $props();

  let query = $state("");
  let selected = $state(0);

  const results = $derived(
    SHORTCUT_ACTIONS.filter((a) => a.label.toLowerCase().includes(query.toLowerCase())).slice(
      0,
      20,
    ),
  );

  function run(actionId: string) {
    runShortcutAction(actionId);
    onClose();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      selected = Math.min(selected + 1, results.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
    } else if (e.key === "Enter" && results[selected]) {
      run(results[selected].id);
    }
  }

  function comboLabel(actionId: string): string {
    const action = SHORTCUT_ACTIONS.find((a) => a.id === actionId);
    if (!action) return "";
    const combo = effectiveCombo(action);
    if (!combo || combo.key === "") return "";
    const mod = combo.ctrl_or_cmd ? "Ctrl+" : "";
    const alt = combo.alt ? "Alt+" : "";
    const shift = combo.shift ? "Shift+" : "";
    return `${mod}${alt}${shift}${combo.key.toUpperCase()}`;
  }
</script>

<div class="backdrop" onclick={onClose} role="presentation">
  <div
    class="palette"
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <!-- svelte-ignore a11y_autofocus -->
    <input
      type="text"
      placeholder="Type a command…"
      bind:value={query}
      oninput={() => (selected = 0)}
      autofocus
    />
    <ul>
      {#each results as action, i (action.id)}
        <li>
          <button class:selected={i === selected} onclick={() => run(action.id)}>
            <span>{action.label}</span>
            <span class="shortcut">{comboLabel(action.id)}</span>
          </button>
        </li>
      {/each}
      {#if results.length === 0}
        <li class="empty">No matching command</li>
      {/if}
    </ul>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.2);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 15vh;
    z-index: 1000;
  }

  .palette {
    width: 32rem;
    max-width: 90vw;
    max-height: 60vh;
    background: white;
    border-radius: 6px;
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.3);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  input {
    padding: 0.6rem 0.8rem;
    border: none;
    border-bottom: 1px solid #ddd;
    font-size: 0.95rem;
  }

  input:focus {
    outline: none;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0.25rem 0;
    overflow: auto;
  }

  button {
    display: flex;
    justify-content: space-between;
    width: 100%;
    background: none;
    border: none;
    text-align: left;
    padding: 0.4rem 0.8rem;
    cursor: pointer;
    font-size: 0.85rem;
  }

  button.selected {
    background: #eef;
  }

  .shortcut {
    color: #888;
    font-size: 0.75rem;
  }

  .empty {
    padding: 0.5rem 0.8rem;
    color: #888;
    font-size: 0.85rem;
  }
</style>
