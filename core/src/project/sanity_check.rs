//! Startup sanity check: compares `.smaragd/project.json`'s path-keyed
//! metadata against what's actually in the vault right now (`Project::tree`,
//! freshly scanned off disk by `load_from_folder`/`rescan`), flagging any key
//! that still points at a document or folder that's gone — plus a second,
//! unrelated check (see `conflict_marker_issues`) for documents still
//! carrying unresolved git merge-conflict markers.
//!
//! Every path this checks is one smaragd's own create/rename/move/delete
//! routes already keep in sync on their own (see
//! `Project::rewrite_relative_key_prefix`/`permanently_delete`) — so under
//! ordinary in-app use, none of this should ever actually go stale. A
//! mismatch means something touched the vault or `project.json` outside
//! smaragd since it was last open: a file deleted or renamed by hand, a
//! `git pull`/checkout that rewound the tree, a sync merge, or (for the
//! three picklist-folder fields and `attachments_folder`, which no
//! rename/delete path rewrites yet — see each check's own comment below) a
//! perfectly ordinary in-app rename or delete of the folder they pointed to.
//!
//! Deliberately read-only and advisory: nothing here is auto-repaired. A
//! stale `node_order`/`document_created` entry is harmless clutter that the
//! next legitimate write to that map quietly overwrites, but a missing
//! picklist source or attachments folder needs a person to decide what to
//! point it at next — silently clearing it would just trade one surprise
//! for another.

use super::*;
use std::collections::HashSet;

/// Every real folder's relative key (see `relative_key`), including the
/// project root itself (`""`).
fn folder_keys(project: &Project) -> HashSet<String> {
    let mut keys = HashSet::new();
    collect_keys(&project.root, &project.tree.root, &mut keys, None);
    keys
}

/// Every real document's relative key.
fn document_keys(project: &Project) -> HashSet<String> {
    let mut keys = HashSet::new();
    collect_keys(&project.root, &project.tree.root, &mut keys, Some(true));
    keys
}

/// Walks `node`, inserting its relative key into `out` when it matches
/// `want_documents` (`None` = folders only, matching `folder_keys`'s need to
/// also count the root; `Some(true)` = documents only).
fn collect_keys(
    root: &Path,
    node: &BinderNode,
    out: &mut HashSet<String>,
    want_documents: Option<bool>,
) {
    match &node.kind {
        BinderNodeKind::Folder { children } => {
            if want_documents != Some(true) {
                out.insert(relative_key(root, &node.path));
            }
            for child in children {
                collect_keys(root, child, out, want_documents);
            }
        }
        BinderNodeKind::Document => {
            if want_documents != Some(false) {
                out.insert(relative_key(root, &node.path));
            }
        }
    }
}

fn display_key(key: &str) -> &str {
    if key.is_empty() { "/" } else { key }
}

/// Every mismatch found between `project.meta` and `project.tree` — see this
/// module's doc comment for what counts as one and why. Empty on a perfectly
/// consistent project, which is the overwhelming common case.
pub fn check(project: &Project) -> Vec<String> {
    let folders = folder_keys(project);
    let docs = document_keys(project);
    let mut issues = Vec::new();

    for key in project.meta.node_order.keys() {
        if !folders.contains(key) {
            issues.push(format!(
                "Manual ordering is recorded for folder \"{}\", which no longer exists.",
                display_key(key)
            ));
        }
    }
    for key in project.meta.folder_roles.keys() {
        if !folders.contains(key) {
            issues.push(format!(
                "A folder role is assigned to \"{}\", which no longer exists.",
                display_key(key)
            ));
        }
    }
    for key in project.meta.folder_meta.keys() {
        if !folders.contains(key) {
            issues.push(format!(
                "Folder metadata is recorded for \"{}\", which no longer exists.",
                display_key(key)
            ));
        }
    }
    // Only the *current* location is checked — `trashed_origins`' value is
    // the pre-trash location, which is supposed to no longer exist (that's
    // the whole point of having been trashed), not a sign of anything wrong.
    for (current, origin) in &project.meta.trashed_origins {
        if !folders.contains(current) && !docs.contains(current) {
            issues.push(format!(
                "A trash record for \"{}\" points at \"{}\", which no longer exists.",
                display_key(origin),
                display_key(current)
            ));
        }
    }
    for key in project.meta.document_created.keys() {
        if !docs.contains(key) {
            issues.push(format!(
                "A creation date is recorded for document \"{}\", which no longer exists.",
                display_key(key)
            ));
        }
    }
    // Unlike the maps above, nothing today rewrites or clears these three on
    // a folder rename/move/delete — so, unlike every other check here, this
    // one can also fire from a perfectly ordinary in-app action, not just
    // outside interference.
    for (label, folder) in [
        ("Type dropdown", &project.meta.type_picklist_folder),
        ("POV dropdown", &project.meta.pov_picklist_folder),
        ("Status dropdown", &project.meta.status_picklist_folder),
    ] {
        if let Some(key) = folder
            && !folders.contains(key)
        {
            issues.push(format!(
                "The {label} source folder \"{}\" no longer exists.",
                display_key(key)
            ));
        }
    }
    if project.meta.attachment_destination == attachments::AttachmentDestination::ConfiguredFolder
        && let Some(key) = &project.meta.attachments_folder
        && !folders.contains(key)
    {
        issues.push(format!(
            "The configured attachments folder \"{}\" no longer exists.",
            display_key(key)
        ));
    }

    issues.extend(conflict_marker_issues(project));
    issues
}

/// Every document still containing unresolved git merge-conflict markers
/// (`<<<<<<<`/`>>>>>>>`) — most likely a merge, rebase, or `git pull` that
/// was never finished resolving, left as plain text smaragd would otherwise
/// treat as ordinary (if garbled) prose. Requires *both* markers to actually
/// appear (as their own line's first seven characters, git's own format)
/// before flagging a document, not just one: `<<<<<<<`/`>>>>>>>` have no
/// legitimate meaning in prose, but a lone `=======` is also valid Markdown
/// (a setext H1 underline) — deliberately not checked on its own, only
/// alongside the pair that actually brackets a real conflict. An unreadable
/// (non-UTF-8, permission-denied, ...) document is silently skipped, same
/// as every other best-effort read in this module.
fn conflict_marker_issues(project: &Project) -> Vec<String> {
    let mut paths = Vec::new();
    collect_document_paths(&project.tree.root, &mut paths);
    paths
        .into_iter()
        .filter_map(|path| {
            let contents = project.store.read_to_string(&path).ok()?;
            let has_ours = contents.lines().any(|line| line.starts_with("<<<<<<<"));
            let has_theirs = contents.lines().any(|line| line.starts_with(">>>>>>>"));
            (has_ours && has_theirs).then(|| {
                format!(
                    "\"{}\" still has unresolved git merge-conflict markers in it.",
                    display_key(&relative_key(&project.root, &path))
                )
            })
        })
        .collect()
}

fn collect_document_paths(node: &BinderNode, out: &mut Vec<PathBuf>) {
    match &node.kind {
        BinderNodeKind::Folder { children } => {
            for child in children {
                collect_document_paths(child, out);
            }
        }
        BinderNodeKind::Document => out.push(node.path.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_freshly_initialized_project_has_no_issues() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        assert_eq!(check(&project), Vec::<String>::new());
    }

    #[test]
    fn a_folder_deleted_outside_smaragd_orphans_its_role_order_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Chapter 1").unwrap();
        project
            .set_folder_role(&folder, Some(FolderRole::Research))
            .unwrap();
        project.create_document(&folder, "Scene 1").unwrap();

        // Simulate the folder having been removed by something other than
        // smaragd (e.g. by hand, or a `git checkout` of an older commit) —
        // bypassing every cleanup `Project::delete` would normally do.
        fs::remove_dir_all(&folder).unwrap();
        project.rescan();

        let issues = check(&project);
        assert_eq!(issues.len(), 3, "{issues:?}");
        assert!(issues.iter().any(|i| i.contains("Manual ordering")));
        assert!(issues.iter().any(|i| i.contains("folder role")));
        assert!(issues.iter().any(|i| i.contains("creation date")));
    }

    #[test]
    fn a_trash_record_whose_current_location_is_gone_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let doc = project.create_document(dir.path(), "Doomed").unwrap();
        project.delete(&doc).unwrap();

        // The trashed copy is removed by something other than smaragd's own
        // Empty Trash/Delete, leaving `trashed_origins` pointing at nothing.
        fs::remove_file(trash.join("Doomed.md")).unwrap();
        project.rescan();

        let issues = check(&project);
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(
            issues
                .iter()
                .any(|i| i.contains("trash record") && i.contains("Doomed.md"))
        );
        assert!(issues.iter().any(|i| i.contains("creation date")));
    }

    #[test]
    fn a_picklist_folder_deleted_in_app_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let types = project.create_folder(dir.path(), "Types").unwrap();
        project
            .set_picklist_folder(PicklistField::Type, Some(&types))
            .unwrap();

        // Unlike the folder-role/node-order case above, nothing rewrites
        // `type_picklist_folder` on delete today — so this reproduces even
        // through smaragd's own `Project::delete`.
        project.delete(&types).unwrap();

        let issues = check(&project);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].contains("Type dropdown"));
    }

    #[test]
    fn an_unconfigured_attachments_folder_is_not_flagged_even_if_it_once_existed() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        // Set while `SameAsDocument` (the default) is still active, then
        // the folder removed — `attachments_folder` is only meaningful once
        // `ConfiguredFolder` is actually selected.
        project.meta.attachments_folder = Some("Attachments".to_string());

        assert_eq!(check(&project), Vec::<String>::new());
    }

    #[test]
    fn a_document_with_both_conflict_markers_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project
            .create_document_with_content(
                dir.path(),
                "Scene 1",
                "Before.\n<<<<<<< HEAD\nMine.\n=======\nTheirs.\n>>>>>>> origin/main\nAfter.\n",
            )
            .unwrap();

        let issues = check(&project);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].contains("Scene 1.md"));
        assert!(issues[0].contains("merge-conflict"));
    }

    #[test]
    fn a_lone_setext_heading_underline_is_not_mistaken_for_a_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project
            .create_document_with_content(dir.path(), "Scene 1", "Title\n=======\nBody text.\n")
            .unwrap();

        assert_eq!(check(&project), Vec::<String>::new());
    }

    #[test]
    fn mentioning_only_one_marker_in_prose_is_not_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project
            .create_document_with_content(
                dir.path(),
                "Scene 1",
                "A git conflict starts with a line like:\n<<<<<<< HEAD\n",
            )
            .unwrap();

        assert_eq!(check(&project), Vec::<String>::new());
    }
}
