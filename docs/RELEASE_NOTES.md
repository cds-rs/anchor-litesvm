# Release Notes

## anchor-litesvm 0.4.0

This release updates `anchor-litesvm` to Anchor `1.0.0`, LiteSVM `0.11.0`, and the newly published `litesvm-utils 0.4.0`.

### Highlights

- Upgraded `anchor-lang` from `1.0.0-rc.2` to `1.0.0`
- Upgraded `litesvm` from `0.8.2` to `0.11.0`
- Upgraded `litesvm-token` from `0.8.2` to `0.11.0`
- Updated the direct `litesvm-utils` dependency to `0.4.0`
- Replaced direct `solana-sdk` usage in the crate with split Solana crates
- Bundled crate-local examples so the published package no longer drops example entries

### Validation

- `cargo package -p anchor-litesvm --allow-dirty`
- `cargo test -p anchor-litesvm --offline`

## litesvm-utils 0.4.0

This release updates `litesvm-utils` to the latest compatible LiteSVM stack and removes its direct dependency on the legacy `solana-sdk` umbrella crate.

### Highlights

- Upgraded `litesvm` from `0.8.2` to `0.11.0`
- Upgraded `litesvm-token` from `0.8.2` to `0.11.0`
- Replaced direct `solana-sdk` usage with split Solana crates:
  - `solana-keypair`
  - `solana-signer`
  - `solana-transaction`
- Kept the remaining direct Solana dependencies on the current compatible Solana 3-line

### Validation

- `cargo package -p litesvm-utils --allow-dirty --offline`
- `cargo test -p litesvm-utils --offline`
