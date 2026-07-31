use super::*;

impl Project {
    /// Set the protagonist's Desire — see `ProjectMeta::protagonist_desire`.
    pub fn set_protagonist_desire(&mut self, desire: String) -> io::Result<()> {
        self.meta.protagonist_desire = desire;
        self.save_metadata()
    }

    /// Set the protagonist's Misbelief — see `ProjectMeta::protagonist_misbelief`.
    pub fn set_protagonist_misbelief(&mut self, misbelief: String) -> io::Result<()> {
        self.meta.protagonist_misbelief = misbelief;
        self.save_metadata()
    }

    /// The story card with `id`, if it still exists.
    pub fn story_card(&self, id: Uuid) -> Option<&StoryCard> {
        self.meta.story_cards.iter().find(|card| card.id == id)
    }

    /// Insert `card` if its id isn't already on the board, or replace the existing
    /// card with the same id otherwise — persisted either way. Used for both creating
    /// and editing a card from the same "Save" action in the card editor.
    pub fn upsert_story_card(&mut self, card: StoryCard) -> io::Result<()> {
        match self.meta.story_cards.iter_mut().find(|c| c.id == card.id) {
            Some(existing) => *existing = card,
            None => self.meta.story_cards.push(card),
        }
        self.save_metadata()
    }

    pub fn delete_story_card(&mut self, id: Uuid) -> io::Result<()> {
        self.meta.story_cards.retain(|card| card.id != id);
        self.save_metadata()
    }

    /// Move the card `id` to `new_index` in board order (clamped to the number of
    /// cards remaining after it's removed), and persist. A no-op if `id` isn't found.
    pub fn move_story_card(&mut self, id: Uuid, new_index: usize) -> io::Result<()> {
        let Some(current_index) = self.meta.story_cards.iter().position(|c| c.id == id) else {
            return Ok(());
        };
        let card = self.meta.story_cards.remove(current_index);
        let new_index = new_index.min(self.meta.story_cards.len());
        self.meta.story_cards.insert(new_index, card);
        self.save_metadata()
    }
}
