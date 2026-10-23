<script lang="ts">
  import { onMount } from "svelte";
  import {
    type StoryCard,
    emptyCard,
    listStoryCards,
    upsertStoryCard,
    deleteStoryCard,
  } from "./storyCards";

  // Phase 4 Corkboard: a wrapping grid of story cards, each a reduced "spine"
  // view (scene number / cause / effect / why it matters) of core's full
  // StoryCard schema — Lisa Cron's "Story Genius" cause-and-effect model. The
  // belief-timeline fields (prior/new belief, value shift, new knowledge),
  // subplot tags, and linked-document resolution are deliberately not
  // surfaced here yet; the goal of this phase is a real Svelte rebuild of
  // the grid/CRUD interaction, not full field parity with corkboard_panel.rs
  // on day one.
  let { root }: { root: string } = $props();

  let cards = $state<StoryCard[]>([]);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      cards = await listStoryCards(root);
    } catch (err) {
      error = String(err);
    }
  });

  async function addCard() {
    try {
      cards = await upsertStoryCard(root, emptyCard());
    } catch (err) {
      error = String(err);
    }
  }

  async function save(card: StoryCard) {
    try {
      cards = await upsertStoryCard(root, card);
    } catch (err) {
      error = String(err);
    }
  }

  async function remove(id: string) {
    try {
      cards = await deleteStoryCard(root, id);
    } catch (err) {
      error = String(err);
    }
  }
</script>

<section class="corkboard">
  <div class="toolbar">
    <button onclick={addCard}>+ New Card</button>
    {#if error}<span class="error">{error}</span>{/if}
  </div>
  <div class="grid">
    {#each cards as card (card.id)}
      <div class="card">
        <input
          class="scene-number"
          placeholder="Scene #"
          bind:value={card.scene_number}
          onblur={() => save(card)}
        />
        <label>
          Cause
          <textarea bind:value={card.cause} onblur={() => save(card)}></textarea>
        </label>
        <label>
          Effect
          <textarea bind:value={card.effect} onblur={() => save(card)}></textarea>
        </label>
        <label>
          Why it matters
          <textarea bind:value={card.why_it_matters} onblur={() => save(card)}></textarea>
        </label>
        <button class="delete" onclick={() => remove(card.id)}>Delete</button>
      </div>
    {/each}
  </div>
</section>

<style>
  .corkboard {
    width: 100%;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-bottom: 0.75rem;
  }

  .grid {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
  }

  .card {
    width: 14rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    padding: 0.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    background: #fffef8;
  }

  .scene-number {
    font-weight: bold;
  }

  label {
    display: flex;
    flex-direction: column;
    font-size: 0.75rem;
    color: #666;
    gap: 0.15rem;
  }

  textarea {
    resize: vertical;
    min-height: 2.5rem;
    font-family: inherit;
  }

  .delete {
    align-self: flex-end;
  }

  .error {
    color: #c0392b;
  }
</style>
