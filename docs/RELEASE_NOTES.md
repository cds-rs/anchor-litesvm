# Release Notes

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

## anchor-litesvm dependency bump

This release updates `anchor-litesvm` to the latest compatible Anchor and LiteSVM stack used by this workspace.

### Highlights

- Upgraded `anchor-lang` from `1.0.0-rc.2` to `1.0.0`
- Upgraded `litesvm` from `0.8.2` to `0.11.0`
- Upgraded `litesvm-token` from `0.8.2` to `0.11.0`
- Tightened direct Solana dependency pins to the tested compatibility set
- Removed unused legacy workspace dependencies: `anchor-client` and `solana-client`

### Validation

- `cargo test -p anchor-litesvm --offline`
- `cargo check -p anchor-litesvm --examples --offline`

These checks passed locally, including package doc tests and example compilation for `anchor-litesvm`.
