//! Thin wrapper around the system `git` binary: the app shells out to whatever
//! `git` is on `PATH` rather than embedding a git implementation. Commit/push/
//! pull can be triggered manually or, per project, on an interval (see
//! `app::auto_commit`).

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

/// What [`push`]/[`pull`] resolve to — named so `app::SmaragdApp::pending_git`'s
/// background-thread channel doesn't need to spell out the full nested type
/// (clippy's `type_complexity` lint).
pub type PushOrPullResult = Result<Vec<ChangedFile>, GitError>;

/// Pushes the current branch, returning which files it actually sent — every
/// file touched by a commit between the upstream branch's position just
/// before the push and the local `HEAD` it just pushed there. `Ok(vec![])`
/// (not an error) when there's no upstream to diff against yet (e.g. this is
/// the very first push on a branch git hasn't started tracking) — the push
/// itself can still have succeeded with nothing to report a file list for.
pub fn push(root: &Path) -> PushOrPullResult {
    let before_upstream = rev_parse(root, "@{u}");
    run(root, &["push"])?;
    Ok(match before_upstream {
        Some(before) => diff_name_status(root, &before, "HEAD").unwrap_or_default(),
        None => Vec::new(),
    })
}

/// Pulls into the current branch, returning which files it actually brought
/// in — every file touched by a commit between `HEAD`'s position just before
/// the pull and where it ended up. `Ok(vec![])` when there was no prior
/// `HEAD` to diff against (a brand-new repo with no commits yet).
pub fn pull(root: &Path) -> PushOrPullResult {
    let before_head = rev_parse(root, "HEAD");
    run(root, &["pull"])?;
    Ok(match before_head {
        Some(before) => diff_name_status(root, &before, "HEAD").unwrap_or_default(),
        None => Vec::new(),
    })
}

/// Resolves `rev` (e.g. `"HEAD"`, `"@{u}"`) to a commit hash, or `None` if it
/// doesn't exist yet (no commits, or no upstream configured) — a missing rev
/// is an expected, common case here, not a failure worth surfacing.
fn rev_parse(root: &Path, rev: &str) -> Option<String> {
    let output = run(root, &["rev-parse", rev]).ok()?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Files that differ between `from` and `to` (two commit-ish revs), classified
/// the same way `changed_files` classifies a *working-tree* diff — both
/// produce the same [`ChangedFile`], so [`file_list_lines`] renders either the
/// same way. `None` on any failure (e.g. `from` is a hash the local repo has
/// since garbage-collected): best-effort, since this only feeds an
/// informational file list, never something a push/pull's own success
/// depends on.
fn diff_name_status(root: &Path, from: &str, to: &str) -> Option<Vec<ChangedFile>> {
    let output = run(root, &["diff", "--name-status", from, to]).ok()?;
    let mut files: Vec<ChangedFile> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let status = fields.next()?;
            let kind = match status.as_bytes().first()? {
                b'A' => FileChangeKind::Added,
                b'D' => FileChangeKind::Deleted,
                _ => FileChangeKind::Changed, // M, R, C, T, U, ...
            };
            // A rename/copy (`R100`/`C100`) has an extra "old path" field
            // before the current one; taking the last field either way lands
            // on the file's current location.
            let path = fields.next_back()?;
            Some(ChangedFile {
                kind,
                path: root.join(path),
            })
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Some(files)
}

/// Every path under `root` with uncommitted changes — staged or not,
/// including untracked files — as absolute paths. Powers the Binder's
/// "modified" marker (`ui::binder_panel`); consulted only when git
/// integration is enabled, so a project that never turned git on never pays
/// for this.
pub fn status(root: &Path) -> Result<HashSet<PathBuf>, GitError> {
    Ok(porcelain_entries(root)?
        .into_iter()
        .map(|(_, _, path)| path)
        .collect())
}

/// `(index_status, worktree_status, path)` for every entry `git status
/// --porcelain -z` reports — the shared low-level parse `status`,
/// `untracked_paths` (via `diff_stat`), and `changed_files` all build on.
/// `-z` NUL-terminates every field instead of newline-terminating whole
/// lines, so a path containing a newline (or one git would otherwise
/// quote/escape) round-trips exactly — each entry is `XY<space>PATH`,
/// except a rename/copy (`R`/`C` in either status column), which is
/// followed by one extra field holding the path it was renamed *from*.
fn porcelain_entries(root: &Path) -> Result<Vec<(u8, u8, PathBuf)>, GitError> {
    let output = run(root, &["status", "--porcelain", "-z"])?;
    let mut entries = Vec::new();
    let mut fields = output.stdout.split(|&b| b == 0).filter(|f| !f.is_empty());
    while let Some(entry) = fields.next() {
        if entry.len() < 3 {
            continue;
        }
        let (x, y) = (entry[0], entry[1]);
        let path = root.join(String::from_utf8_lossy(&entry[3..]).into_owned());
        entries.push((x, y, path));
        if x == b'R' || x == b'C' || y == b'R' || y == b'C' {
            fields.next(); // the rename/copy source path, not itself a live file
        }
    }
    Ok(entries)
}

/// One commit from `log`'s history — the Version Activity panel's commit list
/// (`ui::version_activity_panel`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitLogEntry {
    pub hash: String,
    pub author: String,
    /// Git's own human-phrased relative date (`%ar`, e.g. "2 hours ago") —
    /// reused as-is rather than computed here, so it always agrees with what
    /// `git log` itself would print.
    pub relative_date: String,
    pub subject: String,
}

/// The `limit` most recent commits on the current branch, newest first.
/// Best-effort like `diff_stat`: a brand-new repo with no commits yet (or any
/// other `git log` failure) is reported as an empty list rather than an
/// error — this is informational display, not a user-triggered action with
/// its own error to surface.
pub fn log(root: &Path, limit: usize) -> Vec<CommitLogEntry> {
    let Ok(output) = run(
        root,
        &[
            "log",
            "-n",
            &limit.to_string(),
            // `\x1f` (unit separator) between fields: `%s` (subject) is
            // guaranteed single-line, so a plain newline safely separates
            // records without needing `-z`-style NUL framing the way
            // `status`'s paths do.
            "--pretty=format:%H\u{1f}%an\u{1f}%ar\u{1f}%s",
        ],
    ) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, '\u{1f}');
            Some(CommitLogEntry {
                hash: fields.next()?.to_string(),
                author: fields.next()?.to_string(),
                relative_date: fields.next()?.to_string(),
                subject: fields.next()?.to_string(),
            })
        })
        .collect()
}

/// One thing smaragd itself did (or tried to do) via git — distinct from
/// `log`'s output (git's own commit history): this also records no-ops
/// ("Nothing to commit") and push/pull outcomes, and covers automatic
/// (`app::auto_commit`) as well as manual actions. Populated by `app::git`/
/// `app::auto_commit` as each action completes; rendered by
/// `ui::version_activity_panel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitActivityEntry {
    /// Seconds since the Unix epoch, to phrase relative times the same way
    /// `ui::sync_panel::humanize_age` does.
    pub at_unix: u64,
    pub message: String,
    pub outcome: GitActivityOutcome,
    /// Files a push/pull actually sent or brought in — already relative-path
    /// display lines (see `file_list_lines`), same "pre-formatted for
    /// display" convention as `ui::version_activity_panel::VersionActivityData::
    /// dirty_files`. Empty for every other kind of activity (commits, "git
    /// support enabled", a failure with nothing meaningful to list).
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitActivityOutcome {
    Success,
    /// Neither a success nor a failure — e.g. "Nothing to commit."
    Neutral,
    Error,
}

/// How much a project's working tree differs from `HEAD`, for commit-message
/// placeholders (see `render_commit_message`). Best-effort: a brand-new repo
/// with no commits yet (no `HEAD` to diff against) is reported as zero tracked
/// changes rather than an error, same philosophy as `status()`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DiffStat {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

/// `diff_stat` for `root`: tracked changes come from `git diff --shortstat HEAD`;
/// untracked files aren't covered by that (there's nothing in `HEAD` to diff them
/// against), so each is counted as one changed file with its on-disk line count
/// added to `insertions` — read directly rather than needing a git call per file.
pub fn diff_stat(root: &Path) -> Result<DiffStat, GitError> {
    let mut stat = match run(root, &["diff", "--shortstat", "HEAD"]) {
        Ok(output) => parse_shortstat(&String::from_utf8_lossy(&output.stdout)),
        Err(_) => DiffStat::default(),
    };
    for path in untracked_paths(root)? {
        let lines = std::fs::read_to_string(&path)
            .map(|content| content.lines().count())
            .unwrap_or(0);
        stat.files_changed += 1;
        stat.insertions += lines;
    }
    Ok(stat)
}

/// Parses `git diff --shortstat`'s single summary line, e.g.
/// `" 2 files changed, 10 insertions(+), 3 deletions(-)"`. Tolerant of any
/// subset of the three parts being absent (a diff with only insertions omits
/// the deletions clause entirely, and vice versa) and of empty input (no
/// changes at all).
fn parse_shortstat(text: &str) -> DiffStat {
    let mut stat = DiffStat::default();
    for part in text.trim().split(',') {
        let part = part.trim();
        let Some((num, rest)) = part.split_once(' ') else {
            continue;
        };
        let Ok(n) = num.parse::<usize>() else {
            continue;
        };
        if rest.contains("file") {
            stat.files_changed = n;
        } else if rest.contains("insertion") {
            stat.insertions = n;
        } else if rest.contains("deletion") {
            stat.deletions = n;
        }
    }
    stat
}

/// Every untracked (`??`) path reported by `git status --porcelain -z` — a
/// narrower cousin of `status()`, which reports every kind of dirty path but
/// discards which kind each one was.
fn untracked_paths(root: &Path) -> Result<Vec<PathBuf>, GitError> {
    Ok(porcelain_entries(root)?
        .into_iter()
        .filter(|(x, y, _)| *x == b'?' && *y == b'?')
        .map(|(_, _, path)| path)
        .collect())
}

/// Which of the three buckets a changed file falls into — deliberately
/// coarser than git's own status letters (M/R/C/T/U all collapse into
/// `Changed`): the `{{fileList}}` commit-message placeholder lists "added,
/// changed or deleted" files, not a full status-letter legend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    Added,
    Changed,
    Deleted,
}

impl FileChangeKind {
    pub fn letter(self) -> &'static str {
        match self {
            FileChangeKind::Added => "A",
            FileChangeKind::Changed => "M",
            FileChangeKind::Deleted => "D",
        }
    }
}

/// One file `changed_files` reports — an absolute path, same convention as
/// `status()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub kind: FileChangeKind,
    pub path: PathBuf,
}

/// Every dirty file under `root`, classified and sorted by path — the data
/// source for the `{{fileList}}` commit-message placeholder (see
/// `format_file_list`) and for a richer file-by-file view than `status()`'s
/// plain path set.
pub fn changed_files(root: &Path) -> Result<Vec<ChangedFile>, GitError> {
    let mut files: Vec<ChangedFile> = porcelain_entries(root)?
        .into_iter()
        .map(|(x, y, path)| ChangedFile {
            kind: classify_change(x, y),
            path,
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Maps a porcelain entry's index/worktree status letters onto
/// [`FileChangeKind`]. Untracked (`??`) is `Added`; otherwise `D` on either
/// side wins over `A` (a path can't usefully be "added" and "deleted" at
/// once, but prioritizing deletion matches what a user most needs to notice),
/// and everything else (`M`, `R`, `C`, `T`, `U`, ...) falls back to `Changed`.
fn classify_change(x: u8, y: u8) -> FileChangeKind {
    if x == b'?' && y == b'?' {
        FileChangeKind::Added
    } else if x == b'D' || y == b'D' {
        FileChangeKind::Deleted
    } else if x == b'A' || y == b'A' {
        FileChangeKind::Added
    } else {
        FileChangeKind::Changed
    }
}

/// Renders `files` as one `"<letter> <path relative to root>"` line each,
/// sorted — the shared formatting both `format_file_list` (one joined string,
/// for the `{{fileList}}` commit-message placeholder) and
/// `GitActivityEntry::files` (one `Vec` entry per line, for the Version
/// Activity panel's per-push/pull file list) build on.
pub fn file_list_lines(root: &Path, files: &[ChangedFile]) -> Vec<String> {
    files
        .iter()
        .map(|file| {
            let relative = file.path.strip_prefix(root).unwrap_or(&file.path);
            format!("{} {}", file.kind.letter(), relative.display())
        })
        .collect()
}

/// `file_list_lines`, newline-joined — empty when `files` is empty, so a
/// commit-message template with `{{fileList}}` in its body doesn't leave a
/// stray blank line behind.
pub fn format_file_list(root: &Path, files: &[ChangedFile]) -> String {
    file_list_lines(root, files).join("\n")
}

/// Everything a commit-message template's placeholders resolve from — see
/// `render_commit_message`. A struct rather than more positional arguments:
/// `render_commit_message` was already at five before `{{fileList}}` added a
/// sixth.
pub struct CommitContext<'a> {
    pub date: &'a str,
    pub time: &'a str,
    /// Dirty-file count for `{{numFiles}}` — cheap, usually already known by
    /// the caller, so it's taken as-is rather than derived from `files`
    /// (which may itself be an empty placeholder run, e.g. the Settings
    /// preview's illustrative sample).
    pub num_files: usize,
    pub diff: Option<&'a DiffStat>,
    /// For `{{fileList}}` — pass `&[]` when there's nothing to list (a fresh
    /// repo, or the caller didn't fetch `changed_files`).
    pub files: &'a [ChangedFile],
    /// The project root `files`' paths are relative to, for `{{fileList}}`'s
    /// display (see `format_file_list`).
    pub root: &'a Path,
}

/// Fills in a commit-message template: `{{date}}`, `{{time}}`, `{{numFiles}}`,
/// `{{linesAdded}}` / `{{linesDeleted}}` (from `ctx.diff`), `{{linesChanged}}`
/// (`insertions + deletions` — git itself has no single "changed" stat; this
/// is smaragd's own derived total line-level churn), and `{{fileList}}` (see
/// `format_file_list`). Every `lines*` placeholder resolves to `"0"` when
/// `ctx.diff` is `None` (e.g. `diff_stat` wasn't run, or failed) rather than
/// leaking the literal placeholder text into the commit message.
pub fn render_commit_message(template: &str, ctx: &CommitContext) -> String {
    let (added, deleted, changed) = match ctx.diff {
        Some(stat) => (
            stat.insertions,
            stat.deletions,
            stat.insertions + stat.deletions,
        ),
        None => (0, 0, 0),
    };
    template
        .replace("{{date}}", ctx.date)
        .replace("{{time}}", ctx.time)
        .replace("{{numFiles}}", &ctx.num_files.to_string())
        .replace("{{linesAdded}}", &added.to_string())
        .replace("{{linesDeleted}}", &deleted.to_string())
        .replace("{{linesChanged}}", &changed.to_string())
        .replace("{{fileList}}", &format_file_list(ctx.root, ctx.files))
}

/// The current local time as `HH:MM` (24-hour) for `{{time}}` — a fixed
/// format, unlike `{{date}}`, which uses whatever strftime pattern
/// `Settings::template_date_format` is configured with.
pub fn format_commit_time() -> String {
    chrono::Local::now().format("%H:%M").to_string()
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

    /// A bare "remote" repository plus a clone of it — fully local, so push/pull
    /// can be exercised end to end (including the upstream tracking a fresh
    /// `git clone` sets up automatically) without any network access.
    struct RemoteAndClone {
        _remote_dir: tempfile::TempDir,
        clone_dir: tempfile::TempDir,
    }

    fn remote_and_clone() -> RemoteAndClone {
        let remote_dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--bare", "-b", "main"])
            .current_dir(remote_dir.path())
            .output()
            .unwrap();

        // Seed the remote with an initial commit from a throwaway working
        // copy, so cloning it below lands on a real branch (cloning a bare
        // repo with no commits yet leaves the clone on an unborn branch,
        // which a plain `git push`/`git pull` can't exercise the same way).
        let seed_dir = tempfile::tempdir().unwrap();
        init_with_identity(seed_dir.path());
        fs::write(seed_dir.path().join("README.md"), "seed").unwrap();
        commit_all(seed_dir.path(), "seed commit").unwrap();
        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                remote_dir.path().to_str().unwrap(),
            ])
            .current_dir(seed_dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["push", "origin", "HEAD:main"])
            .current_dir(seed_dir.path())
            .output()
            .unwrap();

        let clone_dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args([
                "clone",
                remote_dir.path().to_str().unwrap(),
                clone_dir.path().to_str().unwrap(),
            ])
            .output()
            .unwrap();
        init_with_identity(clone_dir.path());

        RemoteAndClone {
            _remote_dir: remote_dir,
            clone_dir,
        }
    }

    #[test]
    fn push_reports_the_files_a_new_commit_sent() {
        let setup = remote_and_clone();
        fs::write(setup.clone_dir.path().join("new.md"), "brand new").unwrap();
        commit_all(setup.clone_dir.path(), "add new.md").unwrap();

        let files = push(setup.clone_dir.path()).unwrap();

        assert_eq!(
            files,
            vec![ChangedFile {
                kind: FileChangeKind::Added,
                path: setup.clone_dir.path().join("new.md"),
            }]
        );
    }

    #[test]
    fn push_with_nothing_new_reports_no_files() {
        let setup = remote_and_clone();

        let files = push(setup.clone_dir.path()).unwrap();

        assert_eq!(files, Vec::new());
    }

    #[test]
    fn pull_reports_the_files_it_brought_in() {
        let setup = remote_and_clone();
        // A second clone plays "someone else," pushing a new commit the first
        // clone then pulls.
        let other_dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args([
                "clone",
                setup._remote_dir.path().to_str().unwrap(),
                other_dir.path().to_str().unwrap(),
            ])
            .output()
            .unwrap();
        init_with_identity(other_dir.path());
        fs::write(other_dir.path().join("elsewhere.md"), "from someone else").unwrap();
        commit_all(other_dir.path(), "add elsewhere.md").unwrap();
        push(other_dir.path()).unwrap();

        let files = pull(setup.clone_dir.path()).unwrap();

        assert_eq!(
            files,
            vec![ChangedFile {
                kind: FileChangeKind::Added,
                path: setup.clone_dir.path().join("elsewhere.md"),
            }]
        );
    }

    #[test]
    fn pull_with_nothing_new_reports_no_files() {
        let setup = remote_and_clone();

        let files = pull(setup.clone_dir.path()).unwrap();

        assert_eq!(files, Vec::new());
    }

    #[test]
    fn file_list_lines_renders_one_line_per_file_relative_to_root() {
        let root = Path::new("/project");
        let files = vec![
            ChangedFile {
                kind: FileChangeKind::Deleted,
                path: root.join("old.md"),
            },
            ChangedFile {
                kind: FileChangeKind::Added,
                path: root.join("Chapters/new.md"),
            },
        ];

        assert_eq!(
            file_list_lines(root, &files),
            vec!["D old.md".to_string(), "A Chapters/new.md".to_string()]
        );
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

    #[test]
    fn log_is_empty_before_any_commit_exists() {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path()).unwrap();

        assert_eq!(log(dir.path(), 20), Vec::new());
    }

    #[test]
    fn log_returns_recent_commits_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        init_with_identity(dir.path());
        fs::write(dir.path().join("a.md"), "a").unwrap();
        commit_all(dir.path(), "first commit").unwrap();
        fs::write(dir.path().join("b.md"), "b").unwrap();
        commit_all(dir.path(), "second commit").unwrap();

        let entries = log(dir.path(), 20);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].subject, "second commit");
        assert_eq!(entries[1].subject, "first commit");
        assert_eq!(entries[0].author, "Smaragd Tests");
        assert!(!entries[0].hash.is_empty());
        assert!(!entries[0].relative_date.is_empty());
    }

    #[test]
    fn log_respects_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        init_with_identity(dir.path());
        for n in 0..5 {
            fs::write(dir.path().join("a.md"), n.to_string()).unwrap();
            commit_all(dir.path(), &format!("commit {n}")).unwrap();
        }

        assert_eq!(log(dir.path(), 2).len(), 2);
    }

    #[test]
    fn diff_stat_is_zero_before_any_commit_exists() {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path()).unwrap();

        assert_eq!(diff_stat(dir.path()).unwrap(), DiffStat::default());
    }

    #[test]
    fn diff_stat_counts_tracked_and_untracked_changes() {
        let dir = tempfile::tempdir().unwrap();
        init_with_identity(dir.path());
        fs::write(dir.path().join("a.md"), "one\ntwo\n").unwrap();
        commit_all(dir.path(), "first commit").unwrap();
        fs::write(dir.path().join("a.md"), "one\ntwo\nthree\n").unwrap();
        fs::write(dir.path().join("new.md"), "brand\nnew\nfile\n").unwrap();

        let stat = diff_stat(dir.path()).unwrap();

        assert_eq!(stat.files_changed, 2);
        assert_eq!(stat.insertions, 4); // 1 line added to a.md + 3 lines in new.md
        assert_eq!(stat.deletions, 0);
    }

    fn context<'a>(
        diff: Option<&'a DiffStat>,
        files: &'a [ChangedFile],
        root: &'a Path,
    ) -> CommitContext<'a> {
        CommitContext {
            date: "2026-09-30",
            time: "14:05",
            num_files: 2,
            diff,
            files,
            root,
        }
    }

    #[test]
    fn render_commit_message_substitutes_every_placeholder() {
        let diff = DiffStat {
            files_changed: 2,
            insertions: 10,
            deletions: 3,
        };
        let root = Path::new("/project");
        let files = [ChangedFile {
            kind: FileChangeKind::Added,
            path: root.join("new.md"),
        }];
        let message = render_commit_message(
            "{{date}} {{time}}: {{numFiles}} files, +{{linesAdded}} -{{linesDeleted}} \
             ({{linesChanged}} changed)\n\n{{fileList}}",
            &context(Some(&diff), &files, root),
        );

        assert_eq!(
            message,
            "2026-09-30 14:05: 2 files, +10 -3 (13 changed)\n\nA new.md"
        );
    }

    #[test]
    fn render_commit_message_degrades_gracefully_with_no_diff_stat_or_files() {
        let root = Path::new("/project");
        let message = render_commit_message(
            "+{{linesAdded}} -{{linesDeleted}} ({{linesChanged}}) [{{fileList}}]",
            &context(None, &[], root),
        );

        assert_eq!(message, "+0 -0 (0) []");
    }

    #[test]
    fn render_commit_message_leaves_a_template_with_no_placeholders_untouched() {
        let root = Path::new("/project");
        assert_eq!(
            render_commit_message("Smaragd backup", &context(None, &[], root)),
            "Smaragd backup"
        );
    }

    #[test]
    fn format_commit_time_is_hh_mm() {
        let time = format_commit_time();
        assert_eq!(time.len(), 5);
        assert_eq!(time.as_bytes()[2], b':');
        assert!(time[..2].chars().all(|c| c.is_ascii_digit()));
        assert!(time[3..].chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn changed_files_classifies_added_changed_and_deleted() {
        let dir = tempfile::tempdir().unwrap();
        init_with_identity(dir.path());
        fs::write(dir.path().join("keep.md"), "one\n").unwrap();
        fs::write(dir.path().join("remove.md"), "bye\n").unwrap();
        commit_all(dir.path(), "initial commit").unwrap();
        fs::write(dir.path().join("keep.md"), "one\ntwo\n").unwrap();
        fs::remove_file(dir.path().join("remove.md")).unwrap();
        fs::write(dir.path().join("new.md"), "brand new\n").unwrap();

        let files = changed_files(dir.path()).unwrap();

        assert_eq!(
            files
                .iter()
                .map(|f| (f.kind, f.path.clone()))
                .collect::<Vec<_>>(),
            vec![
                (FileChangeKind::Changed, dir.path().join("keep.md")),
                (FileChangeKind::Added, dir.path().join("new.md")),
                (FileChangeKind::Deleted, dir.path().join("remove.md")),
            ]
        );
    }

    #[test]
    fn format_file_list_renders_one_line_per_file_relative_to_root() {
        let root = Path::new("/project");
        let files = vec![
            ChangedFile {
                kind: FileChangeKind::Deleted,
                path: root.join("old.md"),
            },
            ChangedFile {
                kind: FileChangeKind::Added,
                path: root.join("Chapters/new.md"),
            },
        ];

        assert_eq!(
            format_file_list(root, &files),
            "D old.md\nA Chapters/new.md"
        );
    }

    #[test]
    fn format_file_list_is_empty_with_no_files() {
        assert_eq!(format_file_list(Path::new("/project"), &[]), "");
    }
}
