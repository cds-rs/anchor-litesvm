# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **anchor-lang** upgraded from `1.0.0-rc.2` to `1.0.0`
- **litesvm** upgraded from `0.8.2` to `0.11.0`
- **litesvm-token** upgraded from `0.8.2` to `0.11.0`
- Tightened direct workspace dependency pins for the tested Solana compatibility set:
  - `solana-program = 3.0.0`
  - `solana-program-pack = 3.1.0`
  - `solana-keypair = 3.1.2`
  - `solana-signer = 3.0.0`
  - `solana-system-interface = 2.0.0`
  - `solana-transaction = 3.0.1`
  - `thiserror = 2.0.18`
- Removed unused legacy workspace dependencies:
  - `anchor-client`
  - `solana-client`
- `litesvm-utils` no longer depends directly on `solana-sdk`; it now uses split Solana crates for keypairs, signers, and transactions

### Fixed

- Updated version-specific test comments from Anchor `1.0.0-rc.2` to `1.0.0`
- Updated `litesvm-utils` docs and examples to use split Solana imports instead of `solana_sdk`
- Verified `anchor-litesvm` builds and passes its package tests, doc tests, and example compilation against `anchor-lang 1.0.0` and `litesvm 0.11.0`
- Verified `litesvm-utils` packages successfully and passes its package tests and doc tests against `litesvm 0.11.0`

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
