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
# there would otherwise get swept in alongside the version bump.
if [ -n "$(jj diff --stat 2>/dev/null)" ]; then
    echo "error: working copy has uncommitted changes — describe or stash them first:" >&2
    jj diff --stat >&2
    exit 1
fi

current_version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [ "$current_version" = "$version" ]; then
    echo "error: Cargo.toml is already at version $version" >&2
    exit 1
fi

tag="v$version"
if git rev-parse "$tag" >/dev/null 2>&1; then
    echo "error: tag $tag already exists" >&2
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
