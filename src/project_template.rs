//! Scrivener-style project templates: built-in Blank/Novel/Nonfiction/Screenplay/World-Building
//! scaffolds, plus user-saved custom templates loaded from a `template.toml` +
//! `content/` directory pair in [`global_project_templates_dir`] (see [`load`]).
//! Applied to a freshly-`Project::initialize`d project by `SmaragdApp::
//! create_project`, before `set_project`/`ensure_starter_folders` run — see
//! [`ProjectTemplate::apply`].
//!
//! Deliberately structural only: a template describes folder/document shape and
//! starter text, never a project's narrative state (`ProjectMeta::story_cards`,
//! `protagonist_desire`/`protagonist_misbelief`, book/export/git metadata) — see
//! [`save_from_project`]. This is a distinct concept from `crate::templates`
//! (the `${{name}}`/`${{date}}` *document*-level "New From Template" stationery
//! feature, driven by a project's own `FolderRole::Templates` folder): that
//! substitutes placeholders into a single new document inside an existing
//! project, while this scaffolds a whole project's folder/document tree at
//! creation time.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::project::model::{BinderNode, BinderNodeKind};
use crate::project::{FolderRole, Project};

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectTemplate {
    /// Fixed strings for built-ins ("blank"/"novel"/"nonfiction"/"screenplay"), or
    /// a custom template's own `content/`-sibling directory name (lowercased) —
    /// see `load`'s doc comment for why a custom template's id isn't a separate
    /// field inside its `template.toml`.
    pub id: String,
    pub label: String,
    pub description: String,
    /// The folder/document tree to stamp out, in order, under the new project's
    /// root. Empty for "Blank".
    pub entries: Vec<TemplateEntry>,
    /// Which folders (by the same `/`-separated relative-path convention as
    /// `ProjectMeta::folder_roles`, "" meaning the project root, keyed against
    /// this template's own `entries`) get tagged with a role once created.
    pub folder_roles: HashMap<String, FolderRole>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TemplateEntry {
    Folder {
        name: String,
        children: Vec<TemplateEntry>,
    },
    Document {
        name: String,
        content: String,
    },
}

impl TemplateEntry {
    fn folder(name: impl Into<String>, children: Vec<TemplateEntry>) -> Self {
        TemplateEntry::Folder {
            name: name.into(),
            children,
        }
    }

    fn document(name: impl Into<String>, content: impl Into<String>) -> Self {
        TemplateEntry::Document {
            name: name.into(),
            content: content.into(),
        }
    }
}

fn blank_template() -> ProjectTemplate {
    ProjectTemplate {
        id: "blank".to_string(),
        label: "Blank".to_string(),
        description: "An empty project — nothing scaffolded.".to_string(),
        entries: Vec::new(),
        folder_roles: HashMap::new(),
    }
}

fn novel_template() -> ProjectTemplate {
    ProjectTemplate {
        id: "novel".to_string(),
        label: "Novel".to_string(),
        description:
            "Chapters, Characters, Research, and Trash — a standard fiction manuscript layout."
                .to_string(),
        entries: vec![
            TemplateEntry::folder(
                "Manuscript",
                vec![
                    TemplateEntry::document("Chapter 1.md", "# Chapter 1\n\n"),
                    TemplateEntry::document("Chapter 2.md", "# Chapter 2\n\n"),
                ],
            ),
            TemplateEntry::folder(
                "Characters",
                vec![TemplateEntry::document(
                    "Protagonist.md",
                    "# Protagonist\n\n## Desire\n\n## Misbelief\n\n## Arc\n\n",
                )],
            ),
            TemplateEntry::folder("Research", vec![]),
            TemplateEntry::folder("Trash", vec![]),
        ],
        folder_roles: HashMap::from([
            ("Research".to_string(), FolderRole::Research),
            ("Trash".to_string(), FolderRole::Trash),
        ]),
    }
}

fn nonfiction_template() -> ProjectTemplate {
    ProjectTemplate {
        id: "nonfiction".to_string(),
        label: "Nonfiction".to_string(),
        description: "Parts and chapters for a nonfiction book, plus Research and Trash."
            .to_string(),
        entries: vec![
            TemplateEntry::folder(
                "Manuscript",
                vec![
                    TemplateEntry::document("Introduction.md", "# Introduction\n\n"),
                    TemplateEntry::folder(
                        "Part One",
                        vec![TemplateEntry::document("Chapter 1.md", "# Chapter 1\n\n")],
                    ),
                ],
            ),
            TemplateEntry::folder("Research", vec![]),
            TemplateEntry::folder("Trash", vec![]),
        ],
        folder_roles: HashMap::from([
            ("Research".to_string(), FolderRole::Research),
            ("Trash".to_string(), FolderRole::Trash),
        ]),
    }
}

fn screenplay_template() -> ProjectTemplate {
    ProjectTemplate {
        id: "screenplay".to_string(),
        label: "Screenplay".to_string(),
        description: "Act folders for a screenplay draft, plus Research and Trash.".to_string(),
        // Smaragd's editor is plain markdown with no Fountain-format support, so
        // this starter content reproduces a screenplay draft's *look* in plain
        // markdown headings, not a real Fountain pipeline — the same "look, not
        // the pipeline" caveat `color_theme.rs` documents for its own markdown-
        // preview overrides.
        entries: vec![
            TemplateEntry::folder(
                "Screenplay",
                vec![
                    TemplateEntry::document("Act One.md", "# ACT ONE\n\nFADE IN:\n\n"),
                    TemplateEntry::document("Act Two.md", "# ACT TWO\n\n"),
                    TemplateEntry::document("Act Three.md", "# ACT THREE\n\n"),
                ],
            ),
            TemplateEntry::folder("Research", vec![]),
            TemplateEntry::folder("Trash", vec![]),
        ],
        folder_roles: HashMap::from([
            ("Research".to_string(), FolderRole::Research),
            ("Trash".to_string(), FolderRole::Trash),
        ]),
    }
}

fn world_building_template() -> ProjectTemplate {
    ProjectTemplate {
        id: "worldbuilding".to_string(),
        label: "World-Building".to_string(),
        description:
            "Manuscript and Research, plus a World folder for characters, locations, and items, and starter document templates."
                .to_string(),
        entries: vec![
            TemplateEntry::folder(
                "Manuscript",
                vec![TemplateEntry::document("Chapter 1.md", "# Chapter 1\n\n")],
            ),
            TemplateEntry::folder("Research", vec![]),
            TemplateEntry::folder(
                "World",
                vec![
                    TemplateEntry::folder(
                        "Characters",
                        vec![
                            TemplateEntry::folder("Main Characters", vec![]),
                            TemplateEntry::folder("Supporting Characters", vec![]),
                        ],
                    ),
                    TemplateEntry::folder("Locations", vec![]),
                    TemplateEntry::folder("Items", vec![]),
                ],
            ),
            TemplateEntry::folder(
                "Templates",
                vec![
                    TemplateEntry::document(
                        "Character.md",
                        "# ${{name}}\n\n## Want\n\n## Misbelief\n\n## Need\n\n",
                    ),
                    TemplateEntry::document("Location.md", "# ${{name}}\n\n"),
                ],
            ),
            TemplateEntry::folder("Trash", vec![]),
        ],
        folder_roles: HashMap::from([
            ("Manuscript".to_string(), FolderRole::Manuscript),
            ("Research".to_string(), FolderRole::Research),
            ("Templates".to_string(), FolderRole::Templates),
            ("Trash".to_string(), FolderRole::Trash),
        ]),
    }
}

/// The 5 built-in templates, Blank first — the template picker's default
/// selection, and the only one that reproduces today's "just an empty project"
/// New Project behavior exactly.
pub fn built_in_templates() -> Vec<ProjectTemplate> {
    vec![
        blank_template(),
        novel_template(),
        nonfiction_template(),
        screenplay_template(),
        world_building_template(),
    ]
}

/// The always-loaded custom-template directory: `<config_dir>/smaragd/
/// project_templates`, the same base path `color_theme::global_themes_dir` uses
/// for its own `themes` subdirectory.
pub fn global_project_templates_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "smaragd")
        .map(|dirs| dirs.config_dir().join("project_templates"))
}

/// The on-disk shape of a custom template's `template.toml`. A custom template's
/// `id` is deliberately *not* a field here — it's the `content/`-sibling
/// directory's own name (lowercased at load, see `load`), since that directory
/// already has to exist and be uniquely named, so a second id field inside the
/// file would be redundant and could drift from the directory name.
#[derive(Debug, Serialize, Deserialize, Default)]
struct RawCustomTemplate {
    label: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    folder_roles: HashMap<String, FolderRole>,
}

pub fn find<'a>(templates: &'a [ProjectTemplate], id: &str) -> Option<&'a ProjectTemplate> {
    templates.iter().find(|template| template.id == id)
}

/// Load every template: the 4 built-ins, plus every immediate subdirectory of
/// each of `dirs` that contains a `template.toml` (flat, not recursive into
/// `dirs` itself; a missing directory is silently skipped, not an error — same
/// shape and tolerance as `color_theme::load`). Subdirectories are visited in
/// sorted-name order, so load — and therefore id-collision resolution — is
/// deterministic. A subdirectory with an unreadable/malformed `template.toml`, or
/// whose derived id collides with an already-loaded template (built-in ids are
/// reserved), is skipped with a message appended to the returned error list,
/// never aborting the whole load.
pub fn load(dirs: &[&Path]) -> (Vec<ProjectTemplate>, Vec<String>) {
    let mut templates = built_in_templates();
    let mut errors = Vec::new();

    for dir in dirs {
        let Ok(read_dir) = fs::read_dir(dir) else {
            continue;
        };
        let mut subdirs: Vec<PathBuf> = read_dir
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        subdirs.sort();

        for subdir in subdirs {
            let Some(id) = subdir
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
            else {
                continue;
            };
            match load_one(&subdir, &id) {
                Ok(template) => {
                    if templates.iter().any(|existing| existing.id == template.id) {
                        errors.push(format!(
                            "{id}: a template with this id already exists, skipping"
                        ));
                        continue;
                    }
                    templates.push(template);
                }
                Err(err) => errors.push(format!("{id}: {err}")),
            }
        }
    }

    (templates, errors)
}

fn load_one(dir: &Path, id: &str) -> Result<ProjectTemplate, String> {
    let source = fs::read_to_string(dir.join("template.toml"))
        .map_err(|err| format!("couldn't read template.toml: {err}"))?;
    let raw: RawCustomTemplate = toml::from_str(&source).map_err(|err| err.to_string())?;

    let content_dir = dir.join("content");
    let entries = if content_dir.is_dir() {
        load_content_tree(&content_dir).map_err(|err| format!("couldn't read content: {err}"))?
    } else {
        Vec::new()
    };

    Ok(ProjectTemplate {
        id: id.to_string(),
        label: raw.label,
        description: raw.description,
        entries,
        folder_roles: raw.folder_roles,
    })
}

/// Recursively load `dir` (a custom template's `content/` folder, or one of its
/// subfolders) into a `TemplateEntry` tree, in sorted-name order for determinism.
fn load_content_tree(dir: &Path) -> io::Result<Vec<TemplateEntry>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if path.is_dir() {
                Ok(TemplateEntry::folder(name, load_content_tree(&path)?))
            } else {
                Ok(TemplateEntry::document(name, fs::read_to_string(&path)?))
            }
        })
        .collect()
}

impl ProjectTemplate {
    /// Stamp this template's folders/documents onto `project` — expected to be a
    /// just-`Project::initialize`d, still-empty project (called from
    /// `SmaragdApp::create_project`, before `set_project`/`ensure_starter_folders`
    /// run), so every `create_folder`/`create_document_with_content` call below is
    /// collision-free and every `folder_roles` relative-path key resolves against
    /// a folder this same call just created.
    pub fn apply(&self, project: &mut Project) -> io::Result<()> {
        let root = project.root.clone();
        Self::apply_entries(&self.entries, project, &root)?;
        for (relative_key, role) in &self.folder_roles {
            let path = if relative_key.is_empty() {
                project.root.clone()
            } else {
                project.root.join(relative_key)
            };
            project.set_folder_role(&path, Some(*role))?;
        }
        Ok(())
    }

    fn apply_entries(
        entries: &[TemplateEntry],
        project: &mut Project,
        parent: &Path,
    ) -> io::Result<()> {
        for entry in entries {
            match entry {
                TemplateEntry::Folder { name, children } => {
                    let path = project.create_folder(parent, name)?;
                    Self::apply_entries(children, project, &path)?;
                }
                TemplateEntry::Document { name, content } => {
                    project.create_document_with_content(parent, name, content)?;
                }
            }
        }
        Ok(())
    }
}

/// Save `project`'s current structure — folder/document names and content, plus
/// its Research/Trash/Templates role assignments — as a new custom template
/// labeled `label` under `templates_dir`. Returns the new template's id.
///
/// Deliberately structural only: `project.meta`'s narrative fields
/// (`story_cards`, `protagonist_desire`/`protagonist_misbelief`, `book_title`/
/// `book_author`/`book_style`, `git_enabled`/`git_prompted`, `plugins_enabled`)
/// are never copied — a template captures a *shape* to start an unrelated
/// manuscript from, not another project's actual scenes, Story Genius notes, or
/// per-project settings.
///
/// The project's Trash folder (if any) is preserved as an empty folder in the
/// template — its *contents* are excluded, since trashed material is transient
/// and shouldn't be pre-seeded into every future project stamped from this
/// template.
pub fn save_from_project(
    templates_dir: &Path,
    label: &str,
    project: &Project,
) -> io::Result<String> {
    let id = unique_template_id(templates_dir, &slugify(label));
    let dest = templates_dir.join(&id);
    let content_dir = dest.join("content");
    fs::create_dir_all(&content_dir)?;

    let trash_path = project.trash_path();
    copy_content_tree(
        project.tree.root.children(),
        &content_dir,
        trash_path.as_deref(),
    )?;

    let raw = RawCustomTemplate {
        label: label.to_string(),
        description: String::new(),
        folder_roles: project.meta.folder_roles.clone(),
    };
    let source = toml::to_string_pretty(&raw).map_err(io::Error::other)?;
    fs::write(dest.join("template.toml"), source)?;

    Ok(id)
}

/// Mirrors `content_dir`'s tree (which is `project.root`'s own tree, per
/// `BinderNode::name`) 1:1, so `project.meta.folder_roles`'s relative-path keys
/// need no translation to remain valid once copied verbatim into the new
/// template's `template.toml`.
fn copy_content_tree(
    nodes: &[BinderNode],
    dest_dir: &Path,
    trash_path: Option<&Path>,
) -> io::Result<()> {
    for node in nodes {
        let dest = dest_dir.join(&node.name);
        match &node.kind {
            BinderNodeKind::Folder { children } => {
                fs::create_dir_all(&dest)?;
                if Some(node.path.as_path()) == trash_path {
                    continue;
                }
                copy_content_tree(children, &dest, trash_path)?;
            }
            BinderNodeKind::Document => {
                fs::write(&dest, fs::read_to_string(&node.path)?)?;
            }
        }
    }
    Ok(())
}

fn slugify(label: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in label.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_dash = false;
        } else if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }
    if slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "template".to_string()
    } else {
        slug
    }
}

fn unique_template_id(templates_dir: &Path, base: &str) -> String {
    if !templates_dir.join(base).exists() {
        return base.to_string();
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{base}-{suffix}");
        if !templates_dir.join(&candidate).exists() {
            return candidate;
        }
        suffix += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn there_are_five_built_in_templates() {
        assert_eq!(built_in_templates().len(), 5);
    }

    #[test]
    fn built_in_template_ids_are_unique() {
        let ids: HashSet<_> = built_in_templates().into_iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn find_locates_a_known_template() {
        let templates = built_in_templates();
        assert_eq!(
            find(&templates, "novel").map(|t| t.label.as_str()),
            Some("Novel")
        );
    }

    #[test]
    fn find_returns_none_for_an_unknown_id() {
        let templates = built_in_templates();
        assert!(find(&templates, "does-not-exist").is_none());
    }

    #[test]
    fn load_is_just_the_built_ins_when_no_custom_template_dirs_exist() {
        let (templates, errors) = load(&[]);
        assert_eq!(templates.len(), 5);
        assert!(errors.is_empty());
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        let (templates, errors) = load(&[Path::new("/does/not/exist")]);
        assert_eq!(templates.len(), 5);
        assert!(errors.is_empty());
    }

    #[test]
    fn load_picks_up_a_valid_custom_template_directory() {
        let dir = tempfile::tempdir().unwrap();
        let template_dir = dir.path().join("my-template");
        fs::create_dir_all(template_dir.join("content/Manuscript")).unwrap();
        fs::write(
            template_dir.join("content/Manuscript/Chapter 1.md"),
            "# Chapter 1\n",
        )
        .unwrap();
        fs::write(
            template_dir.join("template.toml"),
            "label = \"My Template\"\ndescription = \"A custom layout\"\n",
        )
        .unwrap();

        let (templates, errors) = load(&[dir.path()]);

        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        let custom = find(&templates, "my-template").expect("custom template loaded");
        assert_eq!(custom.label, "My Template");
        assert_eq!(
            custom.entries,
            vec![TemplateEntry::folder(
                "Manuscript",
                vec![TemplateEntry::document("Chapter 1.md", "# Chapter 1\n")],
            )]
        );
    }

    #[test]
    fn a_custom_template_id_colliding_with_a_built_in_is_skipped_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let template_dir = dir.path().join("novel");
        fs::create_dir_all(&template_dir).unwrap();
        fs::write(
            template_dir.join("template.toml"),
            "label = \"Fake Novel\"\n",
        )
        .unwrap();

        let (templates, errors) = load(&[dir.path()]);

        assert_eq!(templates.len(), 5, "the built-in Novel should win");
        assert_eq!(
            find(&templates, "novel").map(|t| t.label.as_str()),
            Some("Novel")
        );
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn a_custom_template_missing_template_toml_is_skipped_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("broken")).unwrap();

        let (templates, errors) = load(&[dir.path()]);

        assert_eq!(templates.len(), 5);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn a_root_tagged_folder_role_round_trips_through_toml() {
        let raw = RawCustomTemplate {
            label: "Root Tagged".to_string(),
            description: String::new(),
            folder_roles: HashMap::from([(String::new(), FolderRole::Research)]),
        };
        let source = toml::to_string_pretty(&raw).expect("serializes");
        let parsed: RawCustomTemplate = toml::from_str(&source).expect("parses back");
        assert_eq!(parsed.folder_roles.get(""), Some(&FolderRole::Research));
    }

    #[test]
    fn apply_blank_template_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        blank_template().apply(&mut project).unwrap();

        assert!(project.tree.root.children().is_empty());
    }

    #[test]
    fn apply_novel_template_creates_expected_folders_and_tags_roles() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        novel_template().apply(&mut project).unwrap();

        let mut names: Vec<_> = project
            .tree
            .root
            .children()
            .iter()
            .map(|n| n.name.clone())
            .collect();
        names.sort();
        assert_eq!(names, vec!["Characters", "Manuscript", "Research", "Trash"]);
        assert_eq!(
            project.folder_role(&dir.path().join("Research")),
            Some(FolderRole::Research)
        );
        assert_eq!(
            project.folder_role(&dir.path().join("Trash")),
            Some(FolderRole::Trash)
        );
    }

    #[test]
    fn apply_world_building_template_creates_expected_folders_and_tags_roles() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        world_building_template().apply(&mut project).unwrap();

        let mut names: Vec<_> = project
            .tree
            .root
            .children()
            .iter()
            .map(|n| n.name.clone())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["Manuscript", "Research", "Templates", "Trash", "World"]
        );
        let world = dir.path().join("World");
        let mut world_children: Vec<_> = project
            .tree
            .find_by_path(&world)
            .expect("World folder created")
            .children()
            .iter()
            .map(|n| n.name.clone())
            .collect();
        world_children.sort();
        assert_eq!(world_children, vec!["Characters", "Items", "Locations"]);
        assert_eq!(
            project.folder_role(&dir.path().join("Manuscript")),
            Some(FolderRole::Manuscript)
        );
        assert_eq!(
            project.folder_role(&dir.path().join("Research")),
            Some(FolderRole::Research)
        );
        assert_eq!(
            project.folder_role(&dir.path().join("Templates")),
            Some(FolderRole::Templates)
        );
        assert_eq!(
            project.folder_role(&dir.path().join("Trash")),
            Some(FolderRole::Trash)
        );
    }

    #[test]
    fn save_from_project_excludes_trash_contents_and_narrative_meta() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        project
            .create_document_with_content(&trash, "deleted.md", "gone")
            .unwrap();
        project
            .create_document_with_content(dir.path(), "keep.md", "kept")
            .unwrap();
        project.meta.protagonist_desire = "Something".to_string();
        project
            .meta
            .story_cards
            .push(crate::project::StoryCard::new());
        project.save_metadata().unwrap();

        let templates_dir = tempfile::tempdir().unwrap();
        let id = save_from_project(templates_dir.path(), "My Template", &project).unwrap();

        let (templates, errors) = load(&[templates_dir.path()]);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        let saved = find(&templates, &id).expect("saved template loaded");

        let trash_entry = saved
            .entries
            .iter()
            .find(|e| matches!(e, TemplateEntry::Folder { name, .. } if name == "Trash"))
            .expect("Trash folder present");
        assert_eq!(
            trash_entry,
            &TemplateEntry::folder("Trash", vec![]),
            "Trash's contents must not be copied into the template"
        );
        assert!(
            saved
                .entries
                .iter()
                .any(|e| matches!(e, TemplateEntry::Document { name, .. } if name == "keep.md")),
        );
        assert_eq!(
            saved.folder_roles.get("Trash"),
            Some(&FolderRole::Trash),
            "structural role tags are preserved even though narrative fields aren't"
        );
    }
}
