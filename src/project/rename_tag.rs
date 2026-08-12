use super::*;

impl Project {
    /// Rename `old_tag` to `new_tag` everywhere it's used in the project: every
    /// document's frontmatter `tags:` entry matching case-insensitively, and every
    /// inline `#tag` mention in document bodies (`markdown::rename_tag`). A document
    /// carrying neither is left untouched (not even rewritten byte-for-byte), the
    /// same "skip the write if nothing changed" discipline `rename_wikilinks_everywhere`
    /// follows.
    ///
    /// Unlike `rename_wikilinks_everywhere`, this is a first-class user action in its
    /// own right — triggered from the Tags dock, not a side effect of renaming
    /// something else — so it's `pub`. No `rescan` needed the way a document/folder
    /// rename needs one (the set of documents doesn't change), but the tag cache
    /// still needs to drop what it remembered about every document this touched —
    /// see `invalidate_tag_cache`, called via `&self` since the cache is a
    /// `RefCell`.
    pub fn rename_tag(&self, old_tag: &str, new_tag: &str) -> io::Result<()> {
        for doc_path in self.tree.document_paths() {
            let contents = fs::read_to_string(&doc_path)?;

            let mut meta = crate::frontmatter::parse(&contents);
            let mut frontmatter_changed = false;
            for tag in meta.tags.iter_mut() {
                if tag.eq_ignore_ascii_case(old_tag) {
                    *tag = new_tag.to_string();
                    frontmatter_changed = true;
                }
            }

            let body = crate::frontmatter::strip(&contents);
            let renamed_body = crate::markdown::rename_tag(body, old_tag, new_tag);
            if !frontmatter_changed && renamed_body.is_none() {
                continue;
            }

            let prefix = &contents[..contents.len() - body.len()];
            let merged = format!("{prefix}{}", renamed_body.as_deref().unwrap_or(body));
            let updated = if frontmatter_changed {
                crate::frontmatter::write_back(&merged, &meta)
            } else {
                merged
            };
            fs::write(&doc_path, updated)?;
        }
        self.invalidate_tag_cache();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_tag_updates_frontmatter_tags_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();
        fs::write(&doc, "---\ntags: [Old-Tag]\n---\nBody.").unwrap();

        project.rename_tag("old-tag", "new-tag").unwrap();

        let contents = fs::read_to_string(&doc).unwrap();
        assert_eq!(crate::frontmatter::parse(&contents).tags, vec!["new-tag"]);
    }

    #[test]
    fn rename_tag_updates_inline_tag_mentions_in_the_body() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();
        fs::write(&doc, "Body mentions #old-tag here.").unwrap();

        project.rename_tag("old-tag", "new-tag").unwrap();

        let contents = fs::read_to_string(&doc).unwrap();
        assert_eq!(contents, "Body mentions #new-tag here.");
    }

    #[test]
    fn rename_tag_updates_both_frontmatter_and_inline_tags_in_the_same_document() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();
        fs::write(&doc, "---\ntags: [old-tag]\n---\nAlso #old-tag inline.").unwrap();

        project.rename_tag("old-tag", "new-tag").unwrap();

        let contents = fs::read_to_string(&doc).unwrap();
        assert_eq!(crate::frontmatter::parse(&contents).tags, vec!["new-tag"]);
        assert!(crate::frontmatter::strip(&contents).contains("#new-tag"));
    }

    #[test]
    fn rename_tag_leaves_unrelated_tags_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();
        fs::write(&doc, "---\ntags: [unrelated]\n---\nBody #unrelated.").unwrap();

        project.rename_tag("old-tag", "new-tag").unwrap();

        let contents = fs::read_to_string(&doc).unwrap();
        assert_eq!(crate::frontmatter::parse(&contents).tags, vec!["unrelated"]);
        assert!(crate::frontmatter::strip(&contents).contains("#unrelated"));
    }

    #[test]
    fn rename_tag_leaves_a_document_with_neither_form_of_the_tag_byte_for_byte_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();
        let original = "---\ntype: Scene\n---\nJust prose, no tags at all.";
        fs::write(&doc, original).unwrap();

        project.rename_tag("old-tag", "new-tag").unwrap();

        assert_eq!(fs::read_to_string(&doc).unwrap(), original);
    }

    #[test]
    fn rename_tag_preserves_other_frontmatter_fields() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();
        fs::write(
            &doc,
            "---\ntype: Scene\nstatus: draft\ntags: [old-tag]\n---\nBody.",
        )
        .unwrap();

        project.rename_tag("old-tag", "new-tag").unwrap();

        let meta = crate::frontmatter::parse(&fs::read_to_string(&doc).unwrap());
        assert_eq!(meta.section_type.as_deref(), Some("Scene"));
        assert_eq!(meta.status.as_deref(), Some("draft"));
        assert_eq!(meta.tags, vec!["new-tag"]);
    }

    #[test]
    fn rename_tag_preserves_unknown_frontmatter_keys() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();
        fs::write(
            &doc,
            "---\ncustom_key: keep-me\ntags: [old-tag]\n---\nBody.",
        )
        .unwrap();

        project.rename_tag("old-tag", "new-tag").unwrap();

        let contents = fs::read_to_string(&doc).unwrap();
        assert!(contents.contains("custom_key: keep-me"));
    }

    #[test]
    fn rename_tag_does_not_rename_a_nested_tag_sharing_a_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc").unwrap();
        fs::write(&doc, "Mentions #projects/smaragd here.").unwrap();

        project.rename_tag("projects", "work").unwrap();

        let contents = fs::read_to_string(&doc).unwrap();
        assert_eq!(contents, "Mentions #projects/smaragd here.");
    }
}
