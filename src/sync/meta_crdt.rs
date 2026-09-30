//! `project.json` (`ProjectMeta`) as a CRDT.
//!
//! A flat text CRDT over the serialized JSON would merge at the *character* level
//! of the encoding — exactly the garbage merge automatic resolution is meant to
//! avoid — so the structure is mapped onto CRDT types instead, in two steps:
//!
//! 1. **[`SyncedFields`]** — a plain, id-keyed snapshot of the fields that sync.
//!    [`SyncedFields::from_meta`] / [`SyncedFields::into_meta`] convert to and from
//!    a `ProjectMeta`, and are where the two hard problems are solved:
//!    - *Per-device state never syncs.* Git/plugin toggles, the session and streak
//!      counters and the schema version stay exactly as they are locally
//!      (`into_meta` only overwrites the synced fields). In particular
//!      `plugins_enabled` must stay local: its doc comment demands explicit local
//!      consent, because plugin content can arrive from elsewhere.
//!    - *Paths become ids.* `node_order`, `folder_roles`, `folder_meta`,
//!      `trashed_origins`, bookmarks and the picklist folders refer to files and
//!      folders by path. Inside the CRDT they use the manifest's stable ids
//!      ([`PathIds`]), translated back to the *current* paths on the way out, so a
//!      rename on one device can't orphan an edit made under the old name on another.
//!      Story cards link documents by file name rather than path; each link travels as
//!      the name *and* the id of the one document with that name (see [`CardLink`]),
//!      so a card edited under an old name still points at the renamed document.
//! 2. **[`MetaDoc`]** — the CRDT. Settings are per-key registers, prose fields are
//!    text (character-level merge), path-keyed maps are nested maps keyed by id, and
//!    ordered lists (a folder's children, story cards, bookmarks) are arrays whose
//!    elements are maps, so concurrent inserts, deletes and reorders merge and edits
//!    to different fields of the same card both survive.
//!
//! Every `ProjectMeta` field is classified exactly once — see
//! [`classify_exhaustively`], which fails to compile when a field is added until it
//! is deliberately placed in one of the lists here.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use smaragd_sync_protocol::DocId;
use yrs::error::Error;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Array, ArrayPrelim, ArrayRef, Doc, GetString, Map as _, MapPrelim, MapRef, Out, ReadTxn,
    StateVector, Text, TextRef, Transact, TransactionMut, Update,
};

use super::manifest::ManifestEntry;
use crate::collab::diff;
use crate::project::ProjectMeta;

/// Simple settings, one last-writer-wins register each (the JSON of the value).
const SCALAR_FIELDS: &[&str] = &[
    "binder_color_mode",
    "book_title",
    "book_subtitle",
    "book_author",
    "book_style",
    "draft_target_words",
    "session_target_words",
    "word_count_scope",
    "last_spell_check_language",
    "sync_files",
];
/// `Option<String>` fields holding a folder's path key; synced as that folder's id.
const FOLDER_REF_FIELDS: &[&str] = &[
    "type_picklist_folder",
    "pov_picklist_folder",
    "status_picklist_folder",
];
/// Prose fields, merged as text.
const TEXT_FIELDS: &[&str] = &[
    "logline",
    "point",
    "synopsis",
    "what_if",
    "protagonist_desire",
    "protagonist_misbelief",
];
/// Fields that never leave the device they're set on.
pub const LOCAL_ONLY_FIELDS: &[&str] = &[
    "version",
    "git_enabled",
    "git_prompted",
    "plugins_enabled",
    "session_baseline_words",
    "session_baseline_date",
    "daily_word_counts",
    "streak_enabled",
    "streak_schedule",
    "streak_evaluation_mode",
    "streak_red_threshold_weeks",
    "session_log",
    "current_session_started",
    "current_session_baseline_words",
    "document_created",
];
/// Fields with structure of their own, handled individually below.
#[cfg_attr(not(test), allow(dead_code))]
const STRUCTURED_FIELDS: &[&str] = &[
    "node_order",
    "folder_roles",
    "trashed_origins",
    "folder_meta",
    "status_colors",
    "pov_colors",
    "story_cards",
    "bookmarks",
    "notes",
];

/// Compile-time guarantee that every `ProjectMeta` field has been classified: adding
/// a field breaks this pattern until the new field is named here *and* put in one of
/// the lists above (a unit test checks the lists against the real field names).
#[allow(dead_code)]
fn classify_exhaustively(meta: &ProjectMeta) {
    let ProjectMeta {
        version: _,
        node_order: _,
        folder_roles: _,
        trashed_origins: _,
        folder_meta: _,
        status_colors: _,
        binder_color_mode: _,
        pov_colors: _,
        story_cards: _,
        protagonist_desire: _,
        protagonist_misbelief: _,
        git_enabled: _,
        git_prompted: _,
        plugins_enabled: _,
        book_title: _,
        book_subtitle: _,
        book_author: _,
        book_style: _,
        type_picklist_folder: _,
        pov_picklist_folder: _,
        status_picklist_folder: _,
        draft_target_words: _,
        session_target_words: _,
        word_count_scope: _,
        session_baseline_words: _,
        session_baseline_date: _,
        daily_word_counts: _,
        streak_enabled: _,
        streak_schedule: _,
        streak_evaluation_mode: _,
        streak_red_threshold_weeks: _,
        logline: _,
        point: _,
        synopsis: _,
        what_if: _,
        bookmarks: _,
        notes: _,
        last_spell_check_language: _,
        sync_files: _,
        session_log: _,
        current_session_started: _,
        current_session_baseline_words: _,
        document_created: _,
    } = meta;
}

/// The manifest's paths and ids, both ways, for the live files and folders. The
/// project root (`""`) has the reserved id [`DocId::PROJECT_ROOT`].
#[derive(Debug, Clone, Default)]
pub struct PathIds {
    by_path: BTreeMap<String, DocId>,
    by_id: BTreeMap<DocId, String>,
    /// The layout as of the last completed metadata pass. A local `project.json` that
    /// was last written before another device renamed something still uses the *old*
    /// paths; resolving those through this map (as a fallback, path -> id only) is what
    /// lets an edit made under an old name follow the item to its new one instead of
    /// being dropped as an unknown path — which would also delete the item's metadata
    /// for every device.
    previous: BTreeMap<String, DocId>,
}

impl PathIds {
    pub fn from_entries(entries: &[ManifestEntry]) -> Self {
        let mut ids = PathIds::default();
        ids.by_path.insert(String::new(), DocId::PROJECT_ROOT);
        ids.by_id.insert(DocId::PROJECT_ROOT, String::new());
        // Smallest id wins a (transient) duplicate path, matching the engine's rule.
        // Reserved ids belong to the singleton documents; an entry claiming one is ignored.
        let mut live: Vec<&ManifestEntry> = entries
            .iter()
            .filter(|e| !e.deleted && !e.doc_id.is_reserved())
            .collect();
        live.sort_by_key(|e| e.doc_id);
        for entry in live {
            ids.by_path
                .entry(entry.path.clone())
                .or_insert(entry.doc_id);
            ids.by_id.insert(entry.doc_id, entry.path.clone());
        }
        ids
    }

    /// Adds the fallback layout described on the `previous` field.
    pub fn with_previous(mut self, previous: BTreeMap<String, DocId>) -> Self {
        self.previous = previous;
        self
    }

    /// This layout's path -> id map, to be handed to [`Self::with_previous`] next time.
    pub fn snapshot(&self) -> BTreeMap<String, DocId> {
        self.by_path.clone()
    }

    fn id(&self, path: &str) -> Option<DocId> {
        self.by_path.get(path).copied().or_else(|| {
            self.previous
                .get(path)
                .copied()
                .filter(|id| self.by_id.contains_key(id))
        })
    }

    fn path(&self, id: DocId) -> Option<&str> {
        self.by_id.get(&id).map(String::as_str)
    }

    /// The document a story-card link names — a file name without `.md`, matched
    /// case-insensitively like `BinderTree::find_document_by_stem` — but only if exactly
    /// one document has that name: an ambiguous link stays a plain name, resolved by
    /// the app as before. A name no longer in use falls back to the previous layout
    /// (see `previous`), so a link written before a rename arrived still resolves.
    fn doc_by_stem(&self, stem: &str) -> Option<DocId> {
        let stem = stem.to_lowercase();
        let matching = |paths: &BTreeMap<String, DocId>| -> BTreeSet<DocId> {
            paths
                .iter()
                .filter(|(path, _)| file_stem(path).is_some_and(|s| s.to_lowercase() == stem))
                .map(|(_, id)| *id)
                .filter(|id| self.by_id.contains_key(id))
                .collect()
        };
        let mut found = matching(&self.by_path);
        if found.is_empty() {
            found = matching(&self.previous);
        }
        match found.len() {
            1 => found.pop_first(),
            _ => None,
        }
    }

    /// The current file name (without `.md`) of document `id`.
    fn stem(&self, id: DocId) -> Option<&str> {
        self.path(id).and_then(file_stem)
    }
}

/// `"Draft/Chapter One.md"` -> `"Chapter One"`; `None` for anything but a document.
fn file_stem(path: &str) -> Option<&str> {
    path.rsplit('/').next()?.strip_suffix(".md")
}

/// One of a story card's linked documents as it travels in the CRDT: the file name
/// `project.json` stores, plus the id of the document it named when last written
/// (empty if the name was ambiguous or unknown). Kept as one value per link, in one
/// field, so a concurrent edit can never pair one device's names with another's ids.
#[derive(Debug, Serialize, Deserialize)]
struct CardLink {
    stem: String,
    #[serde(default)]
    target: String,
}

/// One element of an ordered list: string fields, always including `"id"`.
pub type Item = BTreeMap<String, String>;

/// The synced part of a `ProjectMeta`, keyed by stable ids rather than paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncedFields {
    /// Field name -> JSON text of the value.
    pub scalars: BTreeMap<String, String>,
    /// Field name -> text; empty fields are omitted.
    pub texts: BTreeMap<String, String>,
    pub status_colors: BTreeMap<String, String>,
    pub pov_colors: BTreeMap<String, String>,
    /// Folder id -> its children's ids, in order.
    pub order: BTreeMap<DocId, Vec<DocId>>,
    /// Folder id -> JSON text of its role.
    pub roles: BTreeMap<DocId, String>,
    /// Folder id -> its metadata fields (JSON text each).
    pub folder_meta: BTreeMap<DocId, BTreeMap<String, String>>,
    /// Trashed item id -> the path key it was trashed from.
    pub trashed: BTreeMap<DocId, String>,
    pub story_cards: Vec<Item>,
    pub bookmarks: Vec<Item>,
    pub notes: Vec<Item>,
}

fn json_text(value: &Value) -> String {
    value.to_string()
}

fn child_key(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_string()
    } else {
        format!("{folder}/{name}")
    }
}

fn split_key(key: &str) -> (&str, &str) {
    key.rsplit_once('/').unwrap_or(("", key))
}

fn object_fields(value: &Value) -> Option<BTreeMap<String, String>> {
    Some(
        value
            .as_object()?
            .iter()
            .map(|(k, v)| (k.clone(), json_text(v)))
            .collect(),
    )
}

/// Elements of an array of objects, each with a string `id`, as [`Item`]s whose other
/// fields are JSON text. Elements without a usable id are dropped.
fn items_of(value: Option<&Value>) -> Vec<Item> {
    let mut seen = BTreeSet::new();
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|element| {
            let object = element.as_object()?;
            let id = object.get("id")?.as_str()?.to_string();
            if !seen.insert(id.clone()) {
                return None;
            }
            let mut item: Item = object
                .iter()
                .filter(|(k, _)| *k != "id")
                .map(|(k, v)| (k.clone(), json_text(v)))
                .collect();
            item.insert("id".into(), id);
            Some(item)
        })
        .collect()
}

fn item_to_object(item: &Item) -> Option<Map<String, Value>> {
    let mut object = Map::new();
    for (key, text) in item {
        if key == "id" {
            object.insert(key.clone(), Value::String(text.clone()));
        } else {
            object.insert(key.clone(), serde_json::from_str(text).ok()?);
        }
    }
    Some(object)
}

impl SyncedFields {
    /// The synced fields of `meta`, with paths translated to ids. References to paths
    /// the manifest doesn't know are dropped: they can't be expressed as ids, and a
    /// stale path key carries no information another device could use.
    pub fn from_meta(meta: &ProjectMeta, ids: &PathIds) -> Self {
        let Ok(Value::Object(object)) = serde_json::to_value(meta) else {
            return Self::default();
        };
        let mut fields = Self::default();

        for name in SCALAR_FIELDS {
            if let Some(value) = object.get(*name) {
                fields.scalars.insert((*name).into(), json_text(value));
            }
        }
        for name in FOLDER_REF_FIELDS {
            let referenced = object
                .get(*name)
                .and_then(Value::as_str)
                .and_then(|path| ids.id(path));
            if let Some(id) = referenced {
                fields
                    .scalars
                    .insert((*name).into(), json_text(&Value::String(id.to_string())));
            }
        }
        for name in TEXT_FIELDS {
            if let Some(text) = object.get(*name).and_then(Value::as_str)
                && !text.is_empty()
            {
                fields.texts.insert((*name).into(), text.to_string());
            }
        }
        let string_map = |name: &str| -> BTreeMap<String, String> {
            object
                .get(name)
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        };
        fields.status_colors = string_map("status_colors");
        fields.pov_colors = string_map("pov_colors");

        for (folder, names) in object
            .get("node_order")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            let Some(folder_id) = ids.id(folder) else {
                continue;
            };
            let mut children = Vec::new();
            for name in names
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if let Some(child) = ids.id(&child_key(folder, name))
                    && !children.contains(&child)
                {
                    children.push(child);
                }
            }
            fields.order.insert(folder_id, children);
        }
        for (folder, role) in object
            .get("folder_roles")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if let Some(id) = ids.id(folder) {
                fields.roles.insert(id, json_text(role));
            }
        }
        for (folder, meta) in object
            .get("folder_meta")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if let (Some(id), Some(map)) = (ids.id(folder), object_fields(meta)) {
                fields.folder_meta.insert(id, map);
            }
        }
        for (current, origin) in object
            .get("trashed_origins")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if let (Some(id), Some(origin)) = (ids.id(current), origin.as_str()) {
                fields.trashed.insert(id, origin.to_string());
            }
        }

        fields.story_cards = items_of(object.get("story_cards"))
            .into_iter()
            .map(|mut item| {
                let stems: Vec<String> = item
                    .remove("linked_document_stems")
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .unwrap_or_default();
                let links: Vec<CardLink> = stems
                    .into_iter()
                    .map(|stem| CardLink {
                        target: ids
                            .doc_by_stem(&stem)
                            .map(|id| id.to_string())
                            .unwrap_or_default(),
                        stem,
                    })
                    .collect();
                let links = serde_json::to_string(&links).expect("card links always serialize");
                item.insert("links".into(), links);
                item
            })
            .collect();
        fields.bookmarks = items_of(object.get("bookmarks"))
            .into_iter()
            .map(|mut item| {
                let path = item
                    .get("path")
                    .and_then(|text| serde_json::from_str::<String>(text).ok())
                    .unwrap_or_default();
                let target = ids.id(&path).map(|id| id.to_string()).unwrap_or_default();
                item.insert("target".into(), target);
                item
            })
            .collect();
        fields.notes = items_of(object.get("notes"))
            .into_iter()
            .map(|mut item| {
                let path = item
                    .get("path")
                    .and_then(|text| serde_json::from_str::<String>(text).ok())
                    .unwrap_or_default();
                let target = ids.id(&path).map(|id| id.to_string()).unwrap_or_default();
                item.insert("target".into(), target);
                item
            })
            .collect();
        fields
    }

    /// Writes these fields into `meta`, translating ids back to current paths and
    /// leaving every per-device field exactly as it was. Fails (leaving `meta`
    /// untouched) only if the result wouldn't be a valid `ProjectMeta`.
    pub fn into_meta(&self, meta: &mut ProjectMeta, ids: &PathIds) -> Result<(), String> {
        let Value::Object(mut object) = serde_json::to_value(&*meta).map_err(|e| e.to_string())?
        else {
            return Err("project metadata is not an object".into());
        };

        for name in SCALAR_FIELDS {
            match self
                .scalars
                .get(*name)
                .map(|t| serde_json::from_str::<Value>(t))
            {
                Some(Ok(value)) => {
                    object.insert((*name).into(), value);
                }
                Some(Err(_)) => {}
                None => {
                    object.remove(*name);
                }
            }
        }
        for name in FOLDER_REF_FIELDS {
            let path = self
                .scalars
                .get(*name)
                .and_then(|text| serde_json::from_str::<String>(text).ok())
                .and_then(|id| id.parse::<DocId>().ok())
                .and_then(|id| ids.path(id));
            match path {
                Some(path) => {
                    object.insert((*name).into(), Value::String(path.to_string()));
                }
                None => {
                    object.remove(*name);
                }
            }
        }
        for name in TEXT_FIELDS {
            match self.texts.get(*name) {
                Some(text) => {
                    object.insert((*name).into(), Value::String(text.clone()));
                }
                None => {
                    object.remove(*name);
                }
            }
        }
        let string_map = |map: &BTreeMap<String, String>| {
            Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect(),
            )
        };
        object.insert("status_colors".into(), string_map(&self.status_colors));
        object.insert("pov_colors".into(), string_map(&self.pov_colors));

        let mut order = Map::new();
        for (folder_id, children) in &self.order {
            let Some(folder) = ids.path(*folder_id) else {
                continue;
            };
            let names: Vec<Value> = children
                .iter()
                .filter_map(|child| ids.path(*child))
                .filter_map(|key| {
                    let (parent, name) = split_key(key);
                    (parent == folder).then(|| Value::String(name.to_string()))
                })
                .collect();
            order.insert(folder.to_string(), Value::Array(names));
        }
        object.insert("node_order".into(), Value::Object(order));

        let mut roles = Map::new();
        for (id, role) in &self.roles {
            if let (Some(folder), Ok(value)) = (ids.path(*id), serde_json::from_str::<Value>(role))
            {
                roles.insert(folder.to_string(), value);
            }
        }
        object.insert("folder_roles".into(), Value::Object(roles));

        let mut folder_meta = Map::new();
        for (id, fields) in &self.folder_meta {
            let Some(folder) = ids.path(*id) else {
                continue;
            };
            let parsed: Option<Map<String, Value>> = fields
                .iter()
                .map(|(k, v)| Some((k.clone(), serde_json::from_str(v).ok()?)))
                .collect();
            if let Some(map) = parsed {
                folder_meta.insert(folder.to_string(), Value::Object(map));
            }
        }
        object.insert("folder_meta".into(), Value::Object(folder_meta));

        let mut trashed = Map::new();
        for (id, origin) in &self.trashed {
            if let Some(current) = ids.path(*id) {
                trashed.insert(current.to_string(), Value::String(origin.clone()));
            }
        }
        object.insert("trashed_origins".into(), Value::Object(trashed));

        object.insert(
            "story_cards".into(),
            Value::Array(
                self.story_cards
                    .iter()
                    .filter_map(|item| {
                        let mut item = item.clone();
                        if let Some(links) = item.remove("links") {
                            // Follow each linked document to its current name; keep
                            // the stored name if it isn't known here (yet).
                            let links: Vec<CardLink> =
                                serde_json::from_str(&links).unwrap_or_default();
                            let stems: Vec<String> = links
                                .into_iter()
                                .map(|link| {
                                    // Only a real rename changes the stored name, not
                                    // a difference in case the app ignores anyway.
                                    link.target
                                        .parse::<DocId>()
                                        .ok()
                                        .and_then(|id| ids.stem(id))
                                        .filter(|current| {
                                            current.to_lowercase() != link.stem.to_lowercase()
                                        })
                                        .map_or(link.stem, str::to_string)
                                })
                                .collect();
                            item.insert(
                                "linked_document_stems".into(),
                                json_text(&serde_json::json!(stems)),
                            );
                        }
                        item_to_object(&item)
                    })
                    .map(Value::Object)
                    .collect(),
            ),
        );
        let bookmarks: Vec<Value> = self
            .bookmarks
            .iter()
            .filter_map(|item| {
                let mut item = item.clone();
                // Follow the document if it moved; keep the last known path if the
                // target isn't known here (yet).
                let target = item
                    .remove("target")
                    .and_then(|t| t.parse::<DocId>().ok())
                    .and_then(|id| ids.path(id).map(str::to_string));
                if let Some(path) = target {
                    item.insert("path".into(), json_text(&Value::String(path)));
                }
                item_to_object(&item).map(Value::Object)
            })
            .collect();
        object.insert("bookmarks".into(), Value::Array(bookmarks));
        let notes: Vec<Value> = self
            .notes
            .iter()
            .filter_map(|item| {
                let mut item = item.clone();
                // Follow the document if it moved; keep the last known path if the
                // target isn't known here (yet).
                let target = item
                    .remove("target")
                    .and_then(|t| t.parse::<DocId>().ok())
                    .and_then(|id| ids.path(id).map(str::to_string));
                if let Some(path) = target {
                    item.insert("path".into(), json_text(&Value::String(path)));
                }
                item_to_object(&item).map(Value::Object)
            })
            .collect();
        object.insert("notes".into(), Value::Array(notes));

        *meta = serde_json::from_value(Value::Object(object)).map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn any_string(out: Out) -> Option<String> {
    match out {
        Out::Any(Any::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn read_map<T: ReadTxn>(map: &MapRef, txn: &T) -> BTreeMap<String, String> {
    map.iter(txn)
        .filter_map(|(key, value)| Some((key.to_string(), any_string(value)?)))
        .collect()
}

fn read_items<T: ReadTxn>(array: &ArrayRef, txn: &T) -> Vec<Item> {
    let mut seen = BTreeSet::new();
    array
        .iter(txn)
        .filter_map(|element| match element {
            Out::YMap(map) => Some(read_map(&map, txn)),
            _ => None,
        })
        // A duplicated element (two devices inserted the same id concurrently)
        // reads as one.
        .filter(|item| item.get("id").is_some_and(|id| seen.insert(id.clone())))
        .collect()
}

fn item_prelim(item: &Item) -> MapPrelim {
    item.iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<MapPrelim>()
}

fn write_map(txn: &mut TransactionMut, map: &MapRef, target: &BTreeMap<String, String>) {
    let current = read_map(map, txn);
    for key in current.keys().filter(|k| !target.contains_key(*k)) {
        map.remove(txn, key);
    }
    for (key, value) in target {
        if current.get(key) != Some(value) {
            map.insert(txn, key.as_str(), value.as_str());
        }
    }
}

/// Makes `array` hold exactly `target`, by element id: deletes what's gone, keeps
/// (and updates the fields of) what stays, and inserts what's new — reordering by
/// moving the element. A move is a delete plus an insert, which is fine for lists
/// this small and rarely reordered.
fn write_items(txn: &mut TransactionMut, array: &ArrayRef, target: &[Item]) {
    let wanted: BTreeSet<&str> = target
        .iter()
        .filter_map(|item| item.get("id").map(String::as_str))
        .collect();
    let mut current = read_items(array, txn);
    // The raw array may hold duplicates `read_items` hides, so work on real indices.
    let raw_len = array.len(txn) as usize;
    if raw_len != current.len() {
        let mut seen = BTreeSet::new();
        for index in (0..raw_len).rev() {
            let id = match array.get(txn, index as u32) {
                Some(Out::YMap(map)) => map.get(txn, "id").and_then(any_string),
                _ => None,
            };
            let keep = id.as_ref().is_some_and(|id| wanted.contains(id.as_str()));
            let first = id.as_ref().is_some_and(|id| seen.insert(id.clone()));
            // Iterating backwards, so the first occurrence is the last one seen.
            if !keep || !first {
                array.remove(txn, index as u32);
            }
        }
        current = read_items(array, txn);
    }
    for index in (0..current.len()).rev() {
        let id = current[index].get("id").map(String::as_str).unwrap_or("");
        if !wanted.contains(id) {
            array.remove(txn, index as u32);
            current.remove(index);
        }
    }

    for (index, item) in target.iter().enumerate() {
        let Some(id) = item.get("id") else { continue };
        if current.get(index).and_then(|c| c.get("id")) == Some(id) {
            if &current[index] != item
                && let Some(Out::YMap(map)) = array.get(txn, index as u32)
            {
                write_map(txn, &map, item);
            }
            continue;
        }
        if let Some(found) = current.iter().position(|c| c.get("id") == Some(id)) {
            array.remove(txn, found as u32);
            current.remove(found);
        }
        array.insert(txn, index as u32, item_prelim(item));
        current.insert(index, item.clone());
    }
    while array.len(txn) as usize > target.len() {
        array.remove(txn, target.len() as u32);
    }
}

fn write_text(txn: &mut TransactionMut, text: &TextRef, target: &str) {
    let current = text.get_string(txn);
    if let Some(change) = diff::diff(&current, target) {
        if change.deleted_len > 0 {
            text.remove_range(txn, change.pos as u32, change.deleted_len as u32);
        }
        if !change.inserted.is_empty() {
            text.insert(txn, change.pos as u32, &change.inserted);
        }
    }
}

/// `project.json` as a CRDT document. See the module docs for its shape.
pub struct MetaDoc {
    doc: Doc,
    scalars: MapRef,
    status_colors: MapRef,
    pov_colors: MapRef,
    roles: MapRef,
    trashed: MapRef,
    order: MapRef,
    folder_meta: MapRef,
    story_cards: ArrayRef,
    bookmarks: ArrayRef,
    notes: ArrayRef,
    /// One root text per prose field. Created up front: `Doc::get_or_insert_*` must
    /// not be called while a transaction is open (it deadlocks).
    texts: Vec<(&'static str, TextRef)>,
}

fn text_root(name: &str) -> String {
    format!("text:{name}")
}

impl MetaDoc {
    pub fn new() -> Self {
        let doc = Doc::new();
        Self {
            scalars: doc.get_or_insert_map("scalars"),
            status_colors: doc.get_or_insert_map("status_colors"),
            pov_colors: doc.get_or_insert_map("pov_colors"),
            roles: doc.get_or_insert_map("roles"),
            trashed: doc.get_or_insert_map("trashed"),
            order: doc.get_or_insert_map("order"),
            folder_meta: doc.get_or_insert_map("folder_meta"),
            story_cards: doc.get_or_insert_array("story_cards"),
            bookmarks: doc.get_or_insert_array("bookmarks"),
            notes: doc.get_or_insert_array("notes"),
            texts: TEXT_FIELDS
                .iter()
                .map(|name| (*name, doc.get_or_insert_text(text_root(name).as_str())))
                .collect(),
            doc,
        }
    }

    pub fn from_state(state: &[u8]) -> Result<Self, Error> {
        let mut doc = Self::new();
        doc.apply_update(state)?;
        Ok(doc)
    }

    pub fn encode_state(&self) -> Vec<u8> {
        self.encode_state_since(&StateVector::default())
    }

    /// Everything a peer that has seen `since` is missing (deleted content included
    /// only as compact tombstones). See `engine::repack_pending`.
    pub fn encode_state_since(&self, since: &StateVector) -> Vec<u8> {
        self.doc.transact().encode_state_as_update_v1(since)
    }

    pub fn state_vector(&self) -> StateVector {
        self.doc.transact().state_vector()
    }

    pub fn apply_update(&mut self, update: &[u8]) -> Result<(), Error> {
        let update = Update::decode_v1(update)?;
        self.doc.transact_mut().apply_update(update)?;
        Ok(())
    }

    /// Whether nothing has ever been written here.
    pub fn is_empty(&self) -> bool {
        self.read() == SyncedFields::default()
    }

    pub fn read(&self) -> SyncedFields {
        let txn = self.doc.transact();
        let mut fields = SyncedFields {
            scalars: read_map(&self.scalars, &txn),
            status_colors: read_map(&self.status_colors, &txn),
            pov_colors: read_map(&self.pov_colors, &txn),
            story_cards: read_items(&self.story_cards, &txn),
            bookmarks: read_items(&self.bookmarks, &txn),
            notes: read_items(&self.notes, &txn),
            ..SyncedFields::default()
        };
        for (name, text) in &self.texts {
            let text = text.get_string(&txn);
            if !text.is_empty() {
                fields.texts.insert((*name).into(), text);
            }
        }
        for (key, role) in read_map(&self.roles, &txn) {
            if let Ok(id) = key.parse() {
                fields.roles.insert(id, role);
            }
        }
        for (key, origin) in read_map(&self.trashed, &txn) {
            if let Ok(id) = key.parse() {
                fields.trashed.insert(id, origin);
            }
        }
        for (key, value) in self.order.iter(&txn) {
            if let (Ok(id), Out::YArray(array)) = (key.parse::<DocId>(), value) {
                let mut children = Vec::new();
                for item in read_items(&array, &txn) {
                    if let Some(child) = item.get("id").and_then(|c| c.parse::<DocId>().ok()) {
                        children.push(child);
                    }
                }
                fields.order.insert(id, children);
            }
        }
        for (key, value) in self.folder_meta.iter(&txn) {
            if let (Ok(id), Out::YMap(map)) = (key.parse::<DocId>(), value) {
                fields.folder_meta.insert(id, read_map(&map, &txn));
            }
        }
        fields
    }

    /// Makes this document hold exactly `target`, returning the update describing the
    /// change, or `None` if it already did.
    pub fn write(&mut self, target: &SyncedFields) -> Option<Vec<u8>> {
        if self.read() == *target {
            return None;
        }
        let mut txn = self.doc.transact_mut();
        write_map(&mut txn, &self.scalars, &target.scalars);
        write_map(&mut txn, &self.status_colors, &target.status_colors);
        write_map(&mut txn, &self.pov_colors, &target.pov_colors);
        write_map(
            &mut txn,
            &self.roles,
            &target
                .roles
                .iter()
                .map(|(id, role)| (id.to_string(), role.clone()))
                .collect(),
        );
        write_map(
            &mut txn,
            &self.trashed,
            &target
                .trashed
                .iter()
                .map(|(id, origin)| (id.to_string(), origin.clone()))
                .collect(),
        );
        for (name, text) in &self.texts {
            write_text(
                &mut txn,
                text,
                target.texts.get(*name).map_or("", String::as_str),
            );
        }

        let existing: Vec<String> = self.order.keys(&txn).map(str::to_string).collect();
        for key in existing {
            let keep = key
                .parse::<DocId>()
                .is_ok_and(|id| target.order.contains_key(&id));
            if !keep {
                self.order.remove(&mut txn, &key);
            }
        }
        for (id, children) in &target.order {
            let key = id.to_string();
            let array = match self.order.get(&txn, &key) {
                Some(Out::YArray(array)) => array,
                _ => self.order.insert(&mut txn, key, ArrayPrelim::default()),
            };
            let items: Vec<Item> = children
                .iter()
                .map(|child| Item::from([("id".to_string(), child.to_string())]))
                .collect();
            write_items(&mut txn, &array, &items);
        }

        let existing: Vec<String> = self.folder_meta.keys(&txn).map(str::to_string).collect();
        for key in existing {
            let keep = key
                .parse::<DocId>()
                .is_ok_and(|id| target.folder_meta.contains_key(&id));
            if !keep {
                self.folder_meta.remove(&mut txn, &key);
            }
        }
        for (id, fields) in &target.folder_meta {
            let key = id.to_string();
            let map = match self.folder_meta.get(&txn, &key) {
                Some(Out::YMap(map)) => map,
                _ => self.folder_meta.insert(&mut txn, key, MapPrelim::default()),
            };
            write_map(&mut txn, &map, fields);
        }

        write_items(&mut txn, &self.story_cards, &target.story_cards);
        write_items(&mut txn, &self.bookmarks, &target.bookmarks);
        write_items(&mut txn, &self.notes, &target.notes);
        Some(txn.encode_update_v1())
    }
}

impl Default for MetaDoc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::manifest::EntryKind;
    use serde_json::json;
    use uuid::Uuid;

    /// Well clear of the reserved ids (1-3).
    fn id(n: u128) -> DocId {
        DocId(Uuid::from_u128(1000 + n))
    }

    fn entry(n: u128, path: &str, kind: EntryKind) -> ManifestEntry {
        ManifestEntry {
            doc_id: id(n),
            kind,
            path: path.into(),
            deleted: false,
        }
    }

    /// Draft/{Ch1,Ch2}, Notes.md, Trash/.
    fn world() -> Vec<ManifestEntry> {
        vec![
            entry(1, "Draft", EntryKind::Dir),
            entry(2, "Draft/Ch1.md", EntryKind::Doc),
            entry(3, "Draft/Ch2.md", EntryKind::Doc),
            entry(4, "Notes.md", EntryKind::Doc),
            entry(5, "Trash", EntryKind::Dir),
            entry(6, "Trash/Old.md", EntryKind::Doc),
        ]
    }

    fn card(n: u128, cause: &str, effect: &str) -> Value {
        json!({
            "id": Uuid::from_u128(n).to_string(),
            "scene_number": n.to_string(),
            "alpha_point": "",
            "subplot_tags": [],
            "cause": cause,
            "effect": effect,
            "realization": "r",
            "and_so": "a",
        })
    }

    fn rich_meta() -> ProjectMeta {
        serde_json::from_value(json!({
            "version": 1,
            "node_order": { "": ["Draft", "Notes.md", "Trash"], "Draft": ["Ch2.md", "Ch1.md"], "Trash": ["Old.md"] },
            "folder_roles": { "Draft": "Manuscript", "Trash": "Trash" },
            "trashed_origins": { "Trash/Old.md": "Draft/Old.md" },
            "folder_meta": { "Draft": { "status": "draft" } },
            "status_colors": { "draft": "#ff0000" },
            "pov_colors": { "Anna": "#00ff00" },
            "binder_color_mode": "Pov",
            "story_cards": [card(10, "c1", "e1"), card(11, "c2", "e2")],
            "protagonist_desire": "to leave",
            "logline": "A girl and a fjord.",
            "synopsis": "Once upon a time.",
            "book_title": "Fjord",
            "type_picklist_folder": "Draft",
            "draft_target_words": 80000,
            "bookmarks": [{ "id": Uuid::from_u128(20).to_string(), "path": "Draft/Ch1.md", "line": 7 }],
            "git_enabled": true,
            "plugins_enabled": true,
            "session_baseline_words": 1234,
            "streak_enabled": true,
        }))
        .unwrap()
    }

    fn fork(doc: &MetaDoc) -> MetaDoc {
        MetaDoc::from_state(&doc.encode_state()).unwrap()
    }

    fn seeded() -> (MetaDoc, PathIds) {
        let ids = PathIds::from_entries(&world());
        let mut doc = MetaDoc::new();
        doc.write(&SyncedFields::from_meta(&rich_meta(), &ids));
        (doc, ids)
    }

    #[test]
    fn every_project_meta_field_is_classified_exactly_once() {
        let value = serde_json::to_value(ProjectMeta::default()).unwrap();
        let real: BTreeSet<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut listed = Vec::new();
        for list in [
            SCALAR_FIELDS,
            FOLDER_REF_FIELDS,
            TEXT_FIELDS,
            LOCAL_ONLY_FIELDS,
            STRUCTURED_FIELDS,
        ] {
            listed.extend_from_slice(list);
        }
        let unique: BTreeSet<&str> = listed.iter().copied().collect();
        assert_eq!(
            unique.len(),
            listed.len(),
            "a field is in two lists: {listed:?}"
        );
        assert_eq!(unique, real, "lists and ProjectMeta fields must match");
    }

    #[test]
    fn synced_fields_round_trip_through_a_meta_and_keep_local_fields_local() {
        let ids = PathIds::from_entries(&world());
        let original = rich_meta();
        let fields = SyncedFields::from_meta(&original, &ids);

        // Another device's meta: different per-device state, nothing synced yet.
        let mut other: ProjectMeta = serde_json::from_value(json!({
            "version": 1,
            "node_order": {},
            "git_enabled": false,
            "plugins_enabled": false,
            "session_baseline_words": 5,
        }))
        .unwrap();
        fields.into_meta(&mut other, &ids).unwrap();

        let (a, b) = (
            serde_json::to_value(&original).unwrap(),
            serde_json::to_value(&other).unwrap(),
        );
        for key in SCALAR_FIELDS
            .iter()
            .chain(FOLDER_REF_FIELDS)
            .chain(TEXT_FIELDS)
            .chain(STRUCTURED_FIELDS)
        {
            assert_eq!(a[*key], b[*key], "synced field {key} should match");
        }
        // Per-device state stays what it was on `other`.
        assert!(!other.git_enabled);
        assert!(!other.plugins_enabled);
        assert_eq!(other.session_baseline_words, 5);
        assert!(!other.streak_enabled);
    }

    #[test]
    fn per_device_fields_never_enter_the_synced_snapshot() {
        let ids = PathIds::from_entries(&world());
        let mut a = rich_meta();
        let mut b = rich_meta();
        a.plugins_enabled = true;
        b.plugins_enabled = false;
        a.git_enabled = false;
        b.git_enabled = true;
        b.session_baseline_words = 999;
        b.streak_enabled = false;
        b.daily_word_counts.insert("2026-09-01".into(), 500);
        assert_eq!(
            SyncedFields::from_meta(&a, &ids),
            SyncedFields::from_meta(&b, &ids)
        );
    }

    #[test]
    fn a_remote_meta_cannot_switch_on_plugins() {
        let ids = PathIds::from_entries(&world());
        let remote_fields = SyncedFields::from_meta(&rich_meta(), &ids); // plugins_enabled = true there
        let mut mine = ProjectMeta::default();
        assert!(!mine.plugins_enabled);
        remote_fields.into_meta(&mut mine, &ids).unwrap();
        assert!(!mine.plugins_enabled, "plugin consent must stay local");
    }

    #[test]
    fn references_to_unknown_paths_are_dropped_not_invented() {
        let ids = PathIds::from_entries(&world());
        let mut meta = rich_meta();
        meta.node_order.insert("Ghost".into(), vec!["x.md".into()]);
        meta.folder_roles.insert(
            "Ghost".into(),
            serde_json::from_value(json!("Research")).unwrap(),
        );
        let fields = SyncedFields::from_meta(&meta, &ids);
        assert_eq!(fields.order.len(), 3, "root, Draft, Trash only");
        assert_eq!(fields.roles.len(), 2);
    }

    #[test]
    fn a_crdt_write_reads_back_identically_and_a_repeat_is_a_no_op() {
        let (mut doc, ids) = seeded();
        let fields = SyncedFields::from_meta(&rich_meta(), &ids);
        assert_eq!(doc.read(), fields);
        assert!(doc.write(&fields).is_none());
        assert_eq!(fork(&doc).read(), fields);
        assert!(!doc.is_empty());
        assert!(MetaDoc::new().is_empty());
    }

    #[test]
    fn concurrent_edits_to_different_settings_prose_and_cards_all_survive() {
        let (base, ids) = seeded();
        let (mut a, mut b) = (fork(&base), fork(&base));
        let original = SyncedFields::from_meta(&rich_meta(), &ids);

        let mut fa = original.clone();
        fa.texts
            .insert("logline".into(), "A girl, a fjord and a storm.".into());
        fa.status_colors.insert("final".into(), "#0000ff".into());
        let mut fb = original.clone();
        fb.scalars
            .insert("book_title".into(), json_text(&json!("Fjord II")));
        fb.story_cards
            .extend(items_of(Some(&json!([card(12, "c3", "e3")]))));
        fb.folder_meta
            .get_mut(&id(1))
            .unwrap()
            .insert("pov".into(), json_text(&json!("Anna")));

        let (ua, ub) = (a.write(&fa).unwrap(), b.write(&fb).unwrap());
        a.apply_update(&ub).unwrap();
        b.apply_update(&ua).unwrap();

        assert_eq!(a.read(), b.read());
        let merged = a.read();
        assert_eq!(merged.texts["logline"], "A girl, a fjord and a storm.");
        assert!(merged.status_colors.contains_key("final"));
        assert_eq!(merged.scalars["book_title"], "\"Fjord II\"");
        assert_eq!(merged.story_cards.len(), 3);
        assert!(merged.folder_meta[&id(1)].contains_key("pov"));
        assert!(merged.folder_meta[&id(1)].contains_key("status"));
    }

    #[test]
    fn concurrent_prose_edits_merge_character_by_character() {
        let (base, ids) = seeded();
        let (mut a, mut b) = (fork(&base), fork(&base));
        let original = SyncedFields::from_meta(&rich_meta(), &ids);
        let mut fa = original.clone();
        fa.texts
            .insert("synopsis".into(), "Once upon a time, far away.".into());
        let mut fb = original.clone();
        fb.texts
            .insert("synopsis".into(), "Once upon a time.\nThe end.".into());
        let (ua, ub) = (a.write(&fa).unwrap(), b.write(&fb).unwrap());
        a.apply_update(&ub).unwrap();
        b.apply_update(&ua).unwrap();
        assert_eq!(a.read(), b.read());
        let merged = &a.read().texts["synopsis"];
        assert!(
            merged.contains("far away") && merged.contains("The end."),
            "{merged}"
        );
    }

    #[test]
    fn different_fields_of_the_same_story_card_both_survive() {
        let (base, ids) = seeded();
        let (mut a, mut b) = (fork(&base), fork(&base));
        let original = SyncedFields::from_meta(&rich_meta(), &ids);
        let mut fa = original.clone();
        fa.story_cards[0].insert("cause".into(), json_text(&json!("edited on A")));
        let mut fb = original.clone();
        fb.story_cards[0].insert("effect".into(), json_text(&json!("edited on B")));
        let (ua, ub) = (a.write(&fa).unwrap(), b.write(&fb).unwrap());
        a.apply_update(&ub).unwrap();
        b.apply_update(&ua).unwrap();
        assert_eq!(a.read(), b.read());
        let card = &a.read().story_cards[0];
        assert_eq!(card["cause"], "\"edited on A\"");
        assert_eq!(card["effect"], "\"edited on B\"");
    }

    #[test]
    fn concurrent_card_additions_and_a_delete_merge() {
        let (base, ids) = seeded();
        let (mut a, mut b) = (fork(&base), fork(&base));
        let original = SyncedFields::from_meta(&rich_meta(), &ids);
        let mut fa = original.clone();
        fa.story_cards.remove(0);
        let mut fb = original.clone();
        fb.story_cards
            .extend(items_of(Some(&json!([card(30, "new", "new")]))));
        let (ua, ub) = (a.write(&fa).unwrap(), b.write(&fb).unwrap());
        a.apply_update(&ub).unwrap();
        b.apply_update(&ua).unwrap();
        assert_eq!(a.read(), b.read());
        let ids: Vec<_> = a
            .read()
            .story_cards
            .iter()
            .map(|c| c["id"].clone())
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(!ids.contains(&Uuid::from_u128(10).to_string()));
        assert!(ids.contains(&Uuid::from_u128(30).to_string()));
    }

    #[test]
    fn concurrent_appends_to_one_folders_order_both_survive() {
        let (base, ids) = seeded();
        let (mut a, mut b) = (fork(&base), fork(&base));
        let original = SyncedFields::from_meta(&rich_meta(), &ids);
        let mut fa = original.clone();
        fa.order.get_mut(&id(1)).unwrap().push(id(4));
        let mut fb = original.clone();
        fb.order.get_mut(&id(1)).unwrap().push(id(5));
        let (ua, ub) = (a.write(&fa).unwrap(), b.write(&fb).unwrap());
        a.apply_update(&ub).unwrap();
        b.apply_update(&ua).unwrap();
        assert_eq!(a.read(), b.read());
        let merged = &a.read().order[&id(1)];
        assert!(
            merged.contains(&id(4)) && merged.contains(&id(5)),
            "{merged:?}"
        );
        assert_eq!(merged.len(), 4);
    }

    #[test]
    fn a_reordered_list_reads_back_in_the_new_order() {
        let (mut doc, ids) = seeded();
        let mut target = SyncedFields::from_meta(&rich_meta(), &ids);
        target.story_cards.reverse();
        target.order.get_mut(&id(1)).unwrap().reverse();
        assert!(doc.write(&target).is_some());
        assert_eq!(doc.read(), target);
        assert_eq!(fork(&doc).read(), target);
    }

    #[test]
    fn a_renamed_folder_keeps_its_order_role_and_bookmarks() {
        let old_ids = PathIds::from_entries(&world());
        let fields = SyncedFields::from_meta(&rich_meta(), &old_ids);

        // Another device renamed "Draft" to "Manuscript" (same ids, new paths).
        let renamed = vec![
            entry(1, "Manuscript", EntryKind::Dir),
            entry(2, "Manuscript/Ch1.md", EntryKind::Doc),
            entry(3, "Manuscript/Ch2.md", EntryKind::Doc),
            entry(4, "Notes.md", EntryKind::Doc),
            entry(5, "Trash", EntryKind::Dir),
            entry(6, "Trash/Old.md", EntryKind::Doc),
        ];
        let new_ids = PathIds::from_entries(&renamed);
        let mut meta = ProjectMeta::default();
        fields.into_meta(&mut meta, &new_ids).unwrap();

        assert_eq!(meta.node_order["Manuscript"], vec!["Ch2.md", "Ch1.md"]);
        assert_eq!(meta.node_order[""], vec!["Manuscript", "Notes.md", "Trash"]);
        assert!(!meta.node_order.contains_key("Draft"));
        assert!(meta.folder_roles.contains_key("Manuscript"));
        assert!(meta.folder_meta.contains_key("Manuscript"));
        assert_eq!(meta.type_picklist_folder.as_deref(), Some("Manuscript"));
        assert_eq!(meta.bookmarks[0].path, "Manuscript/Ch1.md");
        assert_eq!(meta.bookmarks[0].line, 7);
    }

    #[test]
    fn a_corrupt_remote_card_is_skipped_without_losing_the_rest() {
        let ids = PathIds::from_entries(&world());
        let mut fields = SyncedFields::from_meta(&rich_meta(), &ids);
        fields.story_cards[0].insert("cause".into(), "{not json".into());
        let mut meta = ProjectMeta::default();
        fields.into_meta(&mut meta, &ids).unwrap();
        assert_eq!(meta.story_cards.len(), 1);
        assert_eq!(meta.logline, "A girl and a fjord.");
    }

    #[test]
    fn an_entry_claiming_a_reserved_id_cannot_hijack_the_root() {
        let mut entries = world();
        entries.push(ManifestEntry {
            doc_id: DocId::PROJECT_ROOT,
            kind: EntryKind::Dir,
            path: "Hijack".into(),
            deleted: false,
        });
        let ids = PathIds::from_entries(&entries);
        assert_eq!(ids.path(DocId::PROJECT_ROOT), Some(""));
        assert_eq!(ids.id("Hijack"), None);
    }

    #[test]
    fn keys_from_before_a_rename_still_resolve_through_the_previous_layout() {
        let before = PathIds::from_entries(&world());
        let renamed = vec![
            entry(1, "Manuscript", EntryKind::Dir),
            entry(2, "Manuscript/Ch1.md", EntryKind::Doc),
            entry(3, "Manuscript/Ch2.md", EntryKind::Doc),
            entry(4, "Notes.md", EntryKind::Doc),
            entry(5, "Trash", EntryKind::Dir),
            entry(6, "Trash/Old.md", EntryKind::Doc),
        ];
        let after = PathIds::from_entries(&renamed);
        // A project.json still written with the old names...
        let stale = rich_meta();
        assert_eq!(
            SyncedFields::from_meta(&stale, &after).order.len(),
            2,
            "without the fallback the Draft entries are lost"
        );
        let with_fallback = after.with_previous(before.snapshot());
        let fields = SyncedFields::from_meta(&stale, &with_fallback);
        assert_eq!(fields.order[&id(1)], vec![id(3), id(2)]);
        assert!(fields.roles.contains_key(&id(1)));
        assert!(fields.folder_meta.contains_key(&id(1)));
        assert_eq!(fields.bookmarks[0]["target"], id(2).to_string());
    }

    fn meta_with_card_links(stems: &[&str]) -> ProjectMeta {
        let mut card = card(20, "c", "e");
        card["linked_document_stems"] = json!(stems);
        serde_json::from_value(json!({ "version": 1, "node_order": {}, "story_cards": [card] }))
            .unwrap()
    }

    /// `world()` with Draft/Ch1.md renamed to Draft/Chapter One.md (same id).
    fn world_with_ch1_renamed() -> Vec<ManifestEntry> {
        let mut entries = world();
        entries[1] = entry(2, "Draft/Chapter One.md", EntryKind::Doc);
        entries
    }

    #[test]
    fn card_links_follow_a_renamed_document() {
        let ids = PathIds::from_entries(&world());
        let fields = SyncedFields::from_meta(&meta_with_card_links(&["Ch1", "Notes"]), &ids);

        let mut meta = ProjectMeta::default();
        fields
            .into_meta(&mut meta, &PathIds::from_entries(&world_with_ch1_renamed()))
            .unwrap();
        assert_eq!(
            meta.story_cards[0].linked_document_stems,
            vec!["Chapter One", "Notes"]
        );
    }

    #[test]
    fn a_card_linked_under_an_old_name_still_reaches_the_renamed_document() {
        // This device renamed Ch1; the other one, not knowing yet, linked a card to it
        // by its old name, and that edit wins the merge.
        let (base, old_ids) = seeded();
        let (mut here, mut there) = (fork(&base), fork(&base));
        let original = SyncedFields::from_meta(&rich_meta(), &old_ids);
        let mut linked = original.clone();
        let theirs = SyncedFields::from_meta(&meta_with_card_links(&["Ch1"]), &old_ids);
        linked.story_cards[0].insert("links".into(), theirs.story_cards[0]["links"].clone());
        here.apply_update(&there.write(&linked).unwrap()).unwrap();

        let new_ids = PathIds::from_entries(&world_with_ch1_renamed());
        let mut meta = rich_meta();
        here.read().into_meta(&mut meta, &new_ids).unwrap();
        assert_eq!(
            meta.story_cards[0].linked_document_stems,
            vec!["Chapter One"]
        );

        // A project.json still written with the old name resolves through the
        // previous layout, like every other path reference.
        let fallback = new_ids.with_previous(old_ids.snapshot());
        let fields = SyncedFields::from_meta(&meta_with_card_links(&["Ch1"]), &fallback);
        assert!(fields.story_cards[0]["links"].contains(&id(2).to_string()));
    }

    #[test]
    fn ambiguous_unknown_and_differently_cased_card_links_are_left_alone() {
        let mut entries = world();
        entries.push(entry(7, "Trash/Ch2.md", EntryKind::Doc)); // a second "Ch2"
        let ids = PathIds::from_entries(&entries);
        assert_eq!(
            ids.doc_by_stem("ch1"),
            Some(id(2)),
            "matched like the app does"
        );
        assert_eq!(ids.doc_by_stem("Ch2"), None, "ambiguous");
        assert_eq!(ids.doc_by_stem("Nowhere"), None);
        assert_eq!(ids.doc_by_stem("Draft"), None, "folders aren't documents");

        let stems = ["ch1", "Ch2", "Nowhere"];
        let fields = SyncedFields::from_meta(&meta_with_card_links(&stems), &ids);
        let mut meta = ProjectMeta::default();
        fields.into_meta(&mut meta, &ids).unwrap();
        assert_eq!(meta.story_cards[0].linked_document_stems, stems);
    }

    #[test]
    fn tombstoned_entries_do_not_resolve() {
        let mut entries = world();
        entries[1].deleted = true; // Draft/Ch1.md
        let ids = PathIds::from_entries(&entries);
        let fields = SyncedFields::from_meta(&rich_meta(), &ids);
        assert!(!fields.order[&id(1)].contains(&id(2)));
    }
}
