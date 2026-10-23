import { invoke } from "@tauri-apps/api/core";

// Mirrors core's StoryCard (core/src/project/story_cards.rs) field-for-field —
// it's already Serialize/Deserialize there, so no separate DTO was needed,
// just this hand-written TS shape. Phase 4 exposes the full schema over IPC,
// but Corkboard/StoryGrid only edit a reduced "spine" subset of it (see
// their own doc comments) — later refinement, not a backend gap.
export type StoryCard = {
  id: string;
  scene_number: string;
  alpha_point: string;
  subplot_tags: string[];
  cause: string;
  effect: string;
  why_it_matters: string;
  realization: string;
  and_so: string;
  pov_character: string;
  prior_belief: string;
  new_belief: string;
  value_shift: string;
  new_knowledge: string[];
  linked_document_stems: string[];
};

export function emptyCard(): StoryCard {
  return {
    id: crypto.randomUUID(),
    scene_number: "",
    alpha_point: "",
    subplot_tags: [],
    cause: "",
    effect: "",
    why_it_matters: "",
    realization: "",
    and_so: "",
    pov_character: "",
    prior_belief: "",
    new_belief: "",
    value_shift: "",
    new_knowledge: [],
    linked_document_stems: [],
  };
}

export function listStoryCards(root: string): Promise<StoryCard[]> {
  return invoke<StoryCard[]>("story_cards_list", { root });
}

export function upsertStoryCard(root: string, card: StoryCard): Promise<StoryCard[]> {
  return invoke<StoryCard[]>("story_card_upsert", { root, card });
}

export function deleteStoryCard(root: string, id: string): Promise<StoryCard[]> {
  return invoke<StoryCard[]>("story_card_delete", { root, id });
}

export function moveStoryCard(
  root: string,
  id: string,
  newIndex: number,
): Promise<StoryCard[]> {
  return invoke<StoryCard[]>("story_card_move", { root, id, newIndex });
}
