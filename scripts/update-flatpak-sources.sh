#!/usr/bin/env bash
set -euo pipefail

# Regenerates packaging/flatpak/cargo-sources.json from Cargo.lock, using
# flatpak-builder-tools' cargo generator. The flatpak build vendors crates
# from that file rather than fetching them live, so it drifts out of sync
# with Cargo.lock silently whenever a dependency is bumped (e.g. by
# Dependabot) — nothing else keeps the two in sync. Run this after any
# Cargo.lock change; CI's "Flatpak vendored sources" job fails the build if
# you forget.
cd "$(dirname "$0")/.."

GENERATOR_URL="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py"
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

curl -sSfLo "$TMPDIR/flatpak-cargo-generator.py" "$GENERATOR_URL"

python3 -m venv "$TMPDIR/venv"
"$TMPDIR/venv/bin/pip" install -q "aiohttp<4.0.0,>=3.9.5" "PyYAML<7.0.0,>=6.0.2" "tomlkit>=0.13.3,<1.0"
"$TMPDIR/venv/bin/python3" "$TMPDIR/flatpak-cargo-generator.py" Cargo.lock -o packaging/flatpak/cargo-sources.json

echo "Regenerated packaging/flatpak/cargo-sources.json"
