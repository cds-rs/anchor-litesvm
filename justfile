# Repo chores for the anchor-0.31 compatibility branch of anchor-litesvm.
# `just --list` shows them all.

# Show the recipe list by default.
default:
    @just --list

# Compatibility check: full workspace test suite + the fabrication example.
test-compat:
    # All three crates incl. trybuild fixtures, then the byte-fabrication
    # example that exercises the metaplex + tokens backports end to end.
    cargo test --workspace
    cargo run -p litesvm-utils --example fabricate_nft

# Run the workspace tests.
test:
    cargo test --workspace

# launch cargo doc
doc:
    cargo doc --no-deps -p anchor-litesvm -p litesvm-utils -p anchor-litesvm-derive --open

# Format the whole workspace.
fmt:
    cargo fmt --all

# The CI gate: formatting and clippy, both denying warnings.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
