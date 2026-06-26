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

### Added

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
