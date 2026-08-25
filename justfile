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

# Run the test suite (matches CI: cargo test --all-targets --all-features)
test:
    cargo test --all-targets --all-features

# Lint with clippy, warnings as errors (matches CI)
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Format the code in place
fmt:
    cargo fmt

# Check formatting without modifying files (matches CI)
fmt-check:
    cargo fmt --check

# Run everything CI runs: fmt-check, clippy, test — use before committing
check: fmt-check clippy test

# Generate an lcov coverage report (matches CI's Coverage job)
coverage:
    cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info

# Cut a release: bump version, roll RELEASENOTES.md, check, commit, tag, push. Usage: just release 0.6.2 [--dry-run|--yes]
release version *args:
    ./scripts/release.sh {{ version }} {{ args }}

# Build the user manual locally (requires `cargo install mdbook` once)
book:
    mdbook build book-src

# Live-reloading local preview of the user manual
book-serve:
    mdbook serve book-src --open
