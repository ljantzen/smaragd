use super::*;

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

impl Project {
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

    /// Every distinct tag used anywhere in the project — frontmatter and
    /// inline alike — case-insensitively deduplicated (first-seen casing
    /// kept, the same convention `markdown::inline_tags` uses for one
    /// document), sorted alphabetically. The project's full known-tag
    /// vocabulary, independent of whatever document (if any) is currently
    /// open — backs the `:tag` command prompt's argument completion
    /// (`ui::command_prompt`).
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = Vec::new();
        for doc in self.tag_index() {
            for tag in doc.tags {
                if !tags.iter().any(|seen| seen.eq_ignore_ascii_case(&tag)) {
                    tags.push(tag);
                }
            }
        }
        tags.sort_by_key(|tag| tag.to_lowercase());
        tags
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn all_tags_collects_every_distinct_tag_sorted_alphabetically() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let a = project.create_document(dir.path(), "A").unwrap();
        let b = project.create_document(dir.path(), "B").unwrap();
        fs::write(&a, "---\ntags: [Zebra]\n---\nBody.").unwrap();
        fs::write(&b, "Inline #apple tag.").unwrap();

        assert_eq!(project.all_tags(), vec!["apple", "Zebra"]);
    }

    #[test]
    fn all_tags_deduplicates_case_insensitively_keeping_first_seen_casing() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let a = project.create_document(dir.path(), "A").unwrap();
        let b = project.create_document(dir.path(), "B").unwrap();
        fs::write(&a, "---\ntags: [Foo]\n---\nBody.").unwrap();
        fs::write(&b, "Inline #FOO tag.").unwrap();

        assert_eq!(project.all_tags(), vec!["Foo"]);
    }

    #[test]
    fn all_tags_is_empty_for_a_project_with_no_tagged_documents() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();
        fs::write(&doc, "No tags here.").unwrap();

        assert!(project.all_tags().is_empty());
    }
}
