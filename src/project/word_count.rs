use super::*;

impl Project {
    /// The total word count `scope` currently includes — recomputed fresh from
    /// disk every call, same convention as [`Project::backlinks`]; an unreadable
    /// document is skipped, not fatal. Can be slow for a large project (reads
    /// every tracked document's content), so callers should run it off the UI
    /// thread — see `app.rs`'s `spawn_word_count_recompute`, which mirrors how
    /// `git push`/`pull` are backgrounded.
    pub fn word_count(&self, scope: WordCountScope) -> usize {
        match scope {
            WordCountScope::ManuscriptOnly => {
                let roots = self.folder_role_paths(FolderRole::Manuscript);
                if roots.is_empty() {
                    self.word_count_from(&self.tree.root)
                } else {
                    roots
                        .iter()
                        .filter_map(|path| self.tree.find_by_path(path))
                        .map(|node| self.word_count_from(node))
                        .sum()
                }
            }
            WordCountScope::EverythingExceptTrash => self.word_count_from(&self.tree.root),
        }
    }

    /// Recursive walk behind [`Project::word_count`] — mirrors
    /// `export::gather_into`'s Trash-or-Templates skip exactly, but sums
    /// `frontmatter::count_words` instead of parsing markdown IR.
    fn word_count_from(&self, node: &BinderNode) -> usize {
        match &node.kind {
            BinderNodeKind::Document => fs::read_to_string(&node.path)
                .map(|contents| crate::frontmatter::count_words(&contents))
                .unwrap_or(0),
            BinderNodeKind::Folder { children } => {
                if matches!(
                    self.folder_role(&node.path),
                    Some(FolderRole::Trash) | Some(FolderRole::Templates)
                ) {
                    return 0;
                }
                children
                    .iter()
                    .map(|child| self.word_count_from(child))
                    .sum()
            }
        }
    }

    /// Whether the document at `path` would count toward [`Project::word_count`]
    /// under `scope` — a cheap, no-disk-I/O predicate (just path/role
    /// comparisons) used to gate the live "characters typed this session"
    /// counter (`SmaragdApp::track_char_activity`) to keystrokes made in a
    /// tracked document, without re-walking the whole project on every edit.
    pub fn is_path_tracked(&self, path: &Path, scope: WordCountScope) -> bool {
        let excluded_by_role = path
            .ancestors()
            .skip(1)
            .take_while(|ancestor| ancestor.starts_with(&self.root))
            .any(|ancestor| {
                matches!(
                    self.folder_role(ancestor),
                    Some(FolderRole::Trash) | Some(FolderRole::Templates)
                )
            });
        if excluded_by_role {
            return false;
        }
        match scope {
            WordCountScope::EverythingExceptTrash => true,
            WordCountScope::ManuscriptOnly => {
                let manuscript_paths = self.folder_role_paths(FolderRole::Manuscript);
                manuscript_paths.is_empty()
                    || manuscript_paths.iter().any(|root| path.starts_with(root))
            }
        }
    }

    /// Set the Draft Target (`None` clears it) — see
    /// [`ProjectMeta::draft_target_words`].
    pub fn set_draft_target_words(&mut self, target: Option<u32>) -> io::Result<()> {
        self.meta.draft_target_words = target;
        self.save_metadata()
    }

    /// Set the Session Target — see [`ProjectMeta::session_target_words`].
    pub fn set_session_target_words(&mut self, target: Option<u32>) -> io::Result<()> {
        self.meta.session_target_words = target;
        self.save_metadata()
    }

    /// Switch which documents count toward the Draft Target's live total — see
    /// [`WordCountScope`].
    pub fn set_word_count_scope(&mut self, scope: WordCountScope) -> io::Result<()> {
        self.meta.word_count_scope = scope;
        self.save_metadata()
    }

    /// Automatic daily rollover for the Session Target: re-baselines to
    /// `current_total` only if `session_baseline_date` isn't already today. A
    /// no-op otherwise, so it's safe to call unconditionally on every word-count
    /// recompute rather than needing its own separate trigger.
    pub fn maybe_roll_over_session(&mut self, current_total: usize) -> io::Result<()> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        if self.meta.session_baseline_date.as_deref() == Some(today.as_str()) {
            return Ok(());
        }
        self.meta.session_baseline_words = current_total as u32;
        self.meta.session_baseline_date = Some(today);
        self.save_metadata()
    }

    /// The Word Count panel's explicit "Reset Session" button — re-baselines the
    /// Session Target to `current_total` right now, unconditionally, unlike
    /// [`Project::maybe_roll_over_session`]'s same-day no-op.
    pub fn reset_session(&mut self, current_total: usize) -> io::Result<()> {
        self.meta.session_baseline_words = current_total as u32;
        self.meta.session_baseline_date = Some(chrono::Local::now().format("%Y-%m-%d").to_string());
        self.save_metadata()
    }
}
