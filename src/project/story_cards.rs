use super::*;

/// A single Lisa Cron "Story Genius" scene card: a structured cause-and-effect
/// schema (Cause, Effect, Why It Matters, Realization, And So), not a freeform
/// synopsis. Optionally soft-linked to a
/// document by title (see `linked_document_stem`), the same way `[[wikilinks]]`
/// resolve — never by path or by the document's `BinderNode::id` (which is
/// regenerated on every rescan and so isn't a durable reference).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoryCard {
    pub id: Uuid,
    pub scene_number: String,
    pub alpha_point: String,
    pub subplot_tags: Vec<String>,
    /// External event that occurs.
    pub cause: String,
    /// External and internal consequence of the cause.
    pub effect: String,
    /// Why these events matter to the protagonist personally — the scene's link to
    /// their internal struggle, per Lisa Cron's "Third Rail" concept (see
    /// `ProjectMeta::protagonist_desire`/`protagonist_misbelief`). `#[serde(default)]`
    /// since story cards saved before this field existed have no `why_it_matters`
    /// key at all.
    #[serde(default)]
    pub why_it_matters: String,
    pub realization: String,
    /// What the protagonist does next, as a result of `realization`.
    pub and_so: String,
    /// The linked document's filename stem (no path, no `.md`), resolved on demand via
    /// `BinderTree::find_document_by_stem`. `None` means no scene has been drafted for
    /// this card yet. A stem that no longer resolves (its document was deleted) is a
    /// normal, passive state — the UI just shows "not found" — mirroring how a
    /// dangling `[[wikilink]]` already behaves elsewhere in the app.
    pub linked_document_stem: Option<String>,
}

impl StoryCard {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            scene_number: String::new(),
            alpha_point: String::new(),
            subplot_tags: Vec::new(),
            cause: String::new(),
            effect: String::new(),
            why_it_matters: String::new(),
            realization: String::new(),
            and_so: String::new(),
            linked_document_stem: None,
        }
    }
}

impl Default for StoryCard {
    fn default() -> Self {
        Self::new()
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_protagonist_desire_and_misbelief_persist_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        assert_eq!(project.meta.protagonist_desire, "");
        assert_eq!(project.meta.protagonist_misbelief, "");

        project
            .set_protagonist_desire("Wants to reclaim the family farm".to_string())
            .unwrap();
        project
            .set_protagonist_misbelief("Believes she doesn't deserve a home".to_string())
            .unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(
            reloaded.meta.protagonist_desire,
            "Wants to reclaim the family farm"
        );
        assert_eq!(
            reloaded.meta.protagonist_misbelief,
            "Believes she doesn't deserve a home"
        );
    }

    #[test]
    fn story_card_json_without_why_it_matters_loads_with_it_blank() {
        // Guards `#[serde(default)]` on `StoryCard::why_it_matters`: a project.json
        // written before this field existed has no `why_it_matters` key at all in
        // its story card entries.
        let dir = tempfile::tempdir().unwrap();
        let meta_dir = dir.path().join(METADATA_DIR);
        fs::create_dir_all(&meta_dir).unwrap();
        fs::write(
            meta_dir.join(METADATA_FILE),
            r#"{
                "version": 1,
                "node_order": {},
                "story_cards": [{
                    "id": "3f9e2b1a-0c1d-4a8e-9b2a-2a6f8f7d9c11",
                    "scene_number": "1",
                    "alpha_point": "",
                    "subplot_tags": [],
                    "cause": "",
                    "effect": "",
                    "realization": "",
                    "and_so": "",
                    "linked_document_stem": null
                }]
            }"#,
        )
        .unwrap();

        let project = Project::load_from_folder(dir.path()).unwrap();

        assert_eq!(project.meta.story_cards.len(), 1);
        assert_eq!(project.meta.story_cards[0].why_it_matters, "");
    }

    #[test]
    fn upsert_story_card_inserts_a_new_card() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let card = StoryCard::new();
        let id = card.id;

        project.upsert_story_card(card).unwrap();

        assert!(project.story_card(id).is_some());
        assert_eq!(project.meta.story_cards.len(), 1);
    }

    #[test]
    fn upsert_story_card_replaces_an_existing_card_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let mut card = StoryCard::new();
        let id = card.id;
        project.upsert_story_card(card.clone()).unwrap();

        card.scene_number = "3".to_string();
        project.upsert_story_card(card).unwrap();

        assert_eq!(project.meta.story_cards.len(), 1);
        assert_eq!(project.story_card(id).unwrap().scene_number, "3");
    }

    #[test]
    fn upsert_story_card_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let mut card = StoryCard::new();
        card.alpha_point = "Inciting incident".to_string();
        let id = card.id;
        project.upsert_story_card(card).unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();

        assert_eq!(
            reloaded.story_card(id).unwrap().alpha_point,
            "Inciting incident"
        );
    }

    #[test]
    fn delete_story_card_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let card = StoryCard::new();
        let id = card.id;
        project.upsert_story_card(card).unwrap();

        project.delete_story_card(id).unwrap();

        assert!(project.story_card(id).is_none());
    }

    #[test]
    fn move_story_card_reorders_the_board() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let a = StoryCard::new();
        let b = StoryCard::new();
        let c = StoryCard::new();
        let (a_id, b_id, c_id) = (a.id, b.id, c.id);
        project.upsert_story_card(a).unwrap();
        project.upsert_story_card(b).unwrap();
        project.upsert_story_card(c).unwrap();

        // Move the last card (c) to the front.
        project.move_story_card(c_id, 0).unwrap();

        let order: Vec<Uuid> = project.meta.story_cards.iter().map(|c| c.id).collect();
        assert_eq!(order, vec![c_id, a_id, b_id]);
    }

    #[test]
    fn move_story_card_is_a_no_op_for_an_unknown_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.upsert_story_card(StoryCard::new()).unwrap();

        let result = project.move_story_card(Uuid::new_v4(), 0);

        assert!(result.is_ok());
        assert_eq!(project.meta.story_cards.len(), 1);
    }

    #[test]
    fn deleting_the_linked_document_leaves_a_dangling_but_harmless_link() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        let mut card = StoryCard::new();
        card.linked_document_stem = Some("Scene 1".to_string());
        let id = card.id;
        project.upsert_story_card(card).unwrap();

        project.delete(&doc).unwrap();

        // The card survives untouched; only resolution against the (now-gone) tree
        // fails, mirroring how a dangling [[wikilink]] behaves elsewhere.
        let stem = project.story_card(id).unwrap().linked_document_stem.clone();
        assert_eq!(stem.as_deref(), Some("Scene 1"));
        assert!(project.tree.find_document_by_stem("Scene 1").is_none());
    }
}
