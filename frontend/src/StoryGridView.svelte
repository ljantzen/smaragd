<script lang="ts">
  import { onMount } from "svelte";
  import {
    type StoryCard,
    listStoryCards,
    upsertStoryCard,
    moveStoryCard,
  } from "./storyCards";
  import { project } from "./stores/project.svelte";

  // Phase 4 Story Grid: the same story-card list Corkboard edits, as an
  // ordered table instead of a wrapping card grid, with drag-to-reorder
  // (backed by core's move_story_card, same card list + order core already
  // persists). The real story_grid_panel.rs additionally supports order
  // modes (manuscript order vs. card order) and a Belief Timeline view —
  // not ported here; this phase's job was proving a second, structurally
  // different native view over the same IPC-backed data, not full parity.
  //
  // Phase 5: reads `root` from the shared project store rather than a
  // prop — mounted standalone inside a dockview panel now.
  const root = $derived(project.value!.root);

  let cards = $state<StoryCard[]>([]);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      cards = await listStoryCards(root);
    } catch (err) {
      error = String(err);
    }
  });

  async function save(card: StoryCard) {
    try {
      cards = await upsertStoryCard(root, card);
    } catch (err) {
      error = String(err);
    }
  }

  // Phase 9: up/down buttons instead of drag-to-reorder — HTML5 DnD
  // doesn't fire on touch input at all (see BinderTree.svelte's moveTo for
  // the same issue there), so a touch-only mobile session had no way to
  // reorder cards before this. Replacing the drag handle outright rather
  // than keeping both: two different reorder mechanisms side by side is
  // more to maintain for no real benefit once one of them works everywhere.
  async function move(index: number, direction: -1 | 1) {
    const target = index + direction;
    if (target < 0 || target >= cards.length) return;
    try {
      cards = await moveStoryCard(root, cards[index].id, target);
    } catch (err) {
      error = String(err);
    }
  }
</script>

<section class="story-grid">
  {#if error}<p class="error">{error}</p>{/if}
  <table>
    <thead>
      <tr>
        <th></th>
        <th>Scene #</th>
        <th>Cause</th>
        <th>Effect</th>
        <th>Why it matters</th>
      </tr>
    </thead>
    <tbody>
      {#each cards as card, index (card.id)}
        <tr>
          <td class="handle">
            <button disabled={index === 0} onclick={() => move(index, -1)}>▲</button>
            <button disabled={index === cards.length - 1} onclick={() => move(index, 1)}>▼</button>
          </td>
          <td><input bind:value={card.scene_number} onblur={() => save(card)} /></td>
          <td><input bind:value={card.cause} onblur={() => save(card)} /></td>
          <td><input bind:value={card.effect} onblur={() => save(card)} /></td>
          <td><input bind:value={card.why_it_matters} onblur={() => save(card)} /></td>
        </tr>
      {/each}
    </tbody>
  </table>
</section>

<style>
  .story-grid {
    width: 100%;
    overflow: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
  }

  th,
  td {
    border: 1px solid #ccc;
    padding: 0.3rem 0.5rem;
    text-align: left;
  }

  td input {
    width: 100%;
    border: none;
    background: transparent;
  }

  .handle {
    display: flex;
    gap: 0.15rem;
    width: 3rem;
  }

  .handle button {
    flex: 1;
    padding: 0.1rem;
    font-size: 0.7rem;
  }

  .error {
    color: #c0392b;
  }
</style>
