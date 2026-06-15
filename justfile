# Repo chores for anchor-litesvm. `just --list` shows them all; `just book ...`
# runs the book's own tasks (see book/justfile).

mod book

# Show the recipe list by default.
default:
    @just --list

# Show the md book
mdbook:
    mdbook serve book --open

# launch cargo doc
doc:
    cargo doc --no-deps -p anchor-litesvm -p litesvm-utils -p anchor-litesvm-derive --open

# Run the workspace tests.
test:
    cargo test --workspace

# Format the whole workspace.
fmt:
    cargo fmt --all

# The CI gate: formatting and clippy, both denying warnings.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings

# Build and test every book listing the way book-listings.yml does.
# Needs the Solana + Anchor toolchains on PATH.
listings:
    #!/usr/bin/env bash
    set -euo pipefail
    for d in book/listings/*/; do
      echo "== ${d} =="
      ( cd "${d}" && anchor build --no-idl --ignore-keys && cargo test --tests )
    done
