# Common dev tasks. Run `just` (or `just --list`) to see all recipes.

# List available recipes
default:
    @just --list

# Run a debug build
run:
    cargo run

# Build a debug binary
build:
    cargo build

# Build an optimized release binary
build-release:
    cargo build --release

# Install the release binary to ~/.cargo/bin
install:
    cargo install --path . --locked

# Remove build artifacts: cargo's target/, the built manual, and dist/
clean:
    cargo clean
    rm -rf book-src/book dist

# Run the test suite, including workspace members (matches CI: cargo test --workspace --all-targets --all-features)
test:
    cargo test --workspace --all-targets --all-features

# Lint with clippy, warnings as errors (matches CI)
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Format the code in place
fmt:
    cargo fmt --all

# Check formatting without modifying files (matches CI)
fmt-check:
    cargo fmt --all --check

# Run everything CI runs: fmt-check, clippy, test (app + sync server) — use before committing
check: fmt-check clippy test server-check e2e

# --- Sync server (crates/smaragd-sync-server is its own Cargo workspace) ---

# Build the sync server in release mode
server-build:
    cd crates/smaragd-sync-server && cargo build --release

# Run the sync server locally with open registration, data in ./crates/smaragd-sync-server/data
server-run:
    cd crates/smaragd-sync-server && SMARAGD_SYNC_ALLOW_OPEN_REGISTRATION=true cargo run

# Format-check, lint and test the sync server (matches CI's "Sync server" job)
server-check:
    cd crates/smaragd-sync-server && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test

# End-to-end tests: the real sync client/engine against the real server (heavy: links the whole app)
e2e:
    cd crates/smaragd-sync-e2e && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test

# Build the sync server's Docker image (context must be the repo root)
docker-build:
    docker build -f crates/smaragd-sync-server/Dockerfile -t smaragd-sync-server .

# Generate an lcov coverage report (matches CI's Coverage job)
coverage:
    cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info

# Cut a release: bump version, roll RELEASENOTES.md, check, commit, tag, push. Usage: just release 0.6.2 [--dry-run|--yes]
release version *args:
    ./scripts/release.sh {{ version }} {{ args }}

# Regenerate the flatpak build's vendored cargo sources from Cargo.lock (run after any Cargo.lock change)
flatpak-sources:
    ./scripts/update-flatpak-sources.sh

# Build the user manual locally (requires `cargo install mdbook` once)
book:
    mdbook build book-src

# Live-reloading local preview of the user manual
book-serve:
    mdbook serve book-src --open
