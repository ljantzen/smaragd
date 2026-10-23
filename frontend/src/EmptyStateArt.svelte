<script lang="ts">
  // The "Open a project" empty state's illustration — a provided pen-and-
  // ink style drawing of an emerald-cut gem and a quill (with inkwell and
  // scattered feathers). The illustration *is* the call to action now: no
  // separate heading/Browse button sits below it (removed — redundant
  // once clicking the picture itself opens the folder picker), so this is
  // a real <button> around the <img>, not just a styled div, for
  // accessibility (keyboard-operable, has a real control's semantics)
  // without visible button chrome.
  import backgroundUrl from "./assets/empty-state-background.png";

  let { onClick }: { onClick: () => void } = $props();
</script>

<button class="empty-state-art" onclick={onClick} aria-label="Open a project">
  <img src={backgroundUrl} alt="" />
</button>

<style>
  .empty-state-art {
    display: block;
    width: min(95vw, 60rem);
    padding: 0;
    border: none;
    background: none;
    cursor: pointer;
    border-radius: 4px;
  }

  .empty-state-art img {
    display: block;
    width: 100%;
    height: auto;
    border-radius: 4px;
  }

  .empty-state-art:hover img {
    filter: brightness(1.03);
  }

  .empty-state-art:focus-visible {
    outline: 2px solid #557;
    outline-offset: 4px;
  }
</style>
