//! Zip-based project backups, modeled after Scrivener's own backup scheme: a
//! timestamped snapshot of the whole project folder written to a shared backup
//! directory (not inside the project itself, so it survives the project folder
//! being deleted or moved), with the oldest snapshots pruned once more than a
//! configured number accumulate. Pure file-system logic only — `app::backup`
//! decides *when* to call this (on project open/close/manual save, per
//! `Settings`) and turns the result into UI feedback.

use std::fmt;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

#[derive(Debug)]
pub enum BackupError {
    Io(io::Error),
    Zip(zip::result::ZipError),
    Walk(ignore::Error),
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackupError::Io(err) => write!(f, "{err}"),
            BackupError::Zip(err) => write!(f, "{err}"),
            BackupError::Walk(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for BackupError {}

impl From<io::Error> for BackupError {
    fn from(err: io::Error) -> Self {
        BackupError::Io(err)
    }
}

impl From<zip::result::ZipError> for BackupError {
    fn from(err: zip::result::ZipError) -> Self {
        BackupError::Zip(err)
    }
}

impl From<ignore::Error> for BackupError {
    fn from(err: ignore::Error) -> Self {
        BackupError::Walk(err)
    }
}

/// The default shared backup directory — every project's backups land here,
/// disambiguated by filename prefix, the same "one shared location, not
/// per-project" convention Scrivener's own default backup folder uses. `None`
/// if the platform's data directory can't be determined (mirrors every other
/// `directories`-based path in `settings.rs`, which all use `config_dir()`
/// instead — `data_dir()` here since these are archive files, not
/// configuration).
pub fn default_backup_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "smaragd").map(|dirs| dirs.data_dir().join("backups"))
}

/// Zip `project_root` into a timestamped archive under `backup_dir`, named
/// `{project_name}-{YYYY-MM-DD-HHMMSS}.zip`. Creates `backup_dir` if it
/// doesn't exist yet. Skips `.git` entirely (redundant with git's own
/// history, and often the single largest thing under a project root) but
/// otherwise includes everything under `project_root` not excluded by a
/// `.gitignore`/`.ignore` file — including `.smaragd/`, unlike
/// `project::scan::scan_project`'s walk, since that metadata is exactly what
/// restoring from a backup needs.
pub fn create_backup(
    project_root: &Path,
    backup_dir: &Path,
    project_name: &str,
) -> Result<PathBuf, BackupError> {
    std::fs::create_dir_all(backup_dir)?;
    let timestamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
    let zip_path = backup_dir.join(format!("{project_name}-{timestamp}.zip"));

    let file = File::create(&zip_path)?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let walker = WalkBuilder::new(project_root)
        .hidden(false)
        .require_git(false)
        .filter_entry(|entry| entry.file_name().to_str() != Some(".git"))
        .build();

    for entry in walker {
        let entry = entry?;
        let path = entry.path();
        if path == project_root {
            continue;
        }
        let relative = path.strip_prefix(project_root).unwrap_or(path);
        // Zip entries always use `/`, regardless of platform.
        let relative_name: String = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");

        match entry.file_type() {
            Some(ft) if ft.is_dir() => {
                writer.add_directory(format!("{relative_name}/"), options)?;
            }
            Some(ft) if ft.is_file() => {
                writer.start_file(relative_name, options)?;
                let mut source = File::open(path)?;
                io::copy(&mut source, &mut writer)?;
            }
            // Symlinks and anything else without a plain file type are
            // skipped — same "don't transparently follow a symlink into
            // who-knows-where" reasoning `scan_project` uses.
            _ => continue,
        }
    }
    writer.finish()?;
    Ok(zip_path)
}

/// Delete the oldest backups for `project_name` under `backup_dir` beyond the
/// newest `keep`, identified by the same `{project_name}-...` filename prefix
/// `create_backup` writes — so pruning one project's backups never touches
/// another's in the same shared directory. `keep` of `0` deletes every
/// existing backup for this project; callers resolving "not yet configured"
/// should do so before calling this (see `Settings::resolve_backup_keep_count`).
pub fn prune_old_backups(backup_dir: &Path, project_name: &str, keep: usize) -> io::Result<()> {
    let prefix = format!("{project_name}-");
    let mut backups: Vec<PathBuf> = std::fs::read_dir(backup_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("zip")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
        })
        .collect();
    // Filename timestamps sort lexicographically in chronological order
    // (`YYYY-MM-DD-HHMMSS`), so a plain descending sort is enough — no need
    // to stat each file's mtime.
    backups.sort_unstable_by(|a, b| b.cmp(a));
    for stale in backups.into_iter().skip(keep) {
        std::fs::remove_file(stale)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_zip_names(path: &Path) -> Vec<String> {
        let file = File::open(path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect()
    }

    #[test]
    fn backup_includes_smaragd_metadata_and_markdown_files() {
        let project_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project_dir.path().join(".smaragd")).unwrap();
        std::fs::write(project_dir.path().join(".smaragd/project.json"), "{}").unwrap();
        std::fs::write(project_dir.path().join("Chapter 1.md"), "Once upon a time").unwrap();

        let backup_dir = tempfile::tempdir().unwrap();
        let zip_path = create_backup(project_dir.path(), backup_dir.path(), "my-novel").unwrap();

        let names = read_zip_names(&zip_path);
        assert!(names.contains(&"Chapter 1.md".to_string()));
        assert!(names.iter().any(|n| n.contains(".smaragd")));
    }

    #[test]
    fn backup_excludes_the_git_directory() {
        let project_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project_dir.path().join(".git")).unwrap();
        std::fs::write(project_dir.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();
        std::fs::write(project_dir.path().join("Chapter 1.md"), "text").unwrap();

        let backup_dir = tempfile::tempdir().unwrap();
        let zip_path = create_backup(project_dir.path(), backup_dir.path(), "my-novel").unwrap();

        let names = read_zip_names(&zip_path);
        assert!(names.iter().all(|n| !n.contains(".git")));
    }

    #[test]
    fn backup_respects_gitignore() {
        let project_dir = tempfile::tempdir().unwrap();
        std::fs::write(project_dir.path().join(".gitignore"), "ignored.md\n").unwrap();
        std::fs::write(project_dir.path().join("ignored.md"), "secret").unwrap();
        std::fs::write(project_dir.path().join("kept.md"), "keep me").unwrap();

        let backup_dir = tempfile::tempdir().unwrap();
        let zip_path = create_backup(project_dir.path(), backup_dir.path(), "my-novel").unwrap();

        let names = read_zip_names(&zip_path);
        assert!(names.contains(&"kept.md".to_string()));
        assert!(!names.contains(&"ignored.md".to_string()));
    }

    #[test]
    fn backup_filename_is_prefixed_with_the_project_name() {
        let project_dir = tempfile::tempdir().unwrap();
        std::fs::write(project_dir.path().join("a.md"), "x").unwrap();
        let backup_dir = tempfile::tempdir().unwrap();

        let zip_path = create_backup(project_dir.path(), backup_dir.path(), "my-novel").unwrap();

        assert!(
            zip_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("my-novel-")
        );
    }

    #[test]
    fn prune_keeps_only_the_newest_n_backups_for_that_project() {
        let backup_dir = tempfile::tempdir().unwrap();
        for name in [
            "my-novel-2026-01-01-000000.zip",
            "my-novel-2026-01-02-000000.zip",
            "my-novel-2026-01-03-000000.zip",
            // A different project sharing the same directory must be left alone.
            "other-project-2026-01-01-000000.zip",
        ] {
            std::fs::write(backup_dir.path().join(name), b"").unwrap();
        }

        prune_old_backups(backup_dir.path(), "my-novel", 2).unwrap();

        let mut remaining: Vec<String> = std::fs::read_dir(backup_dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        remaining.sort();
        assert_eq!(
            remaining,
            vec![
                "my-novel-2026-01-02-000000.zip",
                "my-novel-2026-01-03-000000.zip",
                "other-project-2026-01-01-000000.zip",
            ]
        );
    }

    #[test]
    fn prune_is_a_no_op_when_at_or_under_the_limit() {
        let backup_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            backup_dir.path().join("my-novel-2026-01-01-000000.zip"),
            b"",
        )
        .unwrap();

        prune_old_backups(backup_dir.path(), "my-novel", 5).unwrap();

        assert_eq!(std::fs::read_dir(backup_dir.path()).unwrap().count(), 1);
    }
}
