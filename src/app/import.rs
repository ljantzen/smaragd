use super::*;

impl SmaragdApp {
    /// Import a `.docx` file, picked via a native file dialog, into the
    /// currently open project — split into one document per Heading 1 (see
    /// `import::docx::parse`), landing under the same destination
    /// `keyboard_new_file`/`keyboard_new_folder` already use: the currently
    /// selected document's parent folder, or the project root if nothing's
    /// selected.
    pub(super) fn import_docx(&mut self) {
        if self.project.is_none() {
            self.push_error_toast("No project open");
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Word Document", &["docx"])
            .pick_file()
        else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.push_error_toast(format!("Couldn't read {}: {err}", path.display()));
                return;
            }
        };
        let fallback_title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported Document")
            .to_string();
        let nodes = match crate::import::docx::parse(&bytes, &fallback_title) {
            Ok(nodes) => nodes,
            Err(err) => {
                self.push_error_toast(format!("Import failed: {err}"));
                return;
            }
        };

        // Computed with plain field reads, before taking `&mut self.project`
        // below — `self.selected_path`'s parent, or the project root.
        let project_root = self
            .project
            .as_ref()
            .expect("checked project.is_some() above")
            .root
            .clone();
        let destination = self
            .selected_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or(project_root);

        let Some(project) = &mut self.project else {
            return;
        };
        match crate::import::write_imported_tree(project, &destination, &nodes) {
            Ok(summary) => {
                self.set_status_message(format!(
                    "Imported {} document{} from {}",
                    summary.documents,
                    if summary.documents == 1 { "" } else { "s" },
                    path.display()
                ));
            }
            Err(err) => {
                self.push_error_toast(format!("Import failed: {err}"));
            }
        }
    }
}
