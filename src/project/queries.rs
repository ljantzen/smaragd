use super::*;

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
