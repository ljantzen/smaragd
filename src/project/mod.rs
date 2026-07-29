pub mod model;
mod scan;

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use model::{BinderNode, BinderNodeKind, BinderTree};
use scan::scan_project;

const METADATA_DIR: &str = ".smaragd";
const METADATA_FILE: &str = "project.json";

/// Failure to load a project from a folder.
#[derive(Debug)]
pub enum LoadError {
    /// `root` doesn't exist or isn't a directory.
    NotADirectory(PathBuf),
    /// `root` is a plain folder smaragd has never opened before — no
    /// `.smaragd/project.json` marker is present. Distinguished from other IO
    /// failures so callers can offer to adopt/initialize the folder instead of just
    /// reporting failure.
    NotInitialized(PathBuf),
    /// Any other IO failure while creating the marker or scanning the project.
    Io(io::Error),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NotADirectory(path) => write!(f, "{} is not a directory", path.display()),
            LoadError::NotInitialized(path) => {
                write!(
                    f,
                    "{} has not been set up as a smaragd project",
                    path.display()
                )
            }
            LoadError::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<io::Error> for LoadError {
    fn from(err: io::Error) -> Self {
        LoadError::Io(err)
    }
}

/// Failure to restore an item from Trash.
#[derive(Debug)]
pub enum RestoreError {
    /// `path` has no recorded origin — it isn't a top-level trashed item.
    NotTrashed,
    /// The folder this item originally lived in no longer exists.
    OriginalFolderMissing(PathBuf),
    Io(io::Error),
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RestoreError::NotTrashed => write!(f, "not a trashed item"),
            RestoreError::OriginalFolderMissing(path) => {
                write!(
                    f,
                    "original location \"{}\" no longer exists",
                    path.display()
                )
            }
            RestoreError::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for RestoreError {}

impl From<io::Error> for RestoreError {
    fn from(err: io::Error) -> Self {
        RestoreError::Io(err)
    }
}

/// A Scrivener-Research/Trash/Templates-style role assigned to a folder, decoupled
/// from its position in the tree. At most one folder project-wide holds a given role
/// at a time. `Research` is currently just a marker — a forward-looking extension
/// point for features (Compile, word-count rollups) that don't exist yet. `Trash` has
/// a real behavior change: see [`Project::delete`]. `Templates`'s direct child
/// documents become the candidate list for "New From Template": see
/// [`Project::template_documents`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FolderRole {
    Research,
    Trash,
    Templates,
}

/// Which metadata field a folder's direct child documents' titles populate as
/// dropdown options in the Metadata panel — see [`Project::picklist_documents`].
/// Deliberately *not* a [`FolderRole`]: a folder assigned here gets no other special
/// behavior (no export exclusion, no Trash-style semantics, no project-wide
/// exclusivity across fields) — it's purely a pointer used to build a dropdown, so an
/// existing folder that already serves another purpose (e.g. a Research subfolder of
/// character bios) can double as a picklist source without anything else about it
/// changing. Each field is its own independent slot on [`ProjectMeta`] (see
/// `type_picklist_folder`/`pov_picklist_folder`/`status_picklist_folder`), so —
/// unlike `FolderRole`'s single shared map — assigning one field's folder never
/// affects another field's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PicklistField {
    Type,
    Pov,
    Status,
}

/// How `move_node_with` should resolve a name collision in the destination folder —
/// see `Project::move_node`/`move_item`, its two callers, for when each applies.
enum NameCollision {
    Uniquify,
    Refuse,
}

/// Manual ordering and per-node metadata that the filesystem itself can't express.
/// Keyed by a `/`-separated path relative to the project root ("" for the root folder
/// itself) rather than `PathBuf`, so the file stays portable across platforms and
/// serializes to plain JSON without ambiguity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectMeta {
    pub version: u32,
    pub node_order: HashMap<String, Vec<String>>,
    /// `#[serde(default)]` is required, not cosmetic: project.json files written
    /// before this field existed have no `folder_roles`/`trashed_origins` keys at
    /// all — without a default, deserializing them would fail outright and silently
    /// discard their real, already-persisted `node_order` data.
    #[serde(default)]
    pub folder_roles: HashMap<String, FolderRole>,
    /// A trashed item's *current* relative key (its path inside the Trash folder,
    /// post-move) → its *original* relative key (where it lived pre-delete).
    /// Disambiguates same-named items trashed from different folders and is what a
    /// future "restore from trash" action needs to put something back where it came
    /// from — the on-disk name alone (deduplicated with a " (2)" suffix on collision)
    /// doesn't carry that.
    #[serde(default)]
    pub trashed_origins: HashMap<String, String>,
    /// Lisa Cron-style story/plotting cards, deliberately *not* tied to the binder
    /// tree or `node_order`: a card may exist with no linked document at all (a pure
    /// plotting artifact, drafted before any scene exists) and its position in this
    /// list — the corkboard order — is independent of manuscript order. Storing them
    /// here rather than as document frontmatter sidesteps frontmatter write-back
    /// entirely (see `frontmatter.rs`'s doc comment on why that isn't implemented).
    #[serde(default)]
    pub story_cards: Vec<StoryCard>,
    /// The protagonist's driving external/internal want — half of Lisa Cron's
    /// "Third Rail" (the other half is `protagonist_misbelief`): the throughline
    /// every scene's `StoryCard::why_it_matters` should ultimately test or advance.
    /// Project-wide rather than per-scene, since it's meant to anchor the whole
    /// manuscript's arc, not vary scene to scene. Edited from the Corkboard view.
    #[serde(default)]
    pub protagonist_desire: String,
    /// The flawed, usually childhood-formed belief standing between the protagonist
    /// and `protagonist_desire` — see that field's doc comment.
    #[serde(default)]
    pub protagonist_misbelief: String,
    /// Whether git version control (commit/push/pull from the Versions menu, modeled
    /// after the Obsidian Git plugin) is turned on for this project. Deliberately a
    /// per-project setting, not a global one in `Settings`/`settings.rs`: one project
    /// folder might be a git repo (or want to be) while another isn't, and there's no
    /// single "on for every project" answer that would make sense.
    #[serde(default)]
    pub git_enabled: bool,
    /// Whether the user has already been asked (via the one-time "enable git
    /// support?" dialog) whether to turn `git_enabled` on — regardless of their
    /// answer, prevents nagging them again every time the project is opened. Doesn't
    /// block a later manual "Enable Git Support" from the Versions menu.
    #[serde(default)]
    pub git_prompted: bool,
    /// Whether this project's own `.smaragd/plugins/*.rhai` scripts are loaded,
    /// in addition to the always-loaded global plugin directory. Off by default and
    /// requires an explicit action to turn on (see `Project::set_plugins_enabled`)
    /// — unlike a global plugin (which the user deliberately placed themselves), a
    /// project's own plugin folder could arrive via a shared/pulled git repo, so
    /// loading it without consent would be silent code execution from someone
    /// else's content, not just a convenience default.
    #[serde(default)]
    pub plugins_enabled: bool,
    /// Book-level title/author, entered once in the Export dialog and reused on
    /// every later export rather than retyped each time. `None` (not an empty
    /// string) until the user has actually set one, so a never-exported project's
    /// `project.json` doesn't grow two empty-string keys for no reason.
    #[serde(default)]
    pub book_title: Option<String>,
    #[serde(default)]
    pub book_author: Option<String>,
    /// The chosen `export::style::TypesetStyle` id, same reuse-across-exports
    /// rationale as `book_title`/`book_author`. `None` means "use the export
    /// dialog's own default" rather than a project.json key with a specific
    /// style id baked in, so the default can change later without a migration.
    #[serde(default)]
    pub book_style: Option<String>,
    /// The relative key (same `/`-joined, root-is-`""` encoding `folder_roles` uses)
    /// of the folder whose direct child documents' titles populate the Type field's
    /// dropdown in the Metadata panel — see [`PicklistField::Type`] and
    /// [`Project::picklist_documents`]. `None` (the default) keeps that field plain
    /// free text, unchanged from before this existed.
    #[serde(default)]
    pub type_picklist_folder: Option<String>,
    /// Same as `type_picklist_folder`, for the POV field.
    #[serde(default)]
    pub pov_picklist_folder: Option<String>,
    /// Same as `type_picklist_folder`, for the Status field.
    #[serde(default)]
    pub status_picklist_folder: Option<String>,
}

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

/// One `[[wikilink]]` occurrence elsewhere in the project that resolves to a given
/// target document, found by [`Project::backlinks`]. A derived, on-demand query
/// result over a `Project` — not part of the binder tree's own shape, so it lives
/// here rather than in `model.rs`.
#[derive(Debug, Clone, PartialEq)]
pub struct BacklinkEntry {
    /// Absolute path of the document containing the link.
    pub source_path: PathBuf,
    /// The linking document's display title — its filename without the `.md`
    /// extension, matching how the binder and `document_names` present titles.
    pub source_title: String,
    /// A short, single-line excerpt of text around the link (see
    /// `markdown::wikilink_context_snippet`).
    pub snippet: String,
}

/// One document in the project together with its tags — found by
/// [`Project::tag_index`], the shared scan behind [`Project::related_by_tag`]
/// and [`Project::documents_with_tag`].
#[derive(Debug, Clone, PartialEq)]
struct TaggedDocument {
    path: PathBuf,
    title: String,
    tags: Vec<String>,
}

/// One tag on a queried document, together with every *other* document in the
/// project that also carries it — found by [`Project::related_by_tag`]. Kept
/// even when `documents` is empty, so a caller (the Tags dock) can still show
/// "this document has this tag, but nothing else does yet" rather than
/// silently omitting it.
#[derive(Debug, Clone, PartialEq)]
pub struct TagGroup {
    pub tag: String,
    pub documents: Vec<(PathBuf, String)>,
}

pub struct Project {
    pub root: PathBuf,
    pub tree: BinderTree,
    pub meta: ProjectMeta,
}

impl Project {
    /// Load a project from `root`. `root` must already be a smaragd project (i.e.
    /// have a `.smaragd/project.json` marker) — use [`Project::initialize`] to
    /// create one first. A *corrupt* (as opposed to absent) marker file is still not
    /// an error, falling back to default metadata.
    pub fn load_from_folder(root: &Path) -> Result<Project, LoadError> {
        if !root.is_dir() {
            return Err(LoadError::NotADirectory(root.to_path_buf()));
        }
        if !metadata_path(root).is_file() {
            return Err(LoadError::NotInitialized(root.to_path_buf()));
        }

        let meta = load_metadata(root).unwrap_or_default();
        let mut tree = scan_project(root);
        apply_order(&mut tree.root, root, &meta.node_order);

        Ok(Project {
            root: root.to_path_buf(),
            tree,
            meta,
        })
    }

    /// Ensure `root` exists and is marked as a smaragd project — creating it and/or
    /// writing a fresh `.smaragd/project.json` with default metadata if one isn't
    /// already there — then load it. Never overwrites existing metadata, so calling
    /// this on an already-initialized project is a no-op beyond a normal
    /// `load_from_folder`. Backs both "New Project" (a path that doesn't exist yet)
    /// and adopting an existing folder of markdown files as a project.
    pub fn initialize(root: &Path) -> Result<Project, LoadError> {
        fs::create_dir_all(root)?;
        if !metadata_path(root).is_file() {
            save_metadata(root, &ProjectMeta::default())?;
        }
        Self::load_from_folder(root)
    }

    /// Read and parse the YAML frontmatter of the document at `path`. On-demand
    /// only — nothing in `BinderNode`/`ProjectMeta` carries per-document metadata, so
    /// call this only when a specific document's metadata is actually needed.
    pub fn document_meta(&self, path: &Path) -> io::Result<crate::frontmatter::DocumentMeta> {
        let contents = fs::read_to_string(path)?;
        Ok(crate::frontmatter::parse(&contents))
    }

    /// Every `[[wikilink]]` elsewhere in the project whose target resolves (by
    /// filename, full-Unicode-case-insensitively — the same rule
    /// `BinderTree::find_document_by_stem` uses) to the document at `target_path`.
    /// Excludes links from `target_path` to itself. One entry per link occurrence,
    /// not collapsed per source document — a document linking twice produces two
    /// entries, each with its own snippet, matching how Obsidian's own backlinks
    /// panel never hides a second occurrence's distinct context.
    ///
    /// Recomputed fresh from disk on every call, like everything else in `Project`/
    /// `BinderTree` (see `rename_wikilinks_everywhere`) — a document that can't be
    /// read is skipped rather than failing the whole scan, since one unreadable file
    /// shouldn't blank out every other legitimate backlink.
    pub fn backlinks(&self, target_path: &Path) -> Vec<BacklinkEntry> {
        let Some(target_stem) = target_path.file_stem().and_then(|stem| stem.to_str()) else {
            return Vec::new();
        };
        let target_stem = target_stem.to_lowercase();

        let mut entries = Vec::new();
        for doc_path in self.tree.document_paths() {
            if doc_path == target_path {
                continue;
            }
            let Ok(contents) = fs::read_to_string(&doc_path) else {
                continue;
            };
            // Strip frontmatter before scanning: without this, a wikilink close to
            // the top of a document's body could pull YAML frontmatter text into
            // its snippet's "surrounding context" window instead of actual prose.
            let contents = crate::frontmatter::strip(&contents);
            let Some(source_title) = doc_path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            for (range, link_target) in crate::markdown::wikilink_spans(contents) {
                if link_target.to_lowercase() != target_stem {
                    continue;
                }
                entries.push(BacklinkEntry {
                    source_path: doc_path.clone(),
                    source_title: source_title.to_string(),
                    snippet: crate::markdown::wikilink_context_snippet(contents, &range),
                });
            }
        }
        entries
    }

    /// Every document in the project, together with its tags — frontmatter
    /// `tags:` plus inline `#tag` mentions in the body, case-insensitively
    /// deduplicated (frontmatter's casing wins over an inline mention's, since
    /// it's the more deliberately authored form). The shared full-vault scan
    /// behind [`Project::related_by_tag`] and [`Project::documents_with_tag`];
    /// recomputed fresh from disk on every call, like `backlinks` — a document
    /// that can't be read is skipped rather than failing the whole scan.
    fn tag_index(&self) -> Vec<TaggedDocument> {
        let mut index = Vec::new();
        for doc_path in self.tree.document_paths() {
            let Ok(contents) = fs::read_to_string(&doc_path) else {
                continue;
            };
            let Some(title) = doc_path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let meta = crate::frontmatter::parse(&contents);
            let body = crate::frontmatter::strip(&contents);
            let mut tags = meta.tags.clone();
            for inline_tag in crate::markdown::inline_tags(body) {
                if !tags.iter().any(|tag| tag.eq_ignore_ascii_case(&inline_tag)) {
                    tags.push(inline_tag);
                }
            }
            index.push(TaggedDocument {
                path: doc_path.clone(),
                title: title.to_string(),
                tags,
            });
        }
        index
    }

    /// Every tag on the document at `target_path`, each paired with every
    /// *other* document in the project that also carries it (case-insensitive
    /// match), sorted alphabetically by tag. Populates the Tags dock for
    /// whatever document is currently open. Empty if `target_path` isn't a
    /// document in the project, or carries no tags of its own.
    pub fn related_by_tag(&self, target_path: &Path) -> Vec<TagGroup> {
        let index = self.tag_index();
        let Some(target) = index.iter().find(|doc| doc.path == target_path) else {
            return Vec::new();
        };

        let mut groups: Vec<TagGroup> = target
            .tags
            .iter()
            .map(|tag| {
                let documents = index
                    .iter()
                    .filter(|doc| doc.path != target_path)
                    .filter(|doc| doc.tags.iter().any(|other| other.eq_ignore_ascii_case(tag)))
                    .map(|doc| (doc.path.clone(), doc.title.clone()))
                    .collect();
                TagGroup {
                    tag: tag.clone(),
                    documents,
                }
            })
            .collect();
        groups.sort_by_key(|group| group.tag.to_lowercase());
        groups
    }

    /// Every document in the project carrying `tag` (case-insensitive match
    /// against both frontmatter and inline tags), sorted by title —
    /// vault-wide tag search, independent of whatever document (if any) is
    /// currently open.
    pub fn documents_with_tag(&self, tag: &str) -> Vec<(PathBuf, String)> {
        let mut matches: Vec<(PathBuf, String)> = self
            .tag_index()
            .into_iter()
            .filter(|doc| doc.tags.iter().any(|other| other.eq_ignore_ascii_case(tag)))
            .map(|doc| (doc.path, doc.title))
            .collect();
        matches.sort_by_key(|(_, title)| title.to_lowercase());
        matches
    }

    /// The role assigned to the folder at `path`, if any.
    pub fn folder_role(&self, path: &Path) -> Option<FolderRole> {
        self.meta
            .folder_roles
            .get(&relative_key(&self.root, path))
            .copied()
    }

    /// Assign `role` to the folder at `path` (`None` clears it). At most one folder
    /// project-wide holds a given role — assigning it here clears it from wherever it
    /// was previously assigned, mirroring Scrivener's singular Draft/Trash. Errors if
    /// `path` isn't a directory.
    pub fn set_folder_role(&mut self, path: &Path, role: Option<FolderRole>) -> io::Result<()> {
        if !path.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a folder"));
        }
        let key = relative_key(&self.root, path);
        match role {
            Some(role) => {
                self.meta.folder_roles.retain(|_, r| *r != role);
                self.meta.folder_roles.insert(key, role);
            }
            None => {
                self.meta.folder_roles.remove(&key);
            }
        }
        self.save_metadata()
    }

    /// Ensure a folder holding `role` exists, independently of any other role.
    /// If a folder is already assigned `role` but has been deleted from disk since
    /// (e.g. outside the app), recreates it at that same path, keeping the existing
    /// assignment. If no folder holds `role` at all, creates a new one named
    /// `default_name` at the project root (disambiguated via `unique_child_name` if
    /// that name's already taken there) and assigns it. A no-op if the assigned
    /// folder already exists.
    pub fn ensure_role_folder(&mut self, role: FolderRole, default_name: &str) -> io::Result<()> {
        let existing = self
            .meta
            .folder_roles
            .iter()
            .find(|(_, r)| **r == role)
            .map(|(key, _)| key.clone());

        if let Some(key) = existing {
            let path = if key.is_empty() {
                self.root.clone()
            } else {
                self.root.join(&key)
            };
            if !path.is_dir() {
                fs::create_dir_all(&path)?;
                self.rescan();
            }
            return Ok(());
        }

        let root = self.root.clone();
        let name = unique_child_name(&root, default_name);
        let path = self.create_folder(&root, &name)?;
        self.set_folder_role(&path, Some(role))
    }

    /// The absolute path of the project's designated Trash folder, if any. Visible
    /// within the crate (not just this module) so `project_template::save_from_project`
    /// can exclude Trash's contents when saving a project's structure as a template.
    pub(crate) fn trash_path(&self) -> Option<PathBuf> {
        self.meta
            .folder_roles
            .iter()
            .find(|(_, role)| **role == FolderRole::Trash)
            .map(|(key, _)| {
                if key.is_empty() {
                    self.root.clone()
                } else {
                    self.root.join(key)
                }
            })
    }

    /// Whether deleting `path` right now would route it into Trash rather than
    /// permanently removing it — exposed so callers can word a delete confirmation
    /// accurately ("Move to Trash?" vs "This cannot be undone.").
    pub fn deletes_to_trash(&self, path: &Path) -> bool {
        self.trash_path()
            .is_some_and(|trash| path != trash && !path.starts_with(&trash))
    }

    pub fn rescan(&mut self) {
        let mut tree = scan_project(&self.root);
        apply_order(&mut tree.root, &self.root, &self.meta.node_order);
        self.tree = tree;
    }

    pub fn save_metadata(&self) -> io::Result<()> {
        save_metadata(&self.root, &self.meta)
    }

    /// Turn git support on for this project and record that the user's been asked
    /// (so the one-time "enable git support?" dialog never asks again).
    pub fn enable_git_support(&mut self) -> io::Result<()> {
        self.meta.git_enabled = true;
        self.meta.git_prompted = true;
        self.save_metadata()
    }

    /// Record that the user declined the one-time "enable git support?" dialog,
    /// without turning `git_enabled` on — they can still do so later via the
    /// Versions menu's "Enable Git Support" item.
    pub fn decline_git_support(&mut self) -> io::Result<()> {
        self.meta.git_prompted = true;
        self.save_metadata()
    }

    /// Turn this project's own `.smaragd/plugins/*.rhai` on or off (the global
    /// plugin directory always loads regardless — see `plugins_enabled`'s doc
    /// comment). No "prompted" flag to go with it, unlike git support: there's no
    /// auto-detection to avoid re-asking about, just an explicit menu action.
    pub fn set_plugins_enabled(&mut self, enabled: bool) -> io::Result<()> {
        self.meta.plugins_enabled = enabled;
        self.save_metadata()
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

    /// Set the book-level title/author/typesetting-style shown in the Export
    /// dialog — see `ProjectMeta::book_title`/`book_author`/`book_style`. An
    /// empty title/author is stored as `None` rather than `Some(String::new())`.
    pub fn set_book_meta(
        &mut self,
        title: String,
        author: String,
        style_id: String,
    ) -> io::Result<()> {
        self.meta.book_title = (!title.is_empty()).then_some(title);
        self.meta.book_author = (!author.is_empty()).then_some(author);
        self.meta.book_style = (!style_id.is_empty()).then_some(style_id);
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

    /// Create a new empty markdown document under `parent` (a folder within this
    /// project), record it at the end of that folder's manual order, and rescan.
    pub fn create_document(&mut self, parent: &Path, filename: &str) -> io::Result<PathBuf> {
        self.write_new_document(parent, filename, "")
    }

    /// Create a new markdown document under `parent` whose initial content is
    /// `template_path`'s (frontmatter included) with `${{name}}`/`${{date}}`
    /// substituted (see `crate::templates::substitute`) — Scrivener-style "New
    /// From Template". `template_path` itself is left untouched. `date_format` is
    /// `Settings::template_date_format`, threaded through rather than read
    /// directly since `Project` has no access to app-wide `Settings`. Goes through
    /// the same name-validation and collision-refusal path as `create_document`.
    pub fn create_document_from_template(
        &mut self,
        parent: &Path,
        filename: &str,
        template_path: &Path,
        date_format: &str,
    ) -> io::Result<PathBuf> {
        let contents = fs::read_to_string(template_path)?;
        let name = filename.strip_suffix(".md").unwrap_or(filename);
        let contents = crate::templates::substitute(&contents, name, date_format);
        self.write_new_document(parent, filename, &contents)
    }

    /// Create a new document under `parent` with `contents` written verbatim — no
    /// `${{name}}`/`${{date}}` substitution, unlike `create_document_from_template`
    /// (that's for *stationery*-style "New From Template"). This is the write path
    /// `project_template::ProjectTemplate::apply` uses to stamp a project-scaffolding
    /// template's literal starter content onto a freshly initialized project. Same
    /// name-validation/collision-refusal path as `create_document`.
    pub fn create_document_with_content(
        &mut self,
        parent: &Path,
        filename: &str,
        contents: &str,
    ) -> io::Result<PathBuf> {
        self.write_new_document(parent, filename, contents)
    }

    fn write_new_document(
        &mut self,
        parent: &Path,
        filename: &str,
        contents: &str,
    ) -> io::Result<PathBuf> {
        let filename = ensure_md_extension(filename);
        ensure_simple_child_name(&filename)?;
        let path = parent.join(&filename);
        ensure_does_not_exist(&path)?;
        fs::write(&path, contents)?;
        self.record_new_child(parent, &filename)?;
        self.rescan();
        Ok(path)
    }

    /// The documents (folders excluded) directly inside the project's designated
    /// Templates folder, if any — the candidate list for "New From Template". Not
    /// recursive: a template in a subfolder of the Templates folder isn't listed.
    pub fn template_documents(&self) -> Vec<&BinderNode> {
        let Some(path) = self.templates_path() else {
            return Vec::new();
        };
        let Some(folder) = self.tree.find_by_path(&path) else {
            return Vec::new();
        };
        folder
            .children()
            .iter()
            .filter(|child| matches!(child.kind, BinderNodeKind::Document))
            .collect()
    }

    /// The absolute path of the project's designated Templates folder, if any.
    fn templates_path(&self) -> Option<PathBuf> {
        self.meta
            .folder_roles
            .iter()
            .find(|(_, role)| **role == FolderRole::Templates)
            .map(|(key, _)| {
                if key.is_empty() {
                    self.root.clone()
                } else {
                    self.root.join(key)
                }
            })
    }

    /// The relative key (as persisted in `ProjectMeta::{type,pov,status}_picklist_folder`)
    /// of `field`'s currently assigned picklist folder, if any.
    fn picklist_folder_key(&self, field: PicklistField) -> Option<&str> {
        match field {
            PicklistField::Type => self.meta.type_picklist_folder.as_deref(),
            PicklistField::Pov => self.meta.pov_picklist_folder.as_deref(),
            PicklistField::Status => self.meta.status_picklist_folder.as_deref(),
        }
    }

    /// The documents (folders excluded) directly inside `field`'s assigned picklist
    /// folder, if any — the dropdown options for that metadata field in the Metadata
    /// panel (each document's title is one option). Not recursive: a document in a
    /// subfolder of the picklist folder isn't listed. Mirrors
    /// [`Project::template_documents`] exactly, but resolves its folder from
    /// `PicklistField`'s own dedicated slot rather than a shared [`FolderRole`] map.
    pub fn picklist_documents(&self, field: PicklistField) -> Vec<&BinderNode> {
        let Some(key) = self.picklist_folder_key(field) else {
            return Vec::new();
        };
        let path = if key.is_empty() {
            self.root.clone()
        } else {
            self.root.join(key)
        };
        let Some(folder) = self.tree.find_by_path(&path) else {
            return Vec::new();
        };
        folder
            .children()
            .iter()
            .filter(|child| matches!(child.kind, BinderNodeKind::Document))
            .collect()
    }

    /// Whether `path` is currently `field`'s assigned picklist folder — drives the
    /// binder's "Dropdown Source" checkboxes.
    pub fn is_picklist_folder(&self, field: PicklistField, path: &Path) -> bool {
        self.picklist_folder_key(field) == Some(relative_key(&self.root, path).as_str())
    }

    /// Assign `path` as `field`'s picklist folder (`None` clears it). Unlike
    /// [`Project::set_folder_role`], there's no cross-field exclusivity to enforce —
    /// each field is its own independent slot on `ProjectMeta`, so setting `Type`'s
    /// folder can never disturb `Pov`'s or `Status`'s. Errors if `path` isn't a
    /// directory.
    pub fn set_picklist_folder(
        &mut self,
        field: PicklistField,
        path: Option<&Path>,
    ) -> io::Result<()> {
        if let Some(path) = path
            && !path.is_dir()
        {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a folder"));
        }
        let key = path.map(|path| relative_key(&self.root, path));
        match field {
            PicklistField::Type => self.meta.type_picklist_folder = key,
            PicklistField::Pov => self.meta.pov_picklist_folder = key,
            PicklistField::Status => self.meta.status_picklist_folder = key,
        }
        self.save_metadata()
    }

    /// Create a new empty folder under `parent`, record it, and rescan. Refuses to
    /// overwrite an existing file or folder at the destination.
    pub fn create_folder(&mut self, parent: &Path, name: &str) -> io::Result<PathBuf> {
        ensure_simple_child_name(name)?;
        let path = parent.join(name);
        ensure_does_not_exist(&path)?;
        fs::create_dir_all(&path)?;
        self.record_new_child(parent, name)?;
        self.rescan();
        Ok(path)
    }

    fn record_new_child(&mut self, parent: &Path, name: &str) -> io::Result<()> {
        let key = relative_key(&self.root, parent);
        self.meta
            .node_order
            .entry(key)
            .or_default()
            .push(name.to_string());
        self.save_metadata()
    }

    /// Rename the file or folder at `path` to `new_name` (a document keeps its `.md`
    /// extension; a folder is renamed as-is), updating manual ordering and rescanning.
    /// Refuses to overwrite an existing file or folder at the destination.
    pub fn rename(&mut self, path: &Path, new_name: &str) -> io::Result<PathBuf> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
        let is_dir = path.is_dir();
        let new_name = if is_dir {
            new_name.to_string()
        } else {
            ensure_md_extension(new_name)
        };
        ensure_simple_child_name(&new_name)?;
        let new_path = parent.join(&new_name);
        ensure_does_not_exist(&new_path)?;

        fs::rename(path, &new_path)?;

        if let Some(old_name) = path.file_name().and_then(|n| n.to_str()) {
            let parent_key = relative_key(&self.root, parent);
            if let Some(order) = self.meta.node_order.get_mut(&parent_key) {
                for entry in order.iter_mut() {
                    if entry == old_name {
                        *entry = new_name.clone();
                    }
                }
            }
        }
        if is_dir {
            self.rewrite_relative_key_prefix(
                &relative_key(&self.root, path),
                &relative_key(&self.root, &new_path),
            );
        }

        self.save_metadata()?;
        self.rescan();

        // A folder rename can't affect wikilinks — they resolve by document filename,
        // not folder name — but a document rename might be the target of links
        // elsewhere in the project, so those need to follow it to the new name.
        if !is_dir
            && let (Some(old_stem), Some(new_stem)) = (
                path.file_stem().and_then(|s| s.to_str()),
                new_path.file_stem().and_then(|s| s.to_str()),
            )
        {
            self.rename_wikilinks_everywhere(old_stem, new_stem)?;
            self.relink_story_cards(old_stem, new_stem)?;
        }

        Ok(new_path)
    }

    /// Rewrite `[[old_target]]` / `[[old_target|Alias]]` wikilinks to `new_target` in
    /// every document in the project (including, if applicable, the renamed document
    /// itself, in case it links to itself).
    fn rename_wikilinks_everywhere(&self, old_target: &str, new_target: &str) -> io::Result<()> {
        for doc_path in self.tree.document_paths() {
            let contents = fs::read_to_string(&doc_path)?;
            if let Some(updated) =
                crate::markdown::rename_wikilink_target(&contents, old_target, new_target)
            {
                fs::write(&doc_path, updated)?;
            }
        }
        Ok(())
    }

    /// Follow a document rename in any story card linked to it by its old stem — the
    /// same "keep soft references working" job `rename_wikilinks_everywhere` does for
    /// `[[wikilinks]]`, just for `StoryCard::linked_document_stem`. A no-op (and no
    /// extra write) if no card was linked to `old_stem`.
    fn relink_story_cards(&mut self, old_stem: &str, new_stem: &str) -> io::Result<()> {
        let mut changed = false;
        for card in self.meta.story_cards.iter_mut() {
            if card.linked_document_stem.as_deref() == Some(old_stem) {
                card.linked_document_stem = Some(new_stem.to_string());
                changed = true;
            }
        }
        if changed {
            self.save_metadata()
        } else {
            Ok(())
        }
    }

    /// Delete `path`. If a Trash folder is designated and `path` isn't already inside
    /// it (and isn't the Trash folder itself), moves it into Trash instead of
    /// removing it from disk — see [`Project::move_to_trash`]. Otherwise permanently
    /// removes it, as if there were no Trash configured.
    pub fn delete(&mut self, path: &Path) -> io::Result<()> {
        if self.deletes_to_trash(path) {
            let trash = self.trash_path().expect("checked by deletes_to_trash");
            return self.move_to_trash(path, &trash);
        }
        self.permanently_delete(path)
    }

    /// Move the file or folder at `path` into `new_parent` — e.g. dragging and
    /// dropping it onto a different folder in the binder. Unlike `move_node` (used
    /// for trashing/restoring, where silently disambiguating a same-named collision
    /// is the right call), this *refuses* the move if `new_parent` already has an
    /// entry with that name: a drag-and-drop is a deliberate placement, so quietly
    /// renaming around a collision would be surprising. Also refuses moving a folder
    /// into itself or one of its own subfolders, which `fs::rename` would otherwise
    /// reject with a much less legible OS error.
    ///
    /// Dropping an item onto its *own* parent folder's header (as opposed to a
    /// sibling document row — see `move_item_before` for that) is handled as a
    /// special case: there's no actual filesystem move to make, just a
    /// reposition to the end of that folder's order, since `move_node_with`'s
    /// `NameCollision::Refuse` would otherwise reject this as a collision with
    /// the item's own still-existing path.
    pub fn move_item(&mut self, path: &Path, new_parent: &Path) -> io::Result<PathBuf> {
        if new_parent.starts_with(path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "can't move a folder into itself or one of its own subfolders",
            ));
        }
        let dest = if path.parent() == Some(new_parent) {
            let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "path has no file name")
            })?;
            self.reposition_in_order(new_parent, name, None);
            path.to_path_buf()
        } else {
            self.move_node_with(path, new_parent, NameCollision::Refuse)?
        };
        self.save_metadata()?;
        self.rescan();
        Ok(dest)
    }

    /// Move `path` (a file or folder) to sit immediately before `before` among
    /// `before`'s parent's children — used when something is dragged and dropped
    /// directly onto another document row in the binder (as opposed to a folder
    /// header, which always appends to the end — see `move_item`). Works whether
    /// `before` is currently a sibling of `path` (a pure reorder) or in a
    /// different folder (a move, positioned rather than appended). Refuses
    /// moving an item before itself, or a folder into itself/one of its own
    /// subfolders, same reasoning as `move_item`.
    pub fn move_item_before(&mut self, path: &Path, before: &Path) -> io::Result<PathBuf> {
        if path == before {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "can't move an item before itself",
            ));
        }
        let new_parent = before
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no parent"))?;
        if new_parent.starts_with(path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "can't move a folder into itself or one of its own subfolders",
            ));
        }

        let dest = if path.parent() == Some(new_parent) {
            path.to_path_buf()
        } else {
            self.move_node_with(path, new_parent, NameCollision::Refuse)?
        };

        let name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
        let before_name = before.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "target has no file name")
        })?;
        self.reposition_in_order(new_parent, name, Some(before_name));

        self.save_metadata()?;
        self.rescan();
        Ok(dest)
    }

    /// Remove `name` from `parent`'s child order and reinsert it immediately
    /// before `before` — or at the end, if `before` is `None` or not found.
    /// `before` is looked up *after* `name` is removed, so a `before` that
    /// originally sat right after `name` in the list doesn't shift by one and
    /// end up landing after it.
    ///
    /// Starts from `self.tree`'s current children for `parent` (still the
    /// *pre-move* tree at this point — callers run this before `rescan()`),
    /// not directly from `self.meta.node_order`'s entry: `node_order` is a
    /// sparse override list — anything never explicitly reordered has no entry
    /// at all, filled in by `apply_order`'s fallback (existing/alphabetical
    /// order, sorted last) rather than being absent from the *displayed* list.
    /// Computing the target index against that raw, possibly-incomplete list
    /// previously let an untracked sibling's true position throw the result
    /// off — e.g. dragging the last of 5 never-explicitly-reordered scenes
    /// onto its neighbor landed it at the very top of the chapter instead of
    /// where it was dropped, because the stored order only had one or two
    /// tracked entries and the target's position among *those* wasn't its
    /// real, currently-displayed position. `self.tree`'s children are always
    /// the complete, already-`apply_order`-resolved list, so this can't happen
    /// — and writing the full resulting list back (instead of just patching
    /// the old sparse one) also fixes `parent`'s entry going forward.
    fn reposition_in_order(&mut self, parent: &Path, name: &str, before: Option<&str>) {
        let mut order: Vec<String> = self
            .tree
            .find_by_path(parent)
            .map(|node| {
                node.children()
                    .iter()
                    .map(|child| child.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        order.retain(|entry| entry != name);
        let index = before
            .and_then(|before_name| order.iter().position(|entry| entry == before_name))
            .unwrap_or(order.len());
        order.insert(index, name.to_string());
        let parent_key = relative_key(&self.root, parent);
        self.meta.node_order.insert(parent_key, order);
    }

    /// Move `path` (already inside this project) to be a child of `new_parent`,
    /// keeping its own name unless that collides — resolved via `unique_child_name`,
    /// since two different folders can easily contain same-named files — and fixing
    /// up `node_order` — and, for a folder, nested `node_order`/`folder_roles`/
    /// `trashed_origins` keys via `rewrite_relative_key_prefix` — to follow. Does not
    /// persist/rescan; callers own that, since what else needs updating differs
    /// between trashing and restoring. `restore_from_trash` removes its own
    /// `trashed_origins` entry *before* calling this, precisely because this now
    /// touches that map too — see its comment for why the order matters.
    fn move_node(&mut self, path: &Path, new_parent: &Path) -> io::Result<PathBuf> {
        self.move_node_with(path, new_parent, NameCollision::Uniquify)
    }

    /// Shared core of `move_node`/`move_item`: rename `path` into `new_parent` and
    /// fix up `node_order` (and, for a folder, nested order keys) to follow — the
    /// only thing that differs between the two public entry points is what happens
    /// when `new_parent` already has an entry with `path`'s name, which
    /// `on_collision` controls.
    fn move_node_with(
        &mut self,
        path: &Path,
        new_parent: &Path,
        on_collision: NameCollision,
    ) -> io::Result<PathBuf> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
        let dest_name = match on_collision {
            NameCollision::Uniquify => unique_child_name(new_parent, name),
            NameCollision::Refuse => {
                ensure_does_not_exist(&new_parent.join(name))?;
                name.to_string()
            }
        };
        let dest = new_parent.join(&dest_name);
        let is_dir = path.is_dir();

        fs::rename(path, &dest)?;

        if let Some(parent) = path.parent() {
            let parent_key = relative_key(&self.root, parent);
            if let Some(order) = self.meta.node_order.get_mut(&parent_key) {
                order.retain(|entry| entry != name);
            }
        }
        if is_dir {
            self.rewrite_relative_key_prefix(
                &relative_key(&self.root, path),
                &relative_key(&self.root, &dest),
            );
        }
        // Appends `dest_name` after every one of `new_parent`'s *current*
        // children — tracked or not — rather than just whatever was already in
        // its (possibly sparse) `node_order` entry; see `reposition_in_order`'s
        // doc comment for why that distinction matters.
        self.reposition_in_order(new_parent, &dest_name, None);
        Ok(dest)
    }

    /// Move `path` into the designated `trash` folder. Records the item's original
    /// relative path in `meta.trashed_origins`, keyed by its new (post-move) relative
    /// path — this is what disambiguates same-named trashed items and is what
    /// `restore_from_trash` needs, since the on-disk name alone doesn't carry it.
    fn move_to_trash(&mut self, path: &Path, trash: &Path) -> io::Result<()> {
        let original_key = relative_key(&self.root, path);
        let dest = self.move_node(path, trash)?;
        self.meta
            .trashed_origins
            .insert(relative_key(&self.root, &dest), original_key);

        self.save_metadata()?;
        self.rescan();
        Ok(())
    }

    /// The absolute path this trashed item originally lived at, if `path` is
    /// currently a top-level item inside the designated Trash folder that was moved
    /// there via `delete` (as opposed to something nested inside a trashed folder, or
    /// created directly inside Trash — those were never individually recorded and
    /// aren't restorable on their own). Doubles as the "can this be restored?" check.
    pub fn trashed_origin(&self, path: &Path) -> Option<PathBuf> {
        self.meta
            .trashed_origins
            .get(&relative_key(&self.root, path))
            .map(|key| self.root.join(key))
    }

    /// Move a trashed item (see [`Project::trashed_origin`]) back to the folder it
    /// was deleted from, resolving a name collision the same way `move_to_trash`
    /// does. If that folder no longer exists: errors with
    /// `RestoreError::OriginalFolderMissing` when `recreate_missing_folder` is
    /// `false`, or recreates it and proceeds when `true` — callers offer that choice
    /// interactively rather than picking one silently.
    pub fn restore_from_trash(
        &mut self,
        path: &Path,
        recreate_missing_folder: bool,
    ) -> Result<PathBuf, RestoreError> {
        let key = relative_key(&self.root, path);
        let Some(original) = self
            .meta
            .trashed_origins
            .get(&key)
            .cloned()
            .map(|k| self.root.join(k))
        else {
            return Err(RestoreError::NotTrashed);
        };
        let original_parent = original.parent().unwrap_or(&self.root).to_path_buf();
        if !original_parent.is_dir() {
            if recreate_missing_folder {
                fs::create_dir_all(&original_parent)?;
            } else {
                return Err(RestoreError::OriginalFolderMissing(original_parent));
            }
        }
        // Remove this item's own trashed_origins entry *before* move_node runs, not
        // after: move_node (via rewrite_relative_key_prefix, for a folder) now also
        // follows folder_roles/trashed_origins keys under the moved prefix, which for
        // a restored folder would otherwise race with this very removal — the rewrite
        // would relocate the entry to the new key first, leaving nothing for this
        // line to find, and stranding a stale, self-referential entry behind instead
        // of clearing it.
        self.meta.trashed_origins.remove(&key);
        let dest = self.move_node(path, &original_parent)?;
        self.save_metadata()?;
        self.rescan();
        Ok(dest)
    }

    /// Permanently remove everything currently inside the designated Trash folder. A
    /// no-op if no Trash folder is designated or it's already empty.
    pub fn empty_trash(&mut self) -> io::Result<()> {
        let Some(trash) = self.trash_path() else {
            return Ok(());
        };
        let Some(node) = self.tree.find_by_path(&trash) else {
            return Ok(());
        };
        let children: Vec<PathBuf> = node.children().iter().map(|c| c.path.clone()).collect();
        for child in children {
            self.permanently_delete(&child)?;
        }
        Ok(())
    }

    /// Delete the file or folder at `path` (a folder is removed recursively), drop it
    /// (and, for a folder, its descendants) from manual ordering and any assigned
    /// folder role / trashed-origin record, and rescan. Bypasses Trash entirely —
    /// used both when no Trash is configured and to actually clear something out of
    /// Trash (via [`Project::delete`]'s routing, or [`Project::empty_trash`]).
    fn permanently_delete(&mut self, path: &Path) -> io::Result<()> {
        let is_dir = path.is_dir();
        if is_dir {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }

        if let Some(parent) = path.parent()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
        {
            let parent_key = relative_key(&self.root, parent);
            if let Some(order) = self.meta.node_order.get_mut(&parent_key) {
                order.retain(|entry| entry != name);
            }
        }
        let key = relative_key(&self.root, path);
        if is_dir {
            let prefix = key;
            let under_prefix = |k: &String| *k == prefix || k.starts_with(&format!("{prefix}/"));
            self.meta.node_order.retain(|k, _| !under_prefix(k));
            self.meta.folder_roles.retain(|k, _| !under_prefix(k));
            self.meta.trashed_origins.retain(|k, _| !under_prefix(k));
        } else {
            self.meta.folder_roles.remove(&key);
            self.meta.trashed_origins.remove(&key);
        }

        self.save_metadata()?;
        self.rescan();
        Ok(())
    }

    /// Rewrite every `node_order`/`folder_roles`/`trashed_origins` key under
    /// `old_prefix` (the folder itself and all its descendants) to sit under
    /// `new_prefix` instead, following a folder rename or move — so a role (Research/
    /// Trash) assigned to the moved folder or something inside it, or a trashed
    /// item's current-location bookkeeping, keeps pointing at where the thing
    /// actually is instead of a now-nonexistent path. `trashed_origins`' *values*
    /// (each item's pre-trash location) are deliberately left untouched: they're
    /// history, not a reference to something that just moved.
    fn rewrite_relative_key_prefix(&mut self, old_prefix: &str, new_prefix: &str) {
        rewrite_prefix_in(&mut self.meta.node_order, old_prefix, new_prefix);
        rewrite_prefix_in(&mut self.meta.folder_roles, old_prefix, new_prefix);
        rewrite_prefix_in(&mut self.meta.trashed_origins, old_prefix, new_prefix);
    }
}

/// Rewrite every key under `old_prefix` (matching it exactly, or starting with
/// `"{old_prefix}/"`) in `map` to sit under `new_prefix` instead, preserving each
/// key's value and its position relative to the prefix.
fn rewrite_prefix_in<V>(map: &mut HashMap<String, V>, old_prefix: &str, new_prefix: &str) {
    let affected: Vec<String> = map
        .keys()
        .filter(|key| *key == old_prefix || key.starts_with(&format!("{old_prefix}/")))
        .cloned()
        .collect();
    for key in affected {
        if let Some(value) = map.remove(&key) {
            let new_key = format!("{new_prefix}{}", &key[old_prefix.len()..]);
            map.insert(new_key, value);
        }
    }
}

/// Guard against silently clobbering an existing file or folder — used before any
/// operation (create, rename) that's about to write to a new destination path.
/// Reject anything but a single, ordinary path component: no `/`/`\` separators, no
/// `.`/`..`, not empty, not an absolute path. `create_document`/`create_folder`/
/// `rename` all join a user- or document-supplied name (e.g. a wikilink target,
/// which can be aliased so the text a user sees doesn't match it) directly onto a
/// known-good parent directory — without this check, a name like `../../evil` or an
/// absolute path would let `Path::join` escape (or entirely replace) that parent,
/// writing outside the project.
fn ensure_simple_child_name(name: &str) -> io::Result<()> {
    let mut components = Path::new(name).components();
    let is_single_normal_component = matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    );
    if is_single_normal_component {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("\"{name}\" isn't a valid file or folder name"),
        ))
    }
}

fn ensure_does_not_exist(path: &Path) -> io::Result<()> {
    if path.exists() {
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", path.display()),
        ))
    } else {
        Ok(())
    }
}

fn ensure_md_extension(filename: &str) -> String {
    if filename.ends_with(".md") {
        filename.to_string()
    } else {
        format!("{filename}.md")
    }
}

/// Resolve a name collision under `parent` by appending " (2)", " (3)", ... before the
/// extension (or at the end, for an extension-less name/a folder) until it no longer
/// exists on disk. Returns `desired` unchanged if it's already free.
fn unique_child_name(parent: &Path, desired: &str) -> String {
    if !parent.join(desired).exists() {
        return desired.to_string();
    }
    let (stem, ext) = match desired.rsplit_once('.') {
        Some((stem, ext)) => (stem, format!(".{ext}")),
        None => (desired, String::new()),
    };
    for n in 2.. {
        let candidate = format!("{stem} ({n}){ext}");
        if !parent.join(&candidate).exists() {
            return candidate;
        }
    }
    unreachable!()
}

/// Convert an absolute path within the project into the `/`-separated relative key
/// used by `ProjectMeta::node_order`. The project root itself maps to `""`.
fn relative_key(root: &Path, path: &Path) -> String {
    if path == root {
        return String::new();
    }
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Reorder each folder's children to match any recorded order for that folder,
/// appending children with no recorded position (e.g. newly discovered files never
/// created through smaragd) at the end in their existing (alphabetical) order.
fn apply_order(node: &mut BinderNode, root: &Path, order: &HashMap<String, Vec<String>>) {
    if let BinderNodeKind::Folder { children } = &mut node.kind {
        if let Some(order_list) = order.get(&relative_key(root, &node.path)) {
            children.sort_by_key(|child| {
                order_list
                    .iter()
                    .position(|name| name == &child.name)
                    .unwrap_or(usize::MAX)
            });
        }
        for child in children.iter_mut() {
            apply_order(child, root, order);
        }
    }
}

fn metadata_path(root: &Path) -> PathBuf {
    root.join(METADATA_DIR).join(METADATA_FILE)
}

fn load_metadata(root: &Path) -> Option<ProjectMeta> {
    let contents = fs::read_to_string(metadata_path(root)).ok()?;
    serde_json::from_str(&contents).ok()
}

fn save_metadata(root: &Path, meta: &ProjectMeta) -> io::Result<()> {
    let dir = root.join(METADATA_DIR);
    fs::create_dir_all(&dir)?;
    let contents = serde_json::to_string_pretty(meta)?;
    fs::write(metadata_path(root), contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut node_order = HashMap::new();
        node_order.insert(
            "Chapter 1".to_string(),
            vec!["01-opening.md".to_string(), "02-arrival.md".to_string()],
        );
        let meta = ProjectMeta {
            version: 1,
            node_order,
            ..Default::default()
        };

        save_metadata(dir.path(), &meta).unwrap();
        let loaded = load_metadata(dir.path()).unwrap();

        assert_eq!(loaded, meta);
    }

    #[test]
    fn missing_metadata_file_falls_back_to_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_metadata(dir.path()).is_none());
    }

    #[test]
    fn corrupt_metadata_file_falls_back_to_none_instead_of_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let meta_dir = dir.path().join(METADATA_DIR);
        fs::create_dir_all(&meta_dir).unwrap();
        fs::write(meta_dir.join(METADATA_FILE), "{ this is not valid json").unwrap();

        assert!(load_metadata(dir.path()).is_none());
    }

    #[test]
    fn load_from_folder_errors_when_project_has_no_marker() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.md"), "hello").unwrap();

        let result = Project::load_from_folder(dir.path());

        assert!(matches!(result, Err(LoadError::NotInitialized(_))));
    }

    #[test]
    fn load_from_folder_errors_on_nonexistent_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");

        assert!(matches!(
            Project::load_from_folder(&missing),
            Err(LoadError::NotADirectory(_))
        ));
    }

    #[test]
    fn initialize_lets_a_pre_existing_folder_of_markdown_files_load_with_default_metadata() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.md"), "hello").unwrap();

        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(project.meta, ProjectMeta::default());
        assert!(
            project
                .tree
                .find_by_path(&dir.path().join("notes.md"))
                .is_some()
        );
    }

    #[test]
    fn initialize_creates_the_folder_and_marker_for_a_brand_new_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("New Project");

        let project = Project::initialize(&root).unwrap();

        assert!(root.is_dir());
        assert!(root.join(METADATA_DIR).join(METADATA_FILE).is_file());
        assert!(project.tree.root.children().is_empty());
    }

    #[test]
    fn initialize_does_not_clobber_existing_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let mut node_order = HashMap::new();
        node_order.insert("".to_string(), vec!["custom.md".to_string()]);
        let meta = ProjectMeta {
            version: 7,
            node_order,
            ..Default::default()
        };
        save_metadata(dir.path(), &meta).unwrap();

        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(project.meta, meta);
    }

    #[test]
    fn document_meta_reads_frontmatter_from_a_project_document() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Scene 1").unwrap();
        fs::write(&path, "---\ntype: Scene\npov: Alice\n---\nBody.\n").unwrap();

        let meta = project.document_meta(&path).unwrap();

        assert_eq!(meta.section_type.as_deref(), Some("Scene"));
        assert_eq!(meta.pov.as_deref(), Some("Alice"));
    }

    #[test]
    fn document_meta_returns_default_for_a_document_with_no_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Plain").unwrap();

        let meta = project.document_meta(&path).unwrap();

        assert_eq!(meta, crate::frontmatter::DocumentMeta::default());
    }

    #[test]
    fn document_meta_errors_if_the_path_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        let result = project.document_meta(&dir.path().join("missing.md"));

        assert!(result.is_err());
    }

    #[test]
    fn load_metadata_defaults_folder_roles_and_trashed_origins_when_absent_from_older_project_json()
    {
        let dir = tempfile::tempdir().unwrap();
        let meta_dir = dir.path().join(METADATA_DIR);
        fs::create_dir_all(&meta_dir).unwrap();
        fs::write(
            meta_dir.join(METADATA_FILE),
            r#"{"version":1,"node_order":{"":["a.md"]}}"#,
        )
        .unwrap();

        let meta = load_metadata(dir.path()).unwrap();

        assert_eq!(meta.node_order.get(""), Some(&vec!["a.md".to_string()]));
        assert!(meta.folder_roles.is_empty());
        assert!(meta.trashed_origins.is_empty());
        assert!(meta.story_cards.is_empty());
        assert!(!meta.git_enabled);
        assert!(!meta.git_prompted);
        assert!(!meta.plugins_enabled);
        assert_eq!(meta.type_picklist_folder, None);
        assert_eq!(meta.pov_picklist_folder, None);
        assert_eq!(meta.status_picklist_folder, None);
    }

    #[test]
    fn set_plugins_enabled_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        assert!(!project.meta.plugins_enabled);

        project.set_plugins_enabled(true).unwrap();
        assert!(project.meta.plugins_enabled);
        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert!(reloaded.meta.plugins_enabled);

        project.set_plugins_enabled(false).unwrap();
        assert!(!project.meta.plugins_enabled);
        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert!(!reloaded.meta.plugins_enabled);
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
    fn set_book_meta_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        assert_eq!(project.meta.book_title, None);
        assert_eq!(project.meta.book_author, None);
        assert_eq!(project.meta.book_style, None);

        project
            .set_book_meta(
                "My Book".to_string(),
                "Jane Doe".to_string(),
                "trade_paperback".to_string(),
            )
            .unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(reloaded.meta.book_title, Some("My Book".to_string()));
        assert_eq!(reloaded.meta.book_author, Some("Jane Doe".to_string()));
        assert_eq!(
            reloaded.meta.book_style,
            Some("trade_paperback".to_string())
        );
    }

    #[test]
    fn set_book_meta_with_empty_fields_stores_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project
            .set_book_meta(
                "My Book".to_string(),
                "Jane Doe".to_string(),
                "trade_paperback".to_string(),
            )
            .unwrap();
        project
            .set_book_meta(String::new(), String::new(), String::new())
            .unwrap();

        assert_eq!(project.meta.book_title, None);
        assert_eq!(project.meta.book_author, None);
        assert_eq!(project.meta.book_style, None);
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
    fn enable_git_support_sets_both_flags_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project.enable_git_support().unwrap();

        assert!(project.meta.git_enabled);
        assert!(project.meta.git_prompted);
        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert!(reloaded.meta.git_enabled);
        assert!(reloaded.meta.git_prompted);
    }

    #[test]
    fn decline_git_support_sets_prompted_but_not_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project.decline_git_support().unwrap();

        assert!(!project.meta.git_enabled);
        assert!(project.meta.git_prompted);
    }

    #[test]
    fn set_folder_role_assigns_and_enforces_single_holder_per_role() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let a = project.create_folder(dir.path(), "A").unwrap();
        let b = project.create_folder(dir.path(), "B").unwrap();

        project
            .set_folder_role(&a, Some(FolderRole::Trash))
            .unwrap();
        assert_eq!(project.folder_role(&a), Some(FolderRole::Trash));

        project
            .set_folder_role(&b, Some(FolderRole::Trash))
            .unwrap();
        assert_eq!(project.folder_role(&b), Some(FolderRole::Trash));
        assert_eq!(project.folder_role(&a), None);
    }

    #[test]
    fn set_folder_role_clears_with_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let a = project.create_folder(dir.path(), "A").unwrap();
        project
            .set_folder_role(&a, Some(FolderRole::Research))
            .unwrap();

        project.set_folder_role(&a, None).unwrap();

        assert_eq!(project.folder_role(&a), None);
    }

    #[test]
    fn set_folder_role_errors_for_a_document_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();

        let result = project.set_folder_role(&doc, Some(FolderRole::Research));

        assert!(result.is_err());
    }

    #[test]
    fn ensure_role_folder_creates_and_assigns_when_no_folder_holds_the_role() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project
            .ensure_role_folder(FolderRole::Research, "Research")
            .unwrap();

        let path = dir.path().join("Research");
        assert!(path.is_dir());
        assert_eq!(project.folder_role(&path), Some(FolderRole::Research));
    }

    #[test]
    fn ensure_role_folder_uniquifies_the_default_name_if_already_taken() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.create_folder(dir.path(), "Research").unwrap();

        project
            .ensure_role_folder(FolderRole::Research, "Research")
            .unwrap();

        let path = dir.path().join("Research (2)");
        assert!(path.is_dir());
        assert_eq!(project.folder_role(&path), Some(FolderRole::Research));
    }

    #[test]
    fn ensure_role_folder_is_a_no_op_when_the_assigned_folder_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();

        project
            .ensure_role_folder(FolderRole::Trash, "Trash")
            .unwrap();

        assert!(
            !dir.path().join("Trash (2)").exists(),
            "should not have created a second Trash-like folder"
        );
        assert_eq!(project.folder_role(&trash), Some(FolderRole::Trash));
    }

    #[test]
    fn ensure_role_folder_recreates_a_missing_assigned_folder_at_its_original_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        fs::remove_dir(&trash).unwrap();

        project
            .ensure_role_folder(FolderRole::Trash, "Trash")
            .unwrap();

        assert!(trash.is_dir());
        assert_eq!(project.folder_role(&trash), Some(FolderRole::Trash));
    }

    #[test]
    fn ensure_role_folder_treats_research_and_trash_independently() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();

        project
            .ensure_role_folder(FolderRole::Research, "Research")
            .unwrap();

        assert!(dir.path().join("Research").is_dir());
        assert_eq!(project.folder_role(&trash), Some(FolderRole::Trash));
        assert_eq!(
            project.folder_role(&dir.path().join("Research")),
            Some(FolderRole::Research)
        );
    }

    #[test]
    fn folder_role_returns_none_when_unassigned() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(project.folder_role(dir.path()), None);
    }

    #[test]
    fn deletes_to_trash_reports_correctly_for_configured_vs_unconfigured_trash() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();

        assert!(!project.deletes_to_trash(&doc));

        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();

        assert!(project.deletes_to_trash(&doc));
        assert!(!project.deletes_to_trash(&trash));
    }

    #[test]
    fn delete_moves_into_designated_trash_folder_instead_of_removing_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let doc = project.create_document(dir.path(), "Doomed").unwrap();

        project.delete(&doc).unwrap();

        assert!(!doc.exists());
        let expected = trash.join("Doomed.md");
        assert!(expected.exists());
        assert!(project.tree.find_by_path(&expected).is_some());
    }

    #[test]
    fn delete_of_the_trash_folder_itself_permanently_removes_it_and_clears_the_role() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();

        project.delete(&trash).unwrap();

        assert!(!trash.exists());
        assert_eq!(project.folder_role(&trash), None);
    }

    #[test]
    fn delete_of_an_item_already_inside_trash_permanently_removes_it_instead_of_re_trashing() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let doc = project.create_document(&trash, "Already Trashed").unwrap();

        project.delete(&doc).unwrap();

        assert!(!doc.exists());
        assert!(project.tree.find_by_path(&doc).is_none());
    }

    #[test]
    fn move_to_trash_uniquifies_a_colliding_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        fs::write(trash.join("Doomed.md"), "").unwrap();
        let doc = project.create_document(dir.path(), "Doomed").unwrap();

        project.delete(&doc).unwrap();

        assert!(trash.join("Doomed (2).md").exists());
    }

    #[test]
    fn move_to_trash_preserves_nested_order_keys_for_a_trashed_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let chapter = project.create_folder(dir.path(), "Old Chapter").unwrap();
        project.create_document(&chapter, "Scene 1").unwrap();

        project.delete(&chapter).unwrap();

        let moved = trash.join("Old Chapter");
        assert!(moved.join("Scene 1.md").exists());
        assert_eq!(
            project.meta.node_order.get("Trash/Old Chapter"),
            Some(&vec!["Scene 1.md".to_string()])
        );
        assert!(!project.meta.node_order.contains_key("Old Chapter"));
    }

    #[test]
    fn move_to_trash_records_the_original_relative_path_in_trashed_origins() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let doc = project.create_document(&chapter, "Notes").unwrap();

        project.delete(&doc).unwrap();

        assert_eq!(
            project.meta.trashed_origins.get("Trash/Notes.md"),
            Some(&"Chapter 1/Notes.md".to_string())
        );
    }

    #[test]
    fn move_to_trash_disambiguates_two_same_named_items_from_different_folders() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let chapter1 = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let chapter2 = project.create_folder(dir.path(), "Chapter 2").unwrap();
        let doc1 = project.create_document(&chapter1, "notes").unwrap();
        let doc2 = project.create_document(&chapter2, "notes").unwrap();

        project.delete(&doc1).unwrap();
        project.delete(&doc2).unwrap();

        assert!(trash.join("notes.md").exists());
        assert!(trash.join("notes (2).md").exists());
        assert_eq!(
            project.meta.trashed_origins.get("Trash/notes.md"),
            Some(&"Chapter 1/notes.md".to_string())
        );
        assert_eq!(
            project.meta.trashed_origins.get("Trash/notes (2).md"),
            Some(&"Chapter 2/notes.md".to_string())
        );
    }

    #[test]
    fn empty_trash_permanently_removes_all_trashed_items_and_their_origin_records() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let doc = project.create_document(dir.path(), "Doomed").unwrap();
        project.delete(&doc).unwrap();
        assert!(!project.meta.trashed_origins.is_empty());

        project.empty_trash().unwrap();

        assert!(!trash.join("Doomed.md").exists());
        assert!(project.meta.trashed_origins.is_empty());
        assert_eq!(project.folder_role(&trash), Some(FolderRole::Trash));
    }

    #[test]
    fn empty_trash_is_a_no_op_when_no_trash_folder_is_designated() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        assert!(project.empty_trash().is_ok());
    }

    fn project_with_trashed_doc(dir: &Path) -> (Project, PathBuf, PathBuf, PathBuf) {
        let mut project = Project::initialize(dir).unwrap();
        let trash = project.create_folder(dir, "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let chapter = project.create_folder(dir, "Chapter 1").unwrap();
        let doc = project.create_document(&chapter, "Notes").unwrap();
        project.delete(&doc).unwrap();
        let trashed = trash.join("Notes.md");
        (project, trash, chapter, trashed)
    }

    #[test]
    fn trashed_origin_returns_the_original_absolute_path_for_a_trashed_item() {
        let dir = tempfile::tempdir().unwrap();
        let (project, _trash, chapter, trashed) = project_with_trashed_doc(dir.path());

        assert_eq!(
            project.trashed_origin(&trashed),
            Some(chapter.join("Notes.md"))
        );
    }

    #[test]
    fn trashed_origin_returns_none_for_a_non_trashed_path() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(project.trashed_origin(dir.path()), None);
    }

    #[test]
    fn restore_from_trash_moves_item_back_to_its_original_folder_and_clears_the_origin_record() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, _trash, chapter, trashed) = project_with_trashed_doc(dir.path());

        let restored = project.restore_from_trash(&trashed, false).unwrap();

        assert_eq!(restored, chapter.join("Notes.md"));
        assert!(restored.exists());
        assert!(!trashed.exists());
        assert!(project.trashed_origin(&restored).is_none());
        assert!(project.tree.find_by_path(&restored).is_some());
    }

    #[test]
    fn restore_from_trash_errors_when_path_is_not_a_recorded_trashed_item() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();

        let result = project.restore_from_trash(&doc, false);

        assert!(matches!(result, Err(RestoreError::NotTrashed)));
    }

    #[test]
    fn restore_from_trash_errors_when_original_folder_is_missing_and_recreate_is_false() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, _trash, chapter, trashed) = project_with_trashed_doc(dir.path());
        project.delete(&chapter).unwrap(); // moves "Chapter 1" itself into Trash

        let result = project.restore_from_trash(&trashed, false);

        assert!(matches!(
            result,
            Err(RestoreError::OriginalFolderMissing(_))
        ));
        assert!(trashed.exists()); // left in place, not moved
    }

    #[test]
    fn restore_from_trash_recreates_the_original_folder_when_recreate_is_true() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, _trash, chapter, trashed) = project_with_trashed_doc(dir.path());
        fs::remove_dir_all(&chapter).unwrap(); // "Chapter 1" is gone, but not via project.delete

        let restored = project.restore_from_trash(&trashed, true).unwrap();

        assert!(chapter.is_dir());
        assert_eq!(restored, chapter.join("Notes.md"));
        assert!(restored.exists());
    }

    #[test]
    fn restore_from_trash_uniquifies_when_something_now_occupies_the_original_name() {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, _trash, chapter, trashed) = project_with_trashed_doc(dir.path());
        fs::write(chapter.join("Notes.md"), "new content").unwrap();

        let restored = project.restore_from_trash(&trashed, false).unwrap();

        assert_eq!(restored, chapter.join("Notes (2).md"));
        assert_eq!(
            fs::read_to_string(chapter.join("Notes.md")).unwrap(),
            "new content"
        );
    }

    #[test]
    fn restore_from_trash_of_a_folder_preserves_nested_order_keys_and_removes_trashs_order_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let old_chapter = project.create_folder(dir.path(), "Old Chapter").unwrap();
        project.create_document(&old_chapter, "Scene 1").unwrap();
        project.delete(&old_chapter).unwrap();
        let trashed = trash.join("Old Chapter");

        let restored = project.restore_from_trash(&trashed, false).unwrap();

        assert_eq!(restored, dir.path().join("Old Chapter"));
        assert!(restored.join("Scene 1.md").exists());
        assert_eq!(
            project.meta.node_order.get("Old Chapter"),
            Some(&vec!["Scene 1.md".to_string()])
        );
        assert!(!project.meta.node_order.contains_key("Trash/Old Chapter"));
        assert_eq!(project.meta.node_order.get("Trash"), Some(&vec![]));
    }

    #[test]
    fn create_document_writes_file_appends_order_and_appears_in_tree() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        let path = project.create_document(dir.path(), "New Scene").unwrap();

        assert_eq!(path, dir.path().join("New Scene.md"));
        assert!(path.exists());
        assert!(project.tree.find_by_path(&path).is_some());
        assert_eq!(
            project.meta.node_order.get(""),
            Some(&vec!["New Scene.md".to_string()])
        );

        // Order persisted to disk, not just in memory.
        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(
            reloaded.meta.node_order.get(""),
            Some(&vec!["New Scene.md".to_string()])
        );
    }

    #[test]
    fn create_folder_creates_directory_and_appears_in_tree() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        let path = project.create_folder(dir.path(), "Chapter 3").unwrap();

        assert!(path.is_dir());
        let node = project.tree.find_by_path(&path).expect("folder present");
        assert!(matches!(node.kind, BinderNodeKind::Folder { .. }));
    }

    #[test]
    fn recorded_order_is_applied_ahead_of_unlisted_siblings() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.md"), "").unwrap();
        fs::write(dir.path().join("a.md"), "").unwrap();

        let mut node_order = HashMap::new();
        node_order.insert("".to_string(), vec!["b.md".to_string()]);
        save_metadata(
            dir.path(),
            &ProjectMeta {
                version: 1,
                node_order,
                ..Default::default()
            },
        )
        .unwrap();

        let project = Project::load_from_folder(dir.path()).unwrap();
        let names: Vec<&str> = project
            .tree
            .root
            .children()
            .iter()
            .map(|c| c.name.as_str())
            .collect();

        assert_eq!(names, vec!["b.md", "a.md"]);
    }

    #[test]
    fn rename_document_renames_file_and_updates_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Old Name").unwrap();

        let new_path = project.rename(&path, "New Name").unwrap();

        assert_eq!(new_path, dir.path().join("New Name.md"));
        assert!(!path.exists());
        assert!(new_path.exists());
        assert!(project.tree.find_by_path(&new_path).is_some());
        assert_eq!(
            project.meta.node_order.get(""),
            Some(&vec!["New Name.md".to_string()])
        );
    }

    #[test]
    fn rename_document_updates_wikilinks_pointing_at_it_in_other_documents() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Old Name").unwrap();
        let referrer = project.create_document(dir.path(), "Referrer").unwrap();
        fs::write(
            &referrer,
            "See [[Old Name]] and [[Old Name|the other note]].",
        )
        .unwrap();

        project.rename(&target, "New Name").unwrap();

        let updated = fs::read_to_string(&referrer).unwrap();
        assert_eq!(updated, "See [[New Name]] and [[New Name|the other note]].");
    }

    #[test]
    fn rename_document_updates_a_linked_story_card() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Old Name").unwrap();

        let mut card = StoryCard::new();
        card.linked_document_stem = Some("Old Name".to_string());
        let id = card.id;
        project.upsert_story_card(card).unwrap();

        project.rename(&target, "New Name").unwrap();

        assert_eq!(
            project
                .story_card(id)
                .unwrap()
                .linked_document_stem
                .as_deref(),
            Some("New Name")
        );
    }

    #[test]
    fn rename_document_leaves_unrelated_story_cards_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Old Name").unwrap();

        let mut card = StoryCard::new();
        card.linked_document_stem = Some("Something Else".to_string());
        let id = card.id;
        project.upsert_story_card(card).unwrap();

        project.rename(&target, "New Name").unwrap();

        assert_eq!(
            project
                .story_card(id)
                .unwrap()
                .linked_document_stem
                .as_deref(),
            Some("Something Else")
        );
    }

    #[test]
    fn rename_document_leaves_unrelated_wikilinks_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Old Name").unwrap();
        let referrer = project.create_document(dir.path(), "Referrer").unwrap();
        fs::write(&referrer, "See [[Something Else]].").unwrap();

        project.rename(&target, "New Name").unwrap();

        assert_eq!(
            fs::read_to_string(&referrer).unwrap(),
            "See [[Something Else]]."
        );
    }

    #[test]
    fn rename_folder_updates_nested_order_keys() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Old Chapter").unwrap();
        project.create_document(&folder, "Scene 1").unwrap();

        let new_folder = project.rename(&folder, "New Chapter").unwrap();

        assert!(new_folder.is_dir());
        assert!(new_folder.join("Scene 1.md").exists());
        assert_eq!(
            project.meta.node_order.get("New Chapter"),
            Some(&vec!["Scene 1.md".to_string()])
        );
        assert!(!project.meta.node_order.contains_key("Old Chapter"));
    }

    #[test]
    fn rename_refuses_to_overwrite_an_existing_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "A").unwrap();
        project.create_document(dir.path(), "B").unwrap();

        let result = project.rename(&path, "B");

        assert!(result.is_err());
        assert!(path.exists());
    }

    #[test]
    fn create_document_refuses_to_overwrite_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Existing").unwrap();
        fs::write(&path, "original content").unwrap();

        let result = project.create_document(dir.path(), "Existing");

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "original content");
    }

    #[test]
    fn create_folder_refuses_to_overwrite_an_existing_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.create_folder(dir.path(), "Existing").unwrap();

        let result = project.create_folder(dir.path(), "Existing");

        assert!(result.is_err());
    }

    #[test]
    fn template_documents_is_empty_when_no_folder_holds_the_templates_role() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let templates = project.create_folder(dir.path(), "Templates").unwrap();
        project.create_document(&templates, "Character").unwrap();

        assert!(project.template_documents().is_empty());
    }

    #[test]
    fn template_documents_lists_only_direct_child_documents_of_the_templates_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let templates = project.create_folder(dir.path(), "Templates").unwrap();
        project
            .set_folder_role(&templates, Some(FolderRole::Templates))
            .unwrap();
        project
            .create_document(&templates, "Character Sheet")
            .unwrap();
        let nested = project.create_folder(&templates, "Nested").unwrap();
        project.create_document(&nested, "Too Deep").unwrap();

        let names: Vec<&str> = project
            .template_documents()
            .iter()
            .map(|doc| doc.name.as_str())
            .collect();

        assert_eq!(names, vec!["Character Sheet.md"]);
    }

    #[test]
    fn picklist_documents_is_empty_when_the_field_has_no_folder_assigned() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let people = project.create_folder(dir.path(), "People").unwrap();
        project.create_document(&people, "Alice").unwrap();

        assert!(project.picklist_documents(PicklistField::Pov).is_empty());
    }

    #[test]
    fn picklist_documents_lists_only_direct_child_documents_of_the_assigned_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let people = project.create_folder(dir.path(), "People").unwrap();
        project
            .set_picklist_folder(PicklistField::Pov, Some(&people))
            .unwrap();
        project.create_document(&people, "Alice").unwrap();
        let nested = project.create_folder(&people, "Nested").unwrap();
        project.create_document(&nested, "Too Deep").unwrap();

        let names: Vec<&str> = project
            .picklist_documents(PicklistField::Pov)
            .iter()
            .map(|doc| doc.name.as_str())
            .collect();

        assert_eq!(names, vec!["Alice.md"]);
    }

    #[test]
    fn set_picklist_folder_is_independent_per_field() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let people = project.create_folder(dir.path(), "People").unwrap();
        let types = project.create_folder(dir.path(), "Types").unwrap();
        project
            .set_picklist_folder(PicklistField::Pov, Some(&people))
            .unwrap();
        project
            .set_picklist_folder(PicklistField::Type, Some(&types))
            .unwrap();

        assert!(project.is_picklist_folder(PicklistField::Pov, &people));
        assert!(!project.is_picklist_folder(PicklistField::Type, &people));
        assert!(project.is_picklist_folder(PicklistField::Type, &types));
        assert!(!project.is_picklist_folder(PicklistField::Status, &types));
    }

    #[test]
    fn set_picklist_folder_reassignment_supersedes_the_previous_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let old = project.create_folder(dir.path(), "Old").unwrap();
        let new = project.create_folder(dir.path(), "New").unwrap();
        project
            .set_picklist_folder(PicklistField::Status, Some(&old))
            .unwrap();
        project
            .set_picklist_folder(PicklistField::Status, Some(&new))
            .unwrap();

        assert!(!project.is_picklist_folder(PicklistField::Status, &old));
        assert!(project.is_picklist_folder(PicklistField::Status, &new));
    }

    #[test]
    fn set_picklist_folder_none_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let people = project.create_folder(dir.path(), "People").unwrap();
        project
            .set_picklist_folder(PicklistField::Pov, Some(&people))
            .unwrap();
        project
            .set_picklist_folder(PicklistField::Pov, None)
            .unwrap();

        assert!(!project.is_picklist_folder(PicklistField::Pov, &people));
        assert!(project.picklist_documents(PicklistField::Pov).is_empty());
    }

    #[test]
    fn set_picklist_folder_errors_for_a_document_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Note").unwrap();

        assert!(
            project
                .set_picklist_folder(PicklistField::Type, Some(&doc))
                .is_err()
        );
    }

    #[test]
    fn create_document_from_template_copies_content_including_frontmatter_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let templates = project.create_folder(dir.path(), "Templates").unwrap();
        project
            .set_folder_role(&templates, Some(FolderRole::Templates))
            .unwrap();
        let template_path = project
            .create_document(&templates, "Character Sheet")
            .unwrap();
        let contents = "---\ntype: Character\n---\n## Backstory\n\n";
        fs::write(&template_path, contents).unwrap();

        let new_path = project
            .create_document_from_template(dir.path(), "Aria", &template_path, "")
            .unwrap();

        assert_eq!(fs::read_to_string(&new_path).unwrap(), contents);
        assert_eq!(fs::read_to_string(&template_path).unwrap(), contents);
    }

    #[test]
    fn create_document_from_template_substitutes_name_and_date() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let template_path = project.create_document(dir.path(), "Template").unwrap();
        fs::write(&template_path, "# ${{name}}\n\nStarted ${{date}}.\n").unwrap();

        let new_path = project
            .create_document_from_template(dir.path(), "Aria", &template_path, "%Y")
            .unwrap();

        let year = chrono::Local::now().format("%Y").to_string();
        assert_eq!(
            fs::read_to_string(&new_path).unwrap(),
            format!("# Aria\n\nStarted {year}.\n")
        );
    }

    #[test]
    fn create_document_from_template_refuses_to_overwrite_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let template_path = project.create_document(dir.path(), "Template").unwrap();
        let existing = project.create_document(dir.path(), "Existing").unwrap();
        fs::write(&existing, "original content").unwrap();

        let result =
            project.create_document_from_template(dir.path(), "Existing", &template_path, "");

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&existing).unwrap(), "original content");
    }

    #[test]
    fn create_document_from_template_refuses_a_name_that_escapes_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let template_path = project.create_document(dir.path(), "Template").unwrap();

        let result =
            project.create_document_from_template(dir.path(), "../escaped", &template_path, "");

        assert!(result.is_err());
        assert!(!dir.path().parent().unwrap().join("escaped.md").exists());
    }

    #[test]
    fn create_document_with_content_writes_verbatim_with_no_substitution() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        let path = project
            .create_document_with_content(dir.path(), "Aria", "# ${{name}}\n\n${{date}}\n")
            .unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# ${{name}}\n\n${{date}}\n"
        );
    }

    #[test]
    fn create_document_refuses_a_name_that_escapes_the_project_with_dot_dot() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        let result = project.create_document(dir.path(), "../escaped");

        assert!(result.is_err());
        assert!(!dir.path().parent().unwrap().join("escaped.md").exists());
    }

    #[test]
    fn create_document_refuses_an_absolute_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let absolute_target = outside.path().join("evil.md");

        let result = project.create_document(dir.path(), absolute_target.to_str().unwrap());

        assert!(result.is_err());
        assert!(!absolute_target.exists());
    }

    #[test]
    fn create_folder_refuses_a_name_containing_a_path_separator() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        let result = project.create_folder(dir.path(), "a/b");

        assert!(result.is_err());
        assert!(!dir.path().join("a").exists());
    }

    #[test]
    fn rename_refuses_a_new_name_that_escapes_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Doc").unwrap();

        let result = project.rename(&path, "../escaped");

        assert!(result.is_err());
        assert!(path.exists());
        assert!(!dir.path().parent().unwrap().join("escaped.md").exists());
    }

    #[test]
    fn delete_document_removes_file_and_order_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Doomed").unwrap();

        project.delete(&path).unwrap();

        assert!(!path.exists());
        assert!(project.tree.find_by_path(&path).is_none());
        assert_eq!(project.meta.node_order.get(""), Some(&vec![]));
    }

    #[test]
    fn delete_folder_removes_directory_and_nested_order_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();
        project.create_document(&folder, "Scene 1").unwrap();

        project.delete(&folder).unwrap();

        assert!(!folder.exists());
        assert!(project.tree.find_by_path(&folder).is_none());
        assert!(!project.meta.node_order.contains_key("Chapter 1"));
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

    #[test]
    fn move_item_moves_a_document_and_updates_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();

        let new_path = project.move_item(&doc, &folder).unwrap();

        assert_eq!(new_path, folder.join("Scene 1.md"));
        assert!(!doc.exists());
        assert!(new_path.exists());
        assert!(project.tree.find_by_path(&new_path).is_some());
        // Root's order keeps the folder entry — only "Scene 1.md" is removed from it.
        assert_eq!(
            project.meta.node_order.get(""),
            Some(&vec!["Chapter 1".to_string()])
        );
        assert_eq!(
            project.meta.node_order.get("Chapter 1"),
            Some(&vec!["Scene 1.md".to_string()])
        );
    }

    #[test]
    fn move_item_refuses_to_overwrite_a_same_named_file_in_the_target_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();
        project.create_document(&folder, "Scene 1").unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();

        let result = project.move_item(&doc, &folder);

        assert!(result.is_err());
        // Nothing moved: both copies are exactly where they started.
        assert!(doc.exists());
        assert!(folder.join("Scene 1.md").exists());
    }

    #[test]
    fn move_item_refuses_to_overwrite_a_same_named_folder_in_the_target_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_folder(dir.path(), "Target").unwrap();
        project.create_folder(&target, "Chapter 1").unwrap();
        let source = project.create_folder(dir.path(), "Chapter 1").unwrap();

        let result = project.move_item(&source, &target);

        assert!(result.is_err());
        assert!(source.exists());
        assert!(target.join("Chapter 1").exists());
    }

    #[test]
    fn move_item_moves_a_folder_and_all_its_descendants() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_folder(dir.path(), "Target").unwrap();
        let source = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let nested_folder = project.create_folder(&source, "Nested").unwrap();
        project.create_document(&source, "Scene 1").unwrap();
        project
            .create_document(&nested_folder, "Sub Scene")
            .unwrap();

        let new_path = project.move_item(&source, &target).unwrap();

        assert_eq!(new_path, target.join("Chapter 1"));
        assert!(!source.exists());
        assert!(new_path.join("Scene 1.md").exists());
        assert!(new_path.join("Nested").join("Sub Scene.md").exists());
        assert!(project.tree.find_by_path(&new_path).is_some());
        assert!(
            project
                .tree
                .find_by_path(&new_path.join("Nested").join("Sub Scene.md"))
                .is_some()
        );
        // Nested order keys ("Chapter 1", "Chapter 1/Nested") followed the folder to
        // its new relative location under "Target".
        assert_eq!(
            project.meta.node_order.get("Target/Chapter 1"),
            Some(&vec!["Nested".to_string(), "Scene 1.md".to_string()])
        );
        assert_eq!(
            project.meta.node_order.get("Target/Chapter 1/Nested"),
            Some(&vec!["Sub Scene.md".to_string()])
        );
    }

    #[test]
    fn move_item_refuses_to_move_a_folder_into_itself() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();

        let result = project.move_item(&folder, &folder);

        assert!(result.is_err());
        assert!(folder.exists());
    }

    #[test]
    fn move_item_refuses_to_move_a_folder_into_its_own_subfolder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let nested = project.create_folder(&folder, "Nested").unwrap();

        let result = project.move_item(&folder, &nested);

        assert!(result.is_err());
        assert!(folder.exists());
        assert!(nested.exists());
    }

    #[test]
    fn move_item_does_not_change_a_documents_stem() {
        // A move (unlike a rename) never touches the filename, so wikilinks that
        // resolve by stem keep working with no relinking needed.
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();

        project.move_item(&doc, &folder).unwrap();

        assert!(project.tree.find_document_by_stem("Scene 1").is_some());
    }

    #[test]
    fn move_item_of_a_role_holding_folder_keeps_the_role_pointing_at_the_new_location() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_folder(dir.path(), "Target").unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();

        let new_path = project.move_item(&trash, &target).unwrap();

        assert_eq!(project.folder_role(&new_path), Some(FolderRole::Trash));
        assert_eq!(project.folder_role(&trash), None);
    }

    #[test]
    fn rename_of_a_role_holding_folder_keeps_the_role_pointing_at_the_new_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();

        let new_path = project.rename(&trash, "Recycle Bin").unwrap();

        assert_eq!(project.folder_role(&new_path), Some(FolderRole::Trash));
        assert_eq!(project.folder_role(&trash), None);
    }

    #[test]
    fn a_role_survives_being_moved_and_then_actually_used() {
        // The regression this guards against: a stale folder_roles key after a move
        // meant trash_path() kept returning the OLD (now-nonexistent) location, so a
        // subsequent delete() that should route to Trash would instead try to
        // fs::rename into a directory that no longer exists.
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_folder(dir.path(), "Target").unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let new_trash = project.move_item(&trash, &target).unwrap();
        let doc = project.create_document(dir.path(), "Doomed").unwrap();

        project.delete(&doc).unwrap();

        assert!(new_trash.join("Doomed.md").exists());
    }

    #[test]
    fn move_item_of_a_folder_containing_a_previously_trashed_items_bookkeeping_follows_it() {
        // trashed_origins is keyed by an item's *current* location; moving the Trash
        // folder itself (which holds that item) must carry the key along so
        // restore_from_trash can still find it afterward.
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_folder(dir.path(), "Target").unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let doc = project.create_document(&chapter, "Notes").unwrap();
        project.delete(&doc).unwrap();

        let new_trash = project.move_item(&trash, &target).unwrap();

        let restored = project
            .restore_from_trash(&new_trash.join("Notes.md"), false)
            .unwrap();
        assert_eq!(restored, chapter.join("Notes.md"));
        assert!(restored.exists());
    }

    #[test]
    fn move_item_dropped_onto_its_own_parent_repositions_to_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let scene1 = project.create_document(&folder, "Scene 1").unwrap();
        project.create_document(&folder, "Scene 2").unwrap();

        // Dropping Scene 1 onto Chapter 1's own header — previously this refused
        // (treated as a collision with the item's own still-existing path).
        let new_path = project.move_item(&scene1, &folder).unwrap();

        assert_eq!(new_path, scene1);
        assert!(scene1.exists());
        assert_eq!(
            project.meta.node_order.get("Chapter 1"),
            Some(&vec!["Scene 2.md".to_string(), "Scene 1.md".to_string()])
        );
    }

    #[test]
    fn move_item_before_reorders_siblings_within_the_same_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();
        project.create_document(&folder, "Scene 1").unwrap();
        project.create_document(&folder, "Scene 2").unwrap();
        let scene3 = project.create_document(&folder, "Scene 3").unwrap();
        let scene1 = folder.join("Scene 1.md");

        // Drag Scene 3 to sit immediately before Scene 1.
        let new_path = project.move_item_before(&scene3, &scene1).unwrap();

        assert_eq!(new_path, scene3, "same folder: no filesystem move needed");
        assert!(scene3.exists());
        assert_eq!(
            project.meta.node_order.get("Chapter 1"),
            Some(&vec![
                "Scene 3.md".to_string(),
                "Scene 1.md".to_string(),
                "Scene 2.md".to_string(),
            ])
        );
    }

    #[test]
    fn move_item_before_moves_into_a_different_folder_at_the_target_position() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let chapter1 = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let chapter2 = project.create_folder(dir.path(), "Chapter 2").unwrap();
        let doc = project.create_document(&chapter1, "Scene 1").unwrap();
        project.create_document(&chapter2, "Scene A").unwrap();
        let scene_b = project.create_document(&chapter2, "Scene B").unwrap();

        let new_path = project.move_item_before(&doc, &scene_b).unwrap();

        assert_eq!(new_path, chapter2.join("Scene 1.md"));
        assert!(!doc.exists());
        assert!(new_path.exists());
        assert_eq!(project.meta.node_order.get("Chapter 1"), Some(&vec![]));
        assert_eq!(
            project.meta.node_order.get("Chapter 2"),
            Some(&vec![
                "Scene A.md".to_string(),
                "Scene 1.md".to_string(),
                "Scene B.md".to_string(),
            ])
        );
    }

    #[test]
    fn move_item_before_refuses_to_move_an_item_before_itself() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();

        let result = project.move_item_before(&doc, &doc);

        assert!(result.is_err());
        assert!(doc.exists());
    }

    #[test]
    fn move_item_before_refuses_to_move_a_folder_before_something_inside_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let nested = project.create_document(&folder, "Scene 1").unwrap();

        let result = project.move_item_before(&folder, &nested);

        assert!(result.is_err());
        assert!(folder.exists());
        assert!(nested.exists());
    }

    #[test]
    fn move_item_before_reorders_correctly_among_siblings_never_explicitly_ordered() {
        // Regression test for a reported bug: 5 scene files that had never been
        // individually reordered before (so `node_order` had no entry for any
        // of them — the exact state of files discovered by scanning, as
        // opposed to created one at a time through `create_document`, each of
        // which calls `record_new_child` and gets an entry immediately).
        // Dragging the last one onto its neighbor sent it to the very top of
        // the chapter instead of landing next to where it was dropped, because
        // the target's position was found (or not) in that near-empty
        // `node_order` list rather than in the full, currently-displayed list
        // of siblings.
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();
        for i in 1..=5 {
            fs::write(folder.join(format!("Scene {i}.md")), "").unwrap();
        }
        project.rescan();
        assert!(
            !project.meta.node_order.contains_key("Chapter 1"),
            "sanity check: none of the 5 scenes should be individually tracked yet"
        );

        let scene4 = folder.join("Scene 4.md");
        let scene5 = folder.join("Scene 5.md");
        project.move_item_before(&scene5, &scene4).unwrap();

        assert_eq!(
            project.meta.node_order.get("Chapter 1"),
            Some(&vec![
                "Scene 1.md".to_string(),
                "Scene 2.md".to_string(),
                "Scene 3.md".to_string(),
                "Scene 5.md".to_string(),
                "Scene 4.md".to_string(),
            ])
        );
    }

    #[test]
    fn backlinks_finds_a_link_from_another_document() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Target").unwrap();
        let referrer = project.create_document(dir.path(), "Referrer").unwrap();
        fs::write(&referrer, "See [[Target]] for more.").unwrap();

        let backlinks = project.backlinks(&target);

        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].source_path, referrer);
        assert_eq!(backlinks[0].source_title, "Referrer");
        assert!(backlinks[0].snippet.contains("[[Target]]"));
    }

    #[test]
    fn backlinks_snippet_excludes_frontmatter_even_for_a_link_near_the_top_of_the_body() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Target").unwrap();
        let referrer = project.create_document(dir.path(), "Referrer").unwrap();
        fs::write(
            &referrer,
            "---\ntype: Scene\nstatus: draft\n---\nLink here: [[Target]]",
        )
        .unwrap();

        let backlinks = project.backlinks(&target);

        assert_eq!(backlinks.len(), 1);
        assert!(backlinks[0].snippet.contains("[[Target]]"));
        assert!(!backlinks[0].snippet.contains("type: Scene"));
        assert!(!backlinks[0].snippet.contains("---"));
    }

    #[test]
    fn backlinks_excludes_a_self_link() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Target").unwrap();
        fs::write(&target, "This document links to [[Target]] itself.").unwrap();

        let backlinks = project.backlinks(&target);

        assert!(backlinks.is_empty());
    }

    #[test]
    fn backlinks_matches_target_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Café").unwrap();
        let referrer = project.create_document(dir.path(), "Referrer").unwrap();
        fs::write(&referrer, "See [[CAFÉ]] for more.").unwrap();

        let backlinks = project.backlinks(&target);

        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].source_path, referrer);
    }

    #[test]
    fn backlinks_returns_one_entry_per_occurrence_when_a_document_links_twice() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Target").unwrap();
        let referrer = project.create_document(dir.path(), "Referrer").unwrap();
        fs::write(
            &referrer,
            "First [[Target]] mention.\n\nSecond [[Target]] mention.",
        )
        .unwrap();

        let backlinks = project.backlinks(&target);

        assert_eq!(backlinks.len(), 2);
        assert!(backlinks[0].snippet.contains("First"));
        assert!(backlinks[1].snippet.contains("Second"));
    }

    #[test]
    fn backlinks_is_empty_when_nothing_links_to_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Target").unwrap();
        let other = project.create_document(dir.path(), "Other").unwrap();
        fs::write(&other, "Nothing to see here.").unwrap();

        assert!(project.backlinks(&target).is_empty());
    }

    #[test]
    fn backlinks_ignores_links_inside_fenced_code_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Target").unwrap();
        let referrer = project.create_document(dir.path(), "Referrer").unwrap();
        fs::write(&referrer, "```\n[[Target]]\n```\n").unwrap();

        assert!(project.backlinks(&target).is_empty());
    }

    #[test]
    fn related_by_tag_finds_a_document_sharing_a_frontmatter_tag() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Target").unwrap();
        let other = project.create_document(dir.path(), "Other").unwrap();
        fs::write(&target, "---\ntags: [foo]\n---\nBody.").unwrap();
        fs::write(&other, "---\ntags: [foo]\n---\nBody.").unwrap();

        let groups = project.related_by_tag(&target);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].tag, "foo");
        assert_eq!(groups[0].documents, vec![(other, "Other".to_string())]);
    }

    #[test]
    fn related_by_tag_finds_a_document_sharing_an_inline_tag() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Target").unwrap();
        let other = project.create_document(dir.path(), "Other").unwrap();
        fs::write(&target, "Body with #foo mentioned.").unwrap();
        fs::write(&other, "Also #foo here.").unwrap();

        let groups = project.related_by_tag(&target);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].tag, "foo");
        assert_eq!(groups[0].documents, vec![(other, "Other".to_string())]);
    }

    #[test]
    fn related_by_tag_merges_frontmatter_and_inline_tags_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Target").unwrap();
        fs::write(&target, "---\ntags: [Foo]\n---\nAlso #foo inline.").unwrap();

        let groups = project.related_by_tag(&target);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].tag, "Foo");
    }

    #[test]
    fn related_by_tag_keeps_a_tag_with_no_other_matching_document() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Target").unwrap();
        fs::write(&target, "---\ntags: [lonely]\n---\nBody.").unwrap();

        let groups = project.related_by_tag(&target);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].tag, "lonely");
        assert!(groups[0].documents.is_empty());
    }

    #[test]
    fn related_by_tag_is_empty_for_a_document_with_no_tags() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Target").unwrap();
        fs::write(&target, "Body with no tags.").unwrap();

        assert!(project.related_by_tag(&target).is_empty());
    }

    #[test]
    fn related_by_tag_excludes_the_target_document_itself() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Target").unwrap();
        fs::write(&target, "---\ntags: [foo]\n---\nBody.").unwrap();

        let groups = project.related_by_tag(&target);

        assert_eq!(groups.len(), 1);
        assert!(groups[0].documents.is_empty());
    }

    #[test]
    fn documents_with_tag_matches_case_insensitively_across_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let a = project.create_document(dir.path(), "A").unwrap();
        let b = project.create_document(dir.path(), "B").unwrap();
        let c = project.create_document(dir.path(), "C").unwrap();
        fs::write(&a, "---\ntags: [Foo]\n---\nBody.").unwrap();
        fs::write(&b, "Inline #FOO tag.").unwrap();
        fs::write(&c, "No tags here.").unwrap();

        let matches = project.documents_with_tag("foo");

        assert_eq!(matches, vec![(a, "A".to_string()), (b, "B".to_string())]);
    }

    #[test]
    fn documents_with_tag_is_empty_when_nothing_matches() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();
        fs::write(&doc, "No tags here.").unwrap();

        assert!(project.documents_with_tag("foo").is_empty());
    }
}
