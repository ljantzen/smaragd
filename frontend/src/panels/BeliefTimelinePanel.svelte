<script lang="ts">
  import { onMount } from "svelte";
  import { listStoryCards, type StoryCard } from "../storyCards";
  import { project } from "../stores/project.svelte";

  // Phase 7: no new backend — this is the same story_cards_list data
  // Corkboard/Story Grid already fetch (Phase 4), grouped by pov_character
  // and shown as a prior_belief -> new_belief chain in card order. The real
  // belief_timeline_panel.rs lets the author pick which character's
  // timeline to view and cross-links to the card editor; here it's a flat
  // read-only list per character, grouped by whatever pov_character text
  // each card already has.
  let cards = $state<StoryCard[]>([]);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      cards = await listStoryCards(project.value!.root);
    } catch (err) {
      error = String(err);
    }
  });

  let byCharacter = $derived.by(() => {
    const groups = new Map<string, StoryCard[]>();
    for (const card of cards) {
      const key = card.pov_character.trim() || "(unassigned)";
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key)!.push(card);
    }
    return groups;
  });
</script>

<div class="panel">
  {#if error}<p class="error">{error}</p>{/if}
  {#if cards.length === 0}
    <p class="empty">No story cards yet — add some in Corkboard.</p>
  {:else}
    {#each [...byCharacter] as [character, group] (character)}
      <section>
        <h3>{character}</h3>
        <ol>
          {#each group as card (card.id)}
            <li>
              <span class="scene">{card.scene_number || "(no scene #)"}</span>
              <span class="belief">{card.prior_belief || "—"} → {card.new_belief || "—"}</span>
            </li>
          {/each}
        </ol>
      </section>
    {/each}
  {/if}
</div>

<style>
  .panel {
    height: 100%;
    overflow: auto;
    padding: 0.5rem;
    box-sizing: border-box;
  }

  h3 {
    margin: 0.5rem 0 0.25rem;
    font-size: 0.9rem;
  }

  ol {
    margin: 0;
    padding-left: 1.2rem;
  }

  li {
    font-size: 0.85rem;
    margin-bottom: 0.2rem;
  }

  .scene {
    font-weight: bold;
    margin-right: 0.5rem;
  }

  .belief {
    color: #444;
  }

  .empty {
    color: #888;
  }

  .error {
    color: #c0392b;
  }
</style>
