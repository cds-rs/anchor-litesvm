# Rebuild vendored program fixtures (.so + IDL) from examples/, then re-capture
# the book snapshots. Requires anchor + cargo-build-sbf.
.PHONY: fixtures book test drift

# vault/escrow pin anchor 1.0.2, which refuses to build on a keypair/declare_id
# mismatch (the vendored keypairs are throwaway, so --ignore-keys sidesteps it).
# staking pins 0.31.1 via its own Anchor.toml [toolchain] section; anchor's CLI
# auto-dispatches to that version with no avm state change, and 0.31.1 predates
# the mismatch check entirely (no flag needed, and --ignore-keys doesn't exist
# on it: passing it would error out).
fixtures:
	cd examples/vault   && anchor build --ignore-keys
	cd examples/escrow  && anchor build --ignore-keys
	cd examples/staking && anchor build
	cp examples/vault/target/deploy/vault.so     crates/anchor-litesvm/tests/fixtures/vault.so
	cp examples/escrow/target/deploy/escrow.so   crates/anchor-litesvm/tests/fixtures/escrow.so
	cp examples/staking/target/deploy/staking.so crates/anchor-litesvm/tests/fixtures/staking.so
	cp examples/vault/target/idl/vault.json      crates/anchor-litesvm/idls/vault.json
	cp examples/escrow/target/idl/escrow.json    crates/anchor-litesvm/idls/escrow.json
	BLESS=1 cargo test -p anchor-litesvm --tests

# Golden guard: fails if any committed snapshot drifts from current output.
drift test:
	cargo test -p anchor-litesvm --tests

book:
	mdbook build book
