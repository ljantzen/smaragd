use super::*;

/// A single story card: a structured cause-and-effect schema (Cause, Effect, Why It
/// Matters, Realization, And So) from Lisa Cron's "Story Genius", extended with an
/// explicit belief-state transition (Prior Belief, New Belief, Value Shift, New
/// Knowledge) — the psychological-change unit a scene's events serve, not a freeform
/// synopsis. Optionally soft-linked to one or more documents by title (see
/// `linked_document_stems`), the same way `[[wikilinks]]` resolve — never by path or
/// by the document's `BinderNode::id` (which is regenerated on every rescan and so
/// isn't a durable reference).
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
    /// Whose belief/knowledge this card tracks. Free text, like `linked_document_stems`
    /// — not tied to `ProjectMeta::pov_picklist_folder`, since a card can describe a
    /// character's arc before any scene (and so any document POV) exists for it.
    /// `#[serde(default)]`: absent in story cards saved before this field existed.
    #[serde(default)]
    pub pov_character: String,
    /// What `pov_character` believes going into this card.
    #[serde(default)]
    pub prior_belief: String,
    /// What `pov_character` believes as a result of this card — together with
    /// `prior_belief`, the belief-state transition a Belief Timeline view chains
    /// across a character's cards in manuscript order.
    #[serde(default)]
    pub new_belief: String,
    /// The value at stake moving from one pole to the other, e.g. `"Trust ->
    /// Distrust"`. Free text for now, deliberately not a structured from/to pair —
    /// there's no usage yet to justify that structure.
    #[serde(default)]
    pub value_shift: String,
    /// Facts `pov_character` learns as of this card. Edited as a comma-separated
    /// list in the card editor, same convention as `subplot_tags`.
    #[serde(default)]
    pub new_knowledge: Vec<String>,
    /// The linked documents' filename stems (no path, no `.md`), resolved on demand
    /// via `BinderTree::find_document_by_stem` — many-to-many, since one card can span
    /// several scenes (or several cards can share one scene). An empty list means no
    /// scene has been drafted for this card yet. A stem that no longer resolves (its
    /// document was deleted) is a normal, passive state — the UI just shows "not
    /// found" — mirroring how a dangling `[[wikilink]]` already behaves elsewhere in
    /// the app.
    ///
    /// `#[serde(alias = "linked_document_stem")]` plus the custom deserializer below
    /// accept story cards saved before this was a list, where the JSON key was
    /// singular and held `null` or one string rather than an array.
    #[serde(
        default,
        alias = "linked_document_stem",
        deserialize_with = "deserialize_linked_document_stems"
    )]
    pub linked_document_stems: Vec<String>,
}

/// Accepts the pre-many-to-many shape (`null` or a single JSON string) in addition to
/// the current `Vec<String>`, so old `project.json` files migrate in place on load —
/// see `StoryCard::linked_document_stems`'s doc comment.
fn deserialize_linked_document_stems<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StemsShape {
        Single(String),
        Many(Vec<String>),
    }

    Ok(match Option::<StemsShape>::deserialize(deserializer)? {
        None => Vec::new(),
        Some(StemsShape::Single(stem)) => vec![stem],
        Some(StemsShape::Many(stems)) => stems,
    })
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
            pov_character: String::new(),
            prior_belief: String::new(),
            new_belief: String::new(),
            value_shift: String::new(),
            new_knowledge: Vec::new(),
            linked_document_stems: Vec::new(),
        }
    }
}

impl Default for StoryCard {
    fn default() -> Self {
        Self::new()
    }
}

impl Project {
    /// The filename stems of every document currently eligible to be linked from a
    /// story card — the candidate list for the card editor's Linked Documents
    /// autocomplete. Restricted to documents under a `FolderRole::Manuscript`
    /// folder (falling back to every document except Trash/Templates content if the
    /// project has no Manuscript folder yet, the same fallback
    /// `WordCountScope::ManuscriptOnly` uses): a story card tracks a psychological
    /// change in the manuscript, not in research notes or character bios, so those
    /// shouldn't clutter the suggestion list. This only narrows what's *suggested* —
    /// an already-linked stem that now falls outside Manuscript (e.g. its folder's
    /// role was removed) still resolves as before, the same tolerant-of-drift
    /// soft-link behavior `linked_document_stems`'s own doc comment describes.
    pub fn manuscript_document_stems(&self) -> Vec<String> {
        self.tree
            .document_paths()
            .into_iter()
            .filter(|path| self.is_path_tracked(path, WordCountScope::ManuscriptOnly))
            .filter_map(|path| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            })
            .collect()
    }

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
    fn manuscript_document_stems_falls_back_to_everything_except_trash_and_templates_when_unassigned()
     {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        project.create_document(&trash, "Old Scene").unwrap();
        project.create_document(dir.path(), "Scene 1").unwrap();

        assert_eq!(project.manuscript_document_stems(), vec!["Scene 1"]);
    }

    #[test]
    fn manuscript_document_stems_is_restricted_to_manuscript_folders_once_one_is_assigned() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let manuscript = project.create_folder(dir.path(), "Book").unwrap();
        project
            .set_folder_role(&manuscript, Some(FolderRole::Manuscript))
            .unwrap();
        project.create_document(&manuscript, "Scene 1").unwrap();
        project
            .create_document(dir.path(), "Character Bio")
            .unwrap();

        assert_eq!(project.manuscript_document_stems(), vec!["Scene 1"]);
    }

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
        card.linked_document_stems = vec!["Scene 1".to_string()];
        let id = card.id;
        project.upsert_story_card(card).unwrap();

        project.delete(&doc).unwrap();

        // The card survives untouched; only resolution against the (now-gone) tree
        // fails, mirroring how a dangling [[wikilink]] behaves elsewhere.
        let stems = project
            .story_card(id)
            .unwrap()
            .linked_document_stems
            .clone();
        assert_eq!(stems, vec!["Scene 1".to_string()]);
        assert!(project.tree.find_document_by_stem("Scene 1").is_none());
    }

    #[test]
    fn story_card_json_with_a_single_old_shape_linked_document_stem_migrates_to_a_list() {
        // Guards the `#[serde(alias, deserialize_with)]` pair on
        // `StoryCard::linked_document_stems`: a project.json written before it became
        // a list has a singular `linked_document_stem` key holding one string.
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
                    "linked_document_stem": "Scene 1"
                }]
            }"#,
        )
        .unwrap();

        let project = Project::load_from_folder(dir.path()).unwrap();

        assert_eq!(
            project.meta.story_cards[0].linked_document_stems,
            vec!["Scene 1".to_string()]
        );
    }

    #[test]
    fn story_card_json_with_a_null_old_shape_linked_document_stem_becomes_an_empty_list() {
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

        assert!(project.meta.story_cards[0].linked_document_stems.is_empty());
    }

    #[test]
    fn story_card_json_without_belief_fields_loads_with_them_blank() {
        // Guards `#[serde(default)]` on the belief/knowledge fields added alongside
        // the many-to-many link change, same rationale as the `why_it_matters` test
        // above.
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
                    "linked_document_stems": []
                }]
            }"#,
        )
        .unwrap();

        let project = Project::load_from_folder(dir.path()).unwrap();

        let card = &project.meta.story_cards[0];
        assert_eq!(card.pov_character, "");
        assert_eq!(card.prior_belief, "");
        assert_eq!(card.new_belief, "");
        assert_eq!(card.value_shift, "");
        assert!(card.new_knowledge.is_empty());
    }
}
