use super::*;

impl Project {
    pub fn daily_notes_enabled(&self) -> bool {
        self.meta.daily_notes_enabled
    }

    pub fn set_daily_notes_enabled(&mut self, enabled: bool) -> io::Result<()> {
        self.meta.daily_notes_enabled = enabled;
        self.save_metadata()
    }

    /// The absolute path of the configured Daily Notes folder, with any strftime
    /// codes in it expanded against today's date (e.g. "Journal/%Y/%m" ->
    /// "Journal/2026/10") — the project root when unconfigured. A plain string
    /// with no `%` codes passes through `format_date` unchanged, so this is a
    /// strict superset of a literal folder name. A leading `/` left over after
    /// expansion is stripped before joining to the root — `PathBuf::join` treats
    /// joining an absolute path as *replacing* the base entirely, which would
    /// otherwise let a folder starting with `/` escape the project. Unlike
    /// `attachments_folder`, never `None`: there's always somewhere to put
    /// today's note, and unlike that folder, this one is allowed not to exist
    /// yet (see `open_or_create_daily_note`).
    pub fn daily_notes_folder(&self) -> PathBuf {
        if self.meta.daily_notes_folder.trim().is_empty() {
            return self.root.clone();
        }
        let expanded = crate::templates::format_date(&self.meta.daily_notes_folder);
        self.root.join(expanded.trim_start_matches('/'))
    }

    /// The raw, unexpanded strftime pattern behind `daily_notes_folder`, exactly
    /// as stored — for the Settings UI's text field (as opposed to the resolved
    /// absolute path).
    pub fn daily_notes_folder_key(&self) -> &str {
        &self.meta.daily_notes_folder
    }

    /// Set the Daily Notes folder to `relative_path` — a strftime pattern, not
    /// necessarily a literal path, see `daily_notes_folder`. Stored *exactly* as
    /// given, with no trimming of its own: the Settings UI's text field is bound
    /// to this value every frame, so normalizing it here (e.g. stripping a
    /// trailing `/`) would fight live typing — someone typing `Journal/%Y` would
    /// see the `/` they just typed vanish the instant they typed it, because the
    /// next frame redraws from the now-trimmed stored value. Leading/trailing
    /// `/` and blank-vs-root handling happen only where the value is actually
    /// resolved to a path (`daily_notes_folder`), which never feeds back into
    /// this field. Still refuses an actual `..` path component outright, unlike
    /// `set_attachments_folder`'s existence check (which this deliberately
    /// skips — a not-yet-existing subfolder is valid here and is created on
    /// demand the first time a daily note is actually written): nobody needs to
    /// type `..` as an intermediate step toward a valid value, so rejecting it
    /// immediately can't fight anyone's typing the way trimming would.
    pub fn set_daily_notes_folder(&mut self, relative_path: &str) -> io::Result<()> {
        let escapes = Path::new(relative_path.trim_matches('/'))
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir));
        if escapes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "daily notes folder must be a path inside the project",
            ));
        }
        self.meta.daily_notes_folder = relative_path.to_string();
        self.save_metadata()
    }

    /// The strftime pattern a daily note's filename (sans `.md`) is formatted
    /// with — see `crate::templates::format_date`'s own fallback for a blank/invalid
    /// pattern.
    pub fn daily_note_filename_format(&self) -> &str {
        &self.meta.daily_note_filename_format
    }

    pub fn set_daily_note_filename_format(&mut self, format: String) -> io::Result<()> {
        self.meta.daily_note_filename_format = format;
        self.save_metadata()
    }

    /// The absolute path of the document used as a daily note's template, if one
    /// is configured *and* it still exists — a configured-but-deleted template
    /// degrades to `None` (a blank daily note) rather than erroring.
    pub fn daily_note_template(&self) -> Option<PathBuf> {
        let key = self.meta.daily_note_template.as_deref()?;
        let path = if key.is_empty() {
            self.root.clone()
        } else {
            self.root.join(key)
        };
        self.store.exists(&path).then_some(path)
    }

    /// Assign `path` as the daily note template (`None` clears it). Errors if
    /// `path` isn't a file inside the project — same guard shape as
    /// `set_attachments_folder`.
    pub fn set_daily_note_template(&mut self, path: Option<&Path>) -> io::Result<()> {
        if let Some(path) = path {
            if !self.store.exists(path) || self.store.is_dir(path) {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file"));
            }
            if path.strip_prefix(&self.root).is_err() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "daily note template must be inside the project",
                ));
            }
        }
        self.meta.daily_note_template = path.map(|path| relative_key(&self.root, path));
        self.save_metadata()
    }

    /// Open today's daily note, creating it first if it doesn't exist yet: an
    /// empty note, or — if a template is configured — one substituted via
    /// `crate::templates::substitute` the same way plain "New From Template" is (see
    /// `create_document_from_template`), except `${{previous_day}}`/`${{next_day}}`
    /// render using *this* daily-note filename format rather than `date_format`, so
    /// they link to what another daily note's filename will actually be. Never
    /// touches an already-existing note for today. `date_format`/`time_format` are
    /// `Settings::template_date_format`/`template_time_format`, threaded through for
    /// the same reason `create_document_from_template` already takes them.
    ///
    /// Doesn't itself check `daily_notes_enabled` — Daily Notes being off is a
    /// "don't offer this at all" UI concern (the shortcut/`:daily` refuse with a
    /// toast instead of calling this), not something this lower-level method
    /// needs to re-enforce.
    pub fn open_or_create_daily_note(
        &mut self,
        date_format: &str,
        time_format: &str,
    ) -> io::Result<(PathBuf, Option<usize>)> {
        let filename = format!(
            "{}.md",
            crate::templates::format_date(self.daily_note_filename_format())
        );
        let folder = self.daily_notes_folder();
        let path = folder.join(&filename);

        if self.store.exists(&path) {
            return Ok((path, None));
        }
        self.store.create_dir_all(&folder)?;

        match self.daily_note_template() {
            Some(template) => {
                let adjacent_day_format = self.daily_note_filename_format().to_string();
                self.create_document_from_template_with_formats(
                    &folder,
                    &filename,
                    &template,
                    &crate::templates::TemplateFormats {
                        date: date_format,
                        time: time_format,
                        adjacent_day: &adjacent_day_format,
                    },
                )
            }
            None => self
                .create_document(&folder, &filename)
                .map(|path| (path, None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_notes_enabled_is_off_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        assert!(!project.daily_notes_enabled());
    }

    #[test]
    fn set_daily_notes_enabled_toggles_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project.set_daily_notes_enabled(true).unwrap();
        assert!(project.daily_notes_enabled());

        project.set_daily_notes_enabled(false).unwrap();
        assert!(!project.daily_notes_enabled());
    }

    #[test]
    fn daily_notes_folder_defaults_to_the_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        assert_eq!(project.daily_notes_folder(), project.root);
        assert_eq!(project.daily_notes_folder_key(), "");
    }

    #[test]
    fn set_daily_notes_folder_stores_the_value_verbatim_with_no_trimming() {
        // Regression test: a previous version trimmed leading/trailing `/` at
        // write time, which fought live typing in the Settings text field —
        // typing "Journal/" toward "Journal/%Y" would see the `/` immediately
        // vanish on the next frame, because the field redraws from this stored
        // (and then-trimmed) value every frame.
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project.set_daily_notes_folder("/Journal/").unwrap();

        assert_eq!(project.daily_notes_folder_key(), "/Journal/");
    }

    #[test]
    fn daily_notes_folder_resolves_a_leading_or_trailing_slash_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project.set_daily_notes_folder("/Journal/").unwrap();

        // A leading `/` must not make `PathBuf::join` treat this as an absolute
        // path that replaces the project root outright.
        assert_eq!(project.daily_notes_folder(), dir.path().join("Journal/"));
    }

    #[test]
    fn daily_notes_folder_expands_strftime_codes_including_nested_subfolders() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project.set_daily_notes_folder("Journal/%Y/%m").unwrap();

        let today = chrono::Local::now();
        let expected = dir.path().join("Journal").join(format!(
            "{}/{}",
            today.format("%Y"),
            today.format("%m")
        ));
        assert_eq!(project.daily_notes_folder(), expected);
    }

    #[test]
    fn daily_notes_folder_leaves_a_plain_name_with_no_percent_codes_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project.set_daily_notes_folder("Journal").unwrap();

        assert_eq!(project.daily_notes_folder(), dir.path().join("Journal"));
    }

    #[test]
    fn set_daily_notes_folder_rejects_a_path_that_escapes_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        assert!(project.set_daily_notes_folder("../escape").is_err());
    }

    #[test]
    fn open_or_create_daily_note_creates_a_nested_dated_subfolder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.set_daily_notes_folder("Journal/%Y/%m").unwrap();

        let (path, _) = project
            .open_or_create_daily_note("%Y-%m-%d", "%H:%M")
            .unwrap();

        assert!(path.is_file());
        let today = chrono::Local::now();
        assert_eq!(
            path,
            dir.path()
                .join("Journal")
                .join(today.format("%Y").to_string())
                .join(today.format("%m").to_string())
                .join(format!("{}.md", today.format("%Y-%m-%d")))
        );
    }

    #[test]
    fn open_or_create_daily_note_creates_a_blank_note_in_a_not_yet_existing_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        project.set_daily_notes_folder("Journal").unwrap();
        project
            .set_daily_note_filename_format("%Y-%m-%d".to_string())
            .unwrap();

        let (path, cursor) = project
            .open_or_create_daily_note("%Y-%m-%d", "%H:%M")
            .unwrap();

        assert!(path.is_file());
        assert_eq!(
            path,
            dir.path()
                .join("Journal")
                .join(format!("{}.md", chrono::Local::now().format("%Y-%m-%d")))
        );
        assert_eq!(cursor, None);
    }

    #[test]
    fn open_or_create_daily_note_reopens_an_existing_note_without_touching_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        let (first_path, _) = project
            .open_or_create_daily_note("%Y-%m-%d", "%H:%M")
            .unwrap();
        project
            .store
            .write(&first_path, b"already written")
            .unwrap();

        let (second_path, cursor) = project
            .open_or_create_daily_note("%Y-%m-%d", "%H:%M")
            .unwrap();

        assert_eq!(first_path, second_path);
        assert_eq!(cursor, None);
        assert_eq!(
            project.store.read_to_string(&second_path).unwrap(),
            "already written"
        );
    }

    #[test]
    fn open_or_create_daily_note_substitutes_from_the_configured_template() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let templates_folder = project.create_folder(dir.path(), "Templates").unwrap();
        project
            .set_folder_role(&templates_folder, Some(FolderRole::Templates))
            .unwrap();
        let template = project
            .create_document_with_content(&templates_folder, "Daily", "# ${{name}}${{cursor}}\n")
            .unwrap();
        project.set_daily_note_template(Some(&template)).unwrap();

        let (path, cursor) = project
            .open_or_create_daily_note("%Y-%m-%d", "%H:%M")
            .unwrap();

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert_eq!(
            project.store.read_to_string(&path).unwrap(),
            format!("# {today}\n")
        );
        assert_eq!(cursor, Some(format!("# {today}").len()));
    }

    #[test]
    fn daily_note_template_falls_back_to_none_once_the_document_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let template = project.create_document(dir.path(), "Daily").unwrap();
        project.set_daily_note_template(Some(&template)).unwrap();
        assert!(project.daily_note_template().is_some());

        std::fs::remove_file(&template).unwrap();

        assert_eq!(project.daily_note_template(), None);
    }

    #[test]
    fn open_or_create_daily_note_creates_a_blank_note_when_the_configured_template_was_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let template = project.create_document(dir.path(), "Daily").unwrap();
        project.set_daily_note_template(Some(&template)).unwrap();
        std::fs::remove_file(&template).unwrap();

        let (path, cursor) = project
            .open_or_create_daily_note("%Y-%m-%d", "%H:%M")
            .unwrap();

        assert_eq!(project.store.read_to_string(&path).unwrap(), "");
        assert_eq!(cursor, None);
    }

    #[test]
    fn set_daily_note_template_rejects_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let folder = project.create_folder(dir.path(), "Folder").unwrap();

        assert!(project.set_daily_note_template(Some(&folder)).is_err());
    }

    #[test]
    fn set_daily_note_template_rejects_a_path_outside_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let file = outside.path().join("outside.md");
        std::fs::write(&file, "").unwrap();

        assert!(project.set_daily_note_template(Some(&file)).is_err());
    }

    #[test]
    fn set_daily_note_template_none_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let template = project.create_document(dir.path(), "Daily").unwrap();
        project.set_daily_note_template(Some(&template)).unwrap();

        project.set_daily_note_template(None).unwrap();

        assert_eq!(project.daily_note_template(), None);
    }
}
