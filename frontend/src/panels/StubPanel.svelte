<script lang="ts">
  // Phase 5's job is dock-layout integration (split/float/persist/reopen),
  // not full content parity for every tab in the real app's DockTab enum
  // (app/src/app/dock.rs) — that's Phase 7. These tabs are still real,
  // fully dockable dockview panels today (open from the Window menu, split,
  // float, closed/reopened, persisted across reload) with a placeholder
  // body standing in for content that doesn't exist yet.
  let { title }: { title: string } = $props();
</script>

<div class="stub">
  <p>{title}</p>
  <p class="note">Content coming in Phase 7.</p>
</div>

<style>
  .stub {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.25rem;
    color: #888;
    box-sizing: border-box;
  }

  .note {
    font-size: 0.8rem;
  }
</style>
