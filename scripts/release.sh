#!/usr/bin/env bash
# Cut a release: bump the version, roll RELEASENOTES.md's Unreleased section
# into a new dated version header, run the same checks CI runs, commit,
# push main, then tag and push — which triggers .github/workflows/release.yml
# (see README.md's "Releases" section).
#
# Usage: scripts/release.sh <version> [--yes] [--dry-run]
#   <version>   New version, e.g. 0.6.2 (no leading "v")
#   --yes, -y   Skip the confirmation prompt before pushing/tagging
#   --dry-run   Do everything except the final push/tag — leaves a
#               described commit and a locally-moved main bookmark you
#               can inspect (or undo) before pushing anything
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/release.sh <version> [--yes] [--dry-run]

  <version>   New version, e.g. 0.6.2 (no leading "v")
  --yes, -y   Skip the confirmation prompt before pushing/tagging
  --dry-run   Do everything except the final push/tag

Must be run with a clean jj working copy (no pending uncommitted changes).
EOF
    exit 1
}

version=""
assume_yes=0
dry_run=0
for arg in "$@"; do
    case "$arg" in
        -y | --yes) assume_yes=1 ;;
        --dry-run) dry_run=1 ;;
        -h | --help) usage ;;
        -*)
            echo "error: unknown option: $arg" >&2
            usage
            ;;
        *)
            if [ -n "$version" ]; then
                echo "error: unexpected argument: $arg" >&2
                usage
            fi
            version="$arg"
            ;;
    esac
done
[ -n "$version" ] || usage

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "error: version must look like X.Y.Z (got: $version)" >&2
    exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [ ! -f Cargo.toml ] || ! grep -q '^name = "smaragd"' Cargo.toml; then
    echo "error: expected to find the smaragd Cargo.toml at $repo_root" >&2
    exit 1
fi

for tool in jj git cargo; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "error: $tool is required" >&2
        exit 1
    }
done

# Refuse to fold unrelated in-progress work into the release commit — jj's
# working copy is always "the current commit," so anything already sitting
# there would otherwise get swept in alongside the version bump. Uses jj's
# own `empty` commit predicate rather than checking whether `jj diff --stat`
# prints anything — that always prints a "0 files changed..." summary line
# even for a genuinely empty commit (confirmed on jj 0.41.0), which made this
# check reject every invocation, clean working copy or not.
if [ "$(jj log -r @ --no-graph -T 'if(empty, "empty", "not-empty")' 2>/dev/null)" != "empty" ]; then
    echo "error: the current commit has changes — describe it, then run 'jj new' for a fresh one, before releasing:" >&2
    jj diff --stat >&2
    exit 1
fi

# Refuse to move `main` onto history that isn't a descendant of main@origin —
# without this, a stray commit checked out by mistake would get silently
# tagged and pushed as the release, and `jj git push` would likely reject it
# anyway, but only after the version bump/checks/commit already happened.
# Skipped (with a warning, not an error) if main@origin can't be resolved at
# all, e.g. no remote tracking data yet — that's a setup issue, not something
# this script should block on.
echo "Checking the current commit is a descendant of main@origin..."
if origin_main_id="$(jj log -r 'main@origin' --no-graph -T 'commit_id' 2>/dev/null)"; then
    if [ -z "$(jj log -r "${origin_main_id}::@" --no-graph -T 'commit_id' -l 1 2>/dev/null)" ]; then
        echo "error: the current commit is not a descendant of main@origin ($origin_main_id) — refusing to move main onto unrelated history. Rebase onto main@origin first." >&2
        exit 1
    fi
else
    echo "warning: couldn't resolve main@origin (no remote tracking data?) — skipping the fast-forward check" >&2
fi

current_version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [ "$current_version" = "$version" ]; then
    echo "error: Cargo.toml is already at version $version" >&2
    exit 1
fi
if [ "$(printf '%s\n%s\n' "$current_version" "$version" | sort -V | tail -n1)" != "$version" ]; then
    echo "error: new version ($version) must be greater than the current version ($current_version)" >&2
    exit 1
fi

tag="v$version"
if git rev-parse "$tag" >/dev/null 2>&1; then
    echo "error: tag $tag already exists locally" >&2
    exit 1
fi
# Also check the remote, not just the local tag namespace — a tag deleted
# locally but still on GitHub (or one someone else pushed) would otherwise
# slip past the check above and only surface once the final `git push origin
# "$tag"` fails, after main has already been pushed. This doubles as an early
# connectivity check for origin, before doing any of the version-bump work.
echo "Checking origin doesn't already have tag $tag..."
if ! remote_tag="$(git ls-remote --tags origin "refs/tags/$tag" 2>&1)"; then
    echo "error: couldn't reach origin to check for an existing tag $tag:" >&2
    echo "$remote_tag" >&2
    exit 1
fi
if [ -n "$remote_tag" ]; then
    echo "error: tag $tag already exists on origin:" >&2
    echo "$remote_tag" >&2
    exit 1
fi

if ! grep -q '^## Unreleased$' RELEASENOTES.md; then
    echo "error: RELEASENOTES.md has no '## Unreleased' header" >&2
    exit 1
fi

unreleased_body="$(awk '/^## Unreleased$/{flag=1;next}/^## /{flag=0}flag' RELEASENOTES.md)"
if [ -z "$(echo "$unreleased_body" | tr -d '[:space:]')" ]; then
    echo "error: RELEASENOTES.md's Unreleased section is empty — nothing to release" >&2
    exit 1
fi

echo "Bumping $current_version -> $version"

# From here on, Cargo.toml/RELEASENOTES.md (and soon Cargo.lock) get edited
# on disk before anything is described — in jj that means they land directly
# in the current (still-anonymous) working-copy commit, not a git-style
# staging area. If `cargo check`/`fmt`/`clippy`/`test` fails partway through,
# make sure that's explained rather than just dying on `set -e`.
files_touched=0
on_error() {
    local exit_code=$?
    if [ "$files_touched" -eq 1 ]; then
        echo >&2
        echo "Release failed with the version bump still on disk, undescribed." >&2
        echo "Run 'jj diff' to see it, 'jj describe -m ...' to commit it as-is once fixed, or 'jj abandon' to discard it and start over." >&2
    fi
    exit "$exit_code"
}
trap on_error ERR

release_date="$(date +%Y-%m-%d)"

# Insert a blank line + the new dated header right after "## Unreleased",
# leaving that section itself empty and everything below it (including the
# blank line that already followed it) untouched.
awk -v ver="$version" -v date="$release_date" '
    { print }
    /^## Unreleased$/ && !done { print ""; print "## v" ver " — " date; done=1 }
' RELEASENOTES.md >RELEASENOTES.md.tmp
mv RELEASENOTES.md.tmp RELEASENOTES.md

sed -i "s/^version = \"$current_version\"/version = \"$version\"/" Cargo.toml
files_touched=1

echo "Refreshing Cargo.lock..."
cargo check --quiet

echo "Running checks (fmt, clippy, test — same as CI)..."
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features

echo
echo "RELEASENOTES.md, Cargo.toml, and Cargo.lock updated:"
jj diff --stat

if [ "$assume_yes" -ne 1 ]; then
    read -r -p "Commit, push main, and push tag $tag? [y/N] " reply
    case "$reply" in
        [yY] | [yY][eE][sS]) ;;
        *)
            echo "Aborted — changes are still on disk, uncommitted." >&2
            exit 1
            ;;
    esac
fi

jj describe -m "Bump version to $version

Rolls RELEASENOTES.md's Unreleased section into a v$version header."
trap - ERR
jj bookmark set main -r @

if [ "$dry_run" -eq 1 ]; then
    echo "Dry run: stopping before push. main bookmark moved locally; nothing pushed or tagged."
    exit 0
fi

jj git push --bookmark main
git tag -a "$tag" -m "$tag" main
git push origin "$tag"

echo
echo "Released $tag."
echo "Watch the build: gh run watch --repo ljantzen/smaragd"
