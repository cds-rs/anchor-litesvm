# Dependencies

One rule above all: test code names `anchor-litesvm` (or `testsvm` /
`testsvm-mollusk`) and nothing else. The framework re-exports everything a
test needs (`Keypair`, `Pubkey`, `Signer`, `Report`, the harness); a direct
`litesvm` or `solana-*` dev-dependency creates the version-alignment problem
the facade exists to prevent. Which litesvm actually runs your tests is the
framework's resolved dependency; see "Which litesvm runs your tests" in
[Installation](https://github.com/cds-rs/anchor-litesvm/blob/turbin3/book/src/intro/installation.md).

## Anchor program

The dependency is host-only (a target cfg keeps it out of the BPF binary),
and there are no `[dev-dependencies]` at all:

```toml
[target.'cfg(not(target_os = "solana"))'.dependencies]
anchor-litesvm = { git = "https://github.com/cds-rs/anchor-litesvm", branch = "turbin3" }
```

This is the vault listing's real manifest. `test_helpers` modules that hold
derive impls live in `src/` behind `#[cfg(not(target_os = "solana"))]`,
satisfying the orphan rule without touching the on-chain build.

## Pinocchio program

Tests must not alter how the program is written or shipped. We use the
standard Serde-style feature-gated derive pattern: an optional dependency,
enabled only by the `testing` feature, with its derive attached via
`cfg_attr`:

```toml
[dependencies]
litesvm-pinocchio = { version = "0.4", optional = true }

[features]
testing = ["dep:litesvm-pinocchio"]
```

```rust
#[cfg_attr(feature = "testing", derive(litesvm_pinocchio::Discriminator))]
#[repr(u8)]
pub enum Instruction { /* ... */ }
```

Run tests with `--features testing`. The acceptance proof, run it after any
manifest change:

```bash
cargo tree -e normal
```

should show `litesvm-pinocchio` (and every other testing-only dependency)
absent from the normal dependency graph, which demonstrates that the
`testing` feature cannot affect the release/SBF artifact.

## Same contract, second engine

Engines never share a dependency graph. A suite that targets a second engine
is a separate crate, excluded from the workspace, with its own lockfile;
"cross-engine" means rebuilding the same test against a different backend,
never linking two engines together. The shipped mollusk adapter is the
canonical example of the shape:

```toml
[package]
name = "testsvm-mollusk"
version = "0.4.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/brimigs/anchor-litesvm"
description = "TestSVM adapter for mollusk-svm: Pinocchio-program suites run the same conformance scenarios on mollusk, in a graph that carries no litesvm."

# Deliberately OUTSIDE the workspace (root Cargo.toml `exclude`): the mollusk
# graph (agave-4.0 tilde pins) and the litesvm graph (solana 3.4 pins) cannot
# share one lockfile. Same test, different backend, rebuild.

[dependencies]
testsvm = { path = "../testsvm" }
mollusk-svm = "0.13.1"
solana-svm-log-collector = "4.0"
solana-account = "3.2"
solana-keypair = "3.1"
solana-message = "3.1"
solana-pubkey = "3.0"
solana-clock = "3.0"
solana-signer = "3.0"
solana-instruction = "3.2"
solana-system-interface = "3.1"
```
