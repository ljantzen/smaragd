//! Thin wrapper around the system `git` binary scoped
//! down to manually-triggered actions (no auto-commit timer or auto-push): the app
//! shells out to whatever `git` is on `PATH`, the same way that plugin ultimately
//! does, rather than embedding a git implementation.

use std::collections::HashSet;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub enum GitError {
    /// `git commit` succeeded in the sense that there was nothing wrong, but there
    /// were no staged changes to commit — worth distinguishing from a real failure so
    /// the caller can report it as "Nothing to commit" rather than an error.
    NothingToCommit,
    /// `git` ran but exited non-zero; the message is its stderr (or stdout, if
    /// stderr was empty), trimmed and ready to display as-is.
    CommandFailed(String),
    /// The `git` process itself couldn't be spawned (e.g. not on `PATH` after all).
    Io(io::Error),
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitError::NothingToCommit => write!(f, "nothing to commit"),
            GitError::CommandFailed(message) => write!(f, "{message}"),
            GitError::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for GitError {}

impl From<io::Error> for GitError {
    fn from(err: io::Error) -> Self {
        GitError::Io(err)
    }
}

/// Whether a `git` binary is available on `PATH` at all — checked before ever
/// offering to enable git support, since there's nothing to offer without it.
pub fn is_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Whether `root` is already a git repository (or inside one) — a `.git` entry can
/// be a directory (the common case) or a file (git worktrees), so this checks
/// existence rather than requiring a directory specifically.
pub fn is_repo(root: &Path) -> bool {
    root.join(".git").exists()
}

pub fn init(root: &Path) -> Result<(), GitError> {
    run(root, &["init"]).map(|_| ())
}

/// Stage every change under `root` and commit it with `message`. Returns
/// `GitError::NothingToCommit` (rather than a generic failure) when there was
/// nothing staged to commit, since that's an expected, non-alarming outcome of a
/// manually-triggered "commit now" action.
pub fn commit_all(root: &Path, message: &str) -> Result<(), GitError> {
    run(root, &["add", "-A"])?;
    match run(root, &["commit", "-m", message]) {
        Ok(_) => Ok(()),
        Err(GitError::CommandFailed(output))
            if output.to_lowercase().contains("nothing to commit") =>
        {
            Err(GitError::NothingToCommit)
        }
        Err(err) => Err(err),
    }
}

pub fn push(root: &Path) -> Result<(), GitError> {
    run(root, &["push"]).map(|_| ())
}

pub fn pull(root: &Path) -> Result<(), GitError> {
    run(root, &["pull"]).map(|_| ())
}

/// Every path under `root` with uncommitted changes — staged or not,
/// including untracked files — as absolute paths. Powers the Binder's
/// "modified" marker (`ui::binder_panel`); consulted only when git
/// integration is enabled, so a project that never turned git on never pays
/// for this.
pub fn status(root: &Path) -> Result<HashSet<PathBuf>, GitError> {
    let output = run(root, &["status", "--porcelain", "-z"])?;
    let mut paths = HashSet::new();
    // `-z` NUL-terminates every field instead of newline-terminating whole
    // lines, so a path containing a newline (or one git would otherwise
    // quote/escape) round-trips exactly — each entry is `XY<space>PATH`,
    // except a rename/copy (`R`/`C` in either status column), which is
    // followed by one extra field holding the path it was renamed *from*.
    let mut fields = output.stdout.split(|&b| b == 0).filter(|f| !f.is_empty());
    while let Some(entry) = fields.next() {
        if entry.len() < 3 {
            continue;
        }
        let (x, y) = (entry[0], entry[1]);
        let path = root.join(String::from_utf8_lossy(&entry[3..]).into_owned());
        paths.insert(path);
        if x == b'R' || x == b'C' || y == b'R' || y == b'C' {
            fields.next(); // the rename/copy source path, not itself a live file
        }
    }
    Ok(paths)
}

fn run(root: &Path, args: &[&str]) -> Result<std::process::Output, GitError> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if output.status.success() {
        Ok(output)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        Err(GitError::CommandFailed(message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn git_is_available_in_the_test_environment() {
        // Not a statement about every machine smaragd might run on — just a sanity
        // check that the environment these tests run in actually has git, since
        // every other test below depends on that.
        assert!(is_available());
    }

    #[test]
    fn is_repo_is_false_before_init_and_true_after() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_repo(dir.path()));

        init(dir.path()).unwrap();

        assert!(is_repo(dir.path()));
    }

    fn init_with_identity(root: &Path) {
        init(root).unwrap();
        // A fresh CI/dev environment may have no configured git identity at all,
        // which would make every commit in these tests fail — set one locally
        // (--local, not --global) so tests never depend on or mutate the
        // surrounding environment's git config.
        Command::new("git")
            .current_dir(root)
            .args(["config", "--local", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "--local", "user.name", "Smaragd Tests"])
            .output()
            .unwrap();
    }

    #[test]
    fn commit_all_commits_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        init_with_identity(dir.path());
        fs::write(dir.path().join("note.md"), "hello").unwrap();

        commit_all(dir.path(), "first commit").unwrap();

        let log = Command::new("git")
            .current_dir(dir.path())
            .args(["log", "--oneline"])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&log.stdout).contains("first commit"));
    }

    #[test]
    fn commit_all_with_no_changes_reports_nothing_to_commit() {
        let dir = tempfile::tempdir().unwrap();
        init_with_identity(dir.path());
        fs::write(dir.path().join("note.md"), "hello").unwrap();
        commit_all(dir.path(), "first commit").unwrap();

        let result = commit_all(dir.path(), "second commit");

        assert!(matches!(result, Err(GitError::NothingToCommit)));
    }

    #[test]
    fn push_without_a_configured_remote_fails_with_the_command_output() {
        let dir = tempfile::tempdir().unwrap();
        init_with_identity(dir.path());
        fs::write(dir.path().join("note.md"), "hello").unwrap();
        commit_all(dir.path(), "first commit").unwrap();

        let result = push(dir.path());

        assert!(matches!(result, Err(GitError::CommandFailed(_))));
    }

    #[test]
    fn status_is_empty_right_after_a_commit() {
        let dir = tempfile::tempdir().unwrap();
        init_with_identity(dir.path());
        fs::write(dir.path().join("note.md"), "hello").unwrap();
        commit_all(dir.path(), "first commit").unwrap();

        assert!(status(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn status_reports_an_untracked_file() {
        let dir = tempfile::tempdir().unwrap();
        init_with_identity(dir.path());
        fs::write(dir.path().join("new.md"), "brand new").unwrap();

        let dirty = status(dir.path()).unwrap();

        assert_eq!(dirty, [dir.path().join("new.md")].into_iter().collect());
    }

    #[test]
    fn status_reports_a_modified_tracked_file_but_not_an_untouched_one() {
        let dir = tempfile::tempdir().unwrap();
        init_with_identity(dir.path());
        fs::write(dir.path().join("a.md"), "a").unwrap();
        fs::write(dir.path().join("b.md"), "b").unwrap();
        commit_all(dir.path(), "first commit").unwrap();
        fs::write(dir.path().join("a.md"), "a, edited").unwrap();

        let dirty = status(dir.path()).unwrap();

        assert_eq!(dirty, [dir.path().join("a.md")].into_iter().collect());
    }

    #[test]
    fn status_reports_a_staged_file_too() {
        let dir = tempfile::tempdir().unwrap();
        init_with_identity(dir.path());
        fs::write(dir.path().join("staged.md"), "staged").unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "staged.md"])
            .output()
            .unwrap();

        let dirty = status(dir.path()).unwrap();

        assert_eq!(dirty, [dir.path().join("staged.md")].into_iter().collect());
    }
}
