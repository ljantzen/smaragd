mod binder_color_mode;
mod create;
mod folder_meta;
mod meta;
pub mod model;
mod picklists;
mod pov_colors;
mod queries;
mod rename_move_delete;
mod roles;
mod scan;
mod status_colors;
mod story_cards;
mod streak;
mod trash;
mod word_count;

pub use binder_color_mode::BinderColorMode;
pub use meta::ProjectMeta;
pub use picklists::PicklistField;
pub use queries::{BacklinkEntry, TagGroup};
pub use roles::FolderRole;
pub use story_cards::StoryCard;
pub use word_count::WordCountScope;

use std::collections::{BTreeMap, HashMap};
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

    /// Every document in the project, in binder order, skipping any folder whose
    /// role is `Trash` or `Templates` — the same skip rule `export::gather_into`
    /// uses when compiling a manuscript, reused here as the "manuscript position"
    /// a Story Grid row is sorted by (see `ui::story_grid_panel`). Unlike
    /// `export::gather`, this never reads file contents.
    pub fn manuscript_document_order(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        self.collect_manuscript_document_order(&self.tree.root, &mut out);
        out
    }

    fn collect_manuscript_document_order(&self, node: &BinderNode, out: &mut Vec<PathBuf>) {
        match &node.kind {
            BinderNodeKind::Document => out.push(node.path.clone()),
            BinderNodeKind::Folder { children } => {
                if matches!(
                    self.folder_role(&node.path),
                    Some(FolderRole::Trash) | Some(FolderRole::Templates)
                ) {
                    return;
                }
                for child in children {
                    self.collect_manuscript_document_order(child, out);
                }
            }
        }
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

    /// Set the book-level title/subtitle/author/typesetting-style shown in the
    /// Export dialog — see `ProjectMeta::book_title`/`book_subtitle`/
    /// `book_author`/`book_style`. An empty title/subtitle/author is stored as
    /// `None` rather than `Some(String::new())`.
    pub fn set_book_meta(
        &mut self,
        title: String,
        subtitle: String,
        author: String,
        style_id: String,
    ) -> io::Result<()> {
        self.meta.book_title = (!title.is_empty()).then_some(title);
        self.meta.book_subtitle = (!subtitle.is_empty()).then_some(subtitle);
        self.meta.book_author = (!author.is_empty()).then_some(author);
        self.meta.book_style = (!style_id.is_empty()).then_some(style_id);
        self.save_metadata()
    }

    /// Set just `book_title`, leaving `book_author`/`book_style` untouched —
    /// used by the Metadata dock's project-wide fields (see
    /// `ui::metadata_panel::show_project`), which edit the title live and
    /// have no style selection of their own the way the Export dialog does.
    pub fn set_book_title(&mut self, title: String) -> io::Result<()> {
        self.meta.book_title = (!title.is_empty()).then_some(title);
        self.save_metadata()
    }

    /// Same as `set_book_title`, for `book_author`.
    pub fn set_book_author(&mut self, author: String) -> io::Result<()> {
        self.meta.book_author = (!author.is_empty()).then_some(author);
        self.save_metadata()
    }

    /// Same as `set_book_title`, for `book_subtitle`.
    pub fn set_book_subtitle(&mut self, subtitle: String) -> io::Result<()> {
        self.meta.book_subtitle = (!subtitle.is_empty()).then_some(subtitle);
        self.save_metadata()
    }

    /// See `ProjectMeta::logline`.
    pub fn set_logline(&mut self, logline: String) -> io::Result<()> {
        self.meta.logline = logline;
        self.save_metadata()
    }

    /// See `ProjectMeta::point`.
    pub fn set_point(&mut self, point: String) -> io::Result<()> {
        self.meta.point = point;
        self.save_metadata()
    }

    /// See `ProjectMeta::synopsis`.
    pub fn set_synopsis(&mut self, synopsis: String) -> io::Result<()> {
        self.meta.synopsis = synopsis;
        self.save_metadata()
    }

    /// See `ProjectMeta::what_if`.
    pub fn set_what_if(&mut self, what_if: String) -> io::Result<()> {
        self.meta.what_if = what_if;
        self.save_metadata()
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
        let mut daily_word_counts = BTreeMap::new();
        daily_word_counts.insert("2024-01-08".to_string(), 512);
        daily_word_counts.insert("2024-01-09".to_string(), 0);
        let meta = ProjectMeta {
            version: 1,
            node_order,
            daily_word_counts,
            ..Default::default()
        };

        save_metadata(dir.path(), &meta).unwrap();
        let loaded = load_metadata(dir.path()).unwrap();

        assert_eq!(loaded, meta);
    }

    #[test]
    fn manuscript_document_order_follows_binder_order_and_skips_trash_and_templates() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let templates = project.create_folder(dir.path(), "Templates").unwrap();
        project
            .set_folder_role(&templates, Some(FolderRole::Templates))
            .unwrap();
        let one = project.create_document(dir.path(), "01").unwrap();
        let two = project.create_document(dir.path(), "02").unwrap();
        project.create_document(&trash, "Trashed").unwrap();
        project.create_document(&templates, "Template").unwrap();
        project.rescan();

        let order = project.manuscript_document_order();

        assert_eq!(order, vec![one, two]);
    }

    #[test]
    fn manuscript_document_order_respects_node_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let first = project.create_document(dir.path(), "Alpha").unwrap();
        let second = project.create_document(dir.path(), "Beta").unwrap();
        project
            .move_item_before(&second, &first)
            .expect("reorder Beta before Alpha");

        let order = project.manuscript_document_order();

        assert_eq!(order, vec![second, first]);
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
        assert_eq!(meta.draft_target_words, None);
        assert_eq!(meta.session_target_words, None);
        assert_eq!(meta.word_count_scope, WordCountScope::ManuscriptOnly);
        assert_eq!(meta.session_baseline_words, 0);
        assert_eq!(meta.session_baseline_date, None);
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
    fn set_book_meta_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        assert_eq!(project.meta.book_title, None);
        assert_eq!(project.meta.book_author, None);
        assert_eq!(project.meta.book_style, None);

        project
            .set_book_meta(
                "My Book".to_string(),
                "A Subtitle".to_string(),
                "Jane Doe".to_string(),
                "trade_paperback".to_string(),
            )
            .unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(reloaded.meta.book_title, Some("My Book".to_string()));
        assert_eq!(reloaded.meta.book_subtitle, Some("A Subtitle".to_string()));
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
                "A Subtitle".to_string(),
                "Jane Doe".to_string(),
                "trade_paperback".to_string(),
            )
            .unwrap();
        project
            .set_book_meta(String::new(), String::new(), String::new(), String::new())
            .unwrap();

        assert_eq!(project.meta.book_title, None);
        assert_eq!(project.meta.book_subtitle, None);
        assert_eq!(project.meta.book_author, None);
        assert_eq!(project.meta.book_style, None);
    }

    #[test]
    fn set_book_subtitle_persists_and_empty_stores_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        assert_eq!(project.meta.book_subtitle, None);

        project
            .set_book_subtitle("A Tale of Two Cities".to_string())
            .unwrap();
        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(
            reloaded.meta.book_subtitle,
            Some("A Tale of Two Cities".to_string())
        );

        project.set_book_subtitle(String::new()).unwrap();
        assert_eq!(project.meta.book_subtitle, None);
    }

    #[test]
    fn set_book_title_leaves_author_and_style_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project
            .set_book_meta(
                "Old Title".to_string(),
                "Old Subtitle".to_string(),
                "Jane Doe".to_string(),
                "trade_paperback".to_string(),
            )
            .unwrap();

        project.set_book_title("New Title".to_string()).unwrap();

        assert_eq!(project.meta.book_title, Some("New Title".to_string()));
        assert_eq!(project.meta.book_subtitle, Some("Old Subtitle".to_string()));
        assert_eq!(project.meta.book_author, Some("Jane Doe".to_string()));
        assert_eq!(project.meta.book_style, Some("trade_paperback".to_string()));
    }

    #[test]
    fn project_wide_pitch_fields_persist_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        assert_eq!(project.meta.logline, "");
        assert_eq!(project.meta.synopsis, "");
        assert_eq!(project.meta.what_if, "");

        project
            .set_logline("A thief steals time itself.".to_string())
            .unwrap();
        project
            .set_synopsis("A longer summary of the plot.".to_string())
            .unwrap();
        project
            .set_what_if("What if memories could be stolen?".to_string())
            .unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(reloaded.meta.logline, "A thief steals time itself.");
        assert_eq!(reloaded.meta.synopsis, "A longer summary of the plot.");
        assert_eq!(reloaded.meta.what_if, "What if memories could be stolen?");
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
        card.linked_document_stems = vec!["Old Name".to_string()];
        let id = card.id;
        project.upsert_story_card(card).unwrap();

        project.rename(&target, "New Name").unwrap();

        assert_eq!(
            project.story_card(id).unwrap().linked_document_stems,
            vec!["New Name".to_string()]
        );
    }

    #[test]
    fn rename_document_leaves_unrelated_story_cards_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Old Name").unwrap();

        let mut card = StoryCard::new();
        card.linked_document_stems = vec!["Something Else".to_string()];
        let id = card.id;
        project.upsert_story_card(card).unwrap();

        project.rename(&target, "New Name").unwrap();

        assert_eq!(
            project.story_card(id).unwrap().linked_document_stems,
            vec!["Something Else".to_string()]
        );
    }

    #[test]
    fn rename_document_updates_only_the_matching_stem_in_a_multi_linked_story_card() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_document(dir.path(), "Old Name").unwrap();

        let mut card = StoryCard::new();
        card.linked_document_stems = vec!["Old Name".to_string(), "Something Else".to_string()];
        let id = card.id;
        project.upsert_story_card(card).unwrap();

        project.rename(&target, "New Name").unwrap();

        assert_eq!(
            project.story_card(id).unwrap().linked_document_stems,
            vec!["New Name".to_string(), "Something Else".to_string()]
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
    fn rename_of_a_folder_with_metadata_keeps_it_pointing_at_the_new_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let meta = crate::frontmatter::DocumentMeta {
            status: Some("draft".to_string()),
            ..Default::default()
        };
        project.set_folder_meta(&chapter, meta.clone()).unwrap();

        let new_path = project.rename(&chapter, "Chapter One").unwrap();

        assert_eq!(project.folder_meta(&new_path), meta);
        assert_eq!(
            project.folder_meta(&chapter),
            crate::frontmatter::DocumentMeta::default()
        );
    }

    #[test]
    fn move_item_of_a_folder_with_metadata_keeps_it_pointing_at_the_new_location() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let target = project.create_folder(dir.path(), "Target").unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let meta = crate::frontmatter::DocumentMeta {
            status: Some("draft".to_string()),
            ..Default::default()
        };
        project.set_folder_meta(&chapter, meta.clone()).unwrap();

        let new_path = project.move_item(&chapter, &target).unwrap();

        assert_eq!(project.folder_meta(&new_path), meta);
        assert_eq!(
            project.folder_meta(&chapter),
            crate::frontmatter::DocumentMeta::default()
        );
    }

    #[test]
    fn permanently_delete_removes_folder_meta_under_the_deleted_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        project
            .set_folder_meta(
                &chapter,
                crate::frontmatter::DocumentMeta {
                    status: Some("draft".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        project.delete(&chapter).unwrap();

        assert!(project.meta.folder_meta.is_empty());
    }

    #[test]
    fn permanently_delete_of_a_single_document_does_not_touch_folder_meta() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        project
            .set_folder_meta(
                &chapter,
                crate::frontmatter::DocumentMeta {
                    status: Some("draft".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        let doc = project.create_document(&chapter, "Scene 1").unwrap();

        project.delete(&doc).unwrap();

        assert!(!project.meta.folder_meta.is_empty());
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
}
