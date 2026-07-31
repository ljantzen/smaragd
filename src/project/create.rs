use super::*;

impl Project {
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
}
