# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **BREAKING** (`anchor-litesvm`): `AnchorContext` now implements the `TestSVM`
  trait, whose `get_account(&self, &Pubkey) -> Option<Account>` returns the raw
  account. To free the name, the deserializing inherent methods were renamed:
  - `ctx.get_account::<T>(addr)` (returns `Result<T, AccountError>`) is now
    `ctx.try_load::<T>(addr)`.
  - `ctx.get_account_unchecked::<T>(addr)` is now `ctx.try_load_unchecked::<T>(addr)`.

  Migration, mechanically:
  - `ctx.get_account::<T>(addr)?` becomes `ctx.try_load::<T>(addr)?`
  - `ctx.get_account::<T>(addr).unwrap()` / `.expect(..)` becomes
    `ctx.load::<T>(addr)` (the existing panicking sibling; drop the
    `unwrap`/`expect`)
  - `ctx.get_account_unchecked::<T>(addr)` becomes `ctx.try_load_unchecked`
    (Result) or `ctx.load_unchecked` (panicking)

  After the rename, `ctx.get_account(addr)` means the trait's raw
  `Option<Account>` accessor; reach for `try_load` / `load` when you want a
  deserialized Anchor account. (The panicking `load` / `load_unchecked` are
  unchanged.)

- (`testsvm-idl`): the generated client is now byte-stable to commit and
  `include!`-able. Instructions are sorted by discriminator on ingest (a Quasar
  IDL lists them in hash-map order, so the same program produced a different
  client run to run), and the generated file's header is a plain `//` banner with
  no inner `#![allow(..)]` (an inner attribute cannot live in an `include!`-d file
  outside the crate root, so the old header blocked module inclusion).

  Migration (only if you generate a client through `testsvm-idl`):
  - if you `include!` the client into a module, apply the allow at the include
    site: `#[allow(dead_code, unused_imports)] pub mod client { include!(..); }`
    (the file no longer carries its own inner attribute).
  - regenerate and commit the client once; the struct order may change
    (per-instruction account order is unchanged, so it is a cosmetic, one-time
    diff). A committed client you do not regenerate is unaffected.

### Added

- `testsvm-quasar-idl` (new crate): a source-extractor that generates the
  construction client straight from a quasar-lang program's source, replacing the
  `quasar idl-build` scrape. `QuasarSource::from_crate(src_dir)` parses the crate
  with `syn` (the `#[program]` instructions and the `#[derive(Accounts)]` structs
  their `Ctx<T>` parameters name), implements `testsvm-idl`'s `IdlSource`, and
  re-exports `emit_client`. The shape comes from the declaration site, so it is
  deterministic (no hash-map ordering) and needs no IDL JSON.

  Adopt it in a consumer's `build.rs` (no migration; it is opt-in):

  ```toml
  [build-dependencies]
  testsvm-quasar-idl = { git = "https://github.com/cds-rs/anchor-litesvm", branch = "turbin3" }
  ```

  ```rust
  // build.rs
  use {std::{env, fs, path::Path}, testsvm_quasar_idl::{emit_client, QuasarSource}};
  let src = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../program/src");
  // emit `cargo:rerun-if-changed=` for each .rs under `src` so it regenerates
  // when the program changes, then:
  let idl = QuasarSource::from_crate(&src).unwrap();
  let out = Path::new(&env::var("OUT_DIR").unwrap()).join("client.rs");
  fs::write(out, emit_client(&idl)).unwrap();
  ```

  Then include it behind the allow (see the Changed note above):
  `#[allow(dead_code, unused_imports)] pub mod client { include!(concat!(env!("OUT_DIR"), "/client.rs")); }`.

  The extractor maps quasar-lang's `Address` arg type (its pubkey type, e.g. an
  optional pool authority `Option<Address>`) to `Pubkey`, so pubkey-typed args
  generate rather than being skipped at the flat-args boundary.

- `testsvm-idl`: `ArgType::Array`, so fixed-size array args (`[u8; N]`, e.g. a
  32-byte commitment or entropy) encode instead of being skipped at the flat-args
  boundary. Purely additive.

- `anchor-litesvm`: `AnchorContext` implements `TestSVM`, so it is usable
  anywhere a `&mut impl TestSVM` is expected and inherits the trait vocabulary
  (`actor`, `prop`, `prop_mint`, `prop_token_account`, `deploy_from_file`,
  `label`, `alias_ata`) as default methods, alongside its Anchor-specific sugar
  (`cast_actor`, `cast_mint`, `fund_ata`, `try_load` / `load`).
- `testsvm`: cast-name uniqueness on `actor` / `prop` (a duplicate cast panics on
  every engine); a `TokenTestSVM` extension trait (`prop_mint`,
  `prop_token_account`, `alias_ata`) that hand-packs the stable SPL mint (82-byte)
  and token-account (165-byte) layouts with no token-crate dependency; `label()`
  and an `aliases()` accessor on the trait; and `model::Transaction::assemble`,
  the single record builder every send adapter shares.

- `litesvm-utils`: four timestamp-based helpers on the `TestHelpers` trait for
  testing time-locked logic (escrow expiries, vesting cliffs, etc.) without
  manually round-tripping the `Clock` sysvar:
  - `get_unix_timestamp()` reads the Clock sysvar's `unix_timestamp`
  - `warp_to_timestamp(unix_ts)` sets `unix_timestamp` to an absolute value
  - `advance_seconds(seconds)` advances `unix_timestamp` by the given seconds
  - `advance_days(days)` convenience wrapper over `advance_seconds`

  Other Clock fields (slot, epoch, etc.) are left unchanged; for slot-based
  warping, continue using `advance_slot` / `warp_to_slot`.

## [anchor-litesvm 0.4.0] - 2026-04-09

### Changed

- Upgraded `anchor-lang` from `1.0.0-rc.2` to `1.0.0`
- Upgraded `litesvm` from `0.8.2` to `0.11.0`
- Upgraded `litesvm-token` from `0.8.2` to `0.11.0`
- Updated the direct `litesvm-utils` dependency to `0.4.0`
- Migrated direct signer, hash, signature, transaction, and account usage from `solana-sdk` to split Solana crates:
  - `solana-account = 3.4.0`
  - `solana-hash = 3.1.0`
  - `solana-keypair = 3.1.2`
  - `solana-signature = 3.4.0`
  - `solana-signer = 3.0.0`
  - `solana-transaction = 3.0.1`
- Kept the remaining direct Solana dependency on the compatible Solana 3 line:
  - `solana-program = 3.0.0`

### Fixed

- Updated version-specific comments from Anchor `1.0.0-rc.2` to `1.0.0`
- Updated crate docs and examples to use split Solana imports and current version snippets
- Bundled crate-local examples so the published package no longer drops example entries during packaging
- Verified `cargo package -p anchor-litesvm --allow-dirty`
- Verified `cargo test -p anchor-litesvm --offline`

## [litesvm-utils 0.4.0] - 2026-04-09

### Changed

- Upgraded `litesvm` from `0.8.2` to `0.11.0`
- Upgraded `litesvm-token` from `0.8.2` to `0.11.0`
- Migrated direct signer and transaction usage from `solana-sdk` to split Solana crates:
  - `solana-keypair = 3.1.2`
  - `solana-signer = 3.0.0`
  - `solana-transaction = 3.0.1`
- Kept the remaining direct Solana dependencies on the current compatible Solana 3-line:
  - `solana-program = 3.0.0`
  - `solana-program-pack = 3.1.0`
  - `solana-system-interface = 2.0.0`

### Fixed

- Updated crate docs and README examples to use split Solana imports
- Verified `cargo package -p litesvm-utils --allow-dirty --offline`
- Verified `cargo test -p litesvm-utils --offline`

## [0.3.0] - 2025-01-12

### Breaking Changes

- **Rust 1.86+ required** - Updated dependencies require newer Rust version
- **anchor-lang** upgraded from 0.31.1 to 1.0.0-rc.2
- **litesvm** upgraded from 0.6.1 to 0.8.2
- **Solana SDK** upgraded from 2.2 to ~3.0
- **spl-token** upgraded from 7.0.0 to 9.0.0
- **spl-associated-token-account** upgraded from 6.0.0 to 8.0.0
- **thiserror** upgraded from 1.0 to 2.0

### Added

- Dedicated README.md for each crate (now displays correctly on crates.io)
- `solana-system-interface` dependency for system program instructions

### Changed

- `system_instruction` now imported from `solana_system_interface` instead of `solana_program`
- `add_program()` now returns `Result` and is handled with `.expect()`
- Simplified type conversions - anchor and litesvm now use same Solana SDK version
- Root README updated to be a workspace overview with links to individual crates

### Fixed

- Documentation now displays properly on crates.io for both crates

## [0.2.0] - Previous Release

Initial public release with:
- `anchor-litesvm`: Simplified Anchor testing with LiteSVM
- `litesvm-utils`: Framework-agnostic testing utilities
