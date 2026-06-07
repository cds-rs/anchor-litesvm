# Migrating away from the `compat/anchor-0.31` branch

This branch is a **frozen compatibility shim**, not a maintained line.
It exists for one reason: [`mpl-core`](https://github.com/metaplex-foundation/mpl-core)'s
`anchor` feature is still pinned to `anchor-lang = "0.31.1"` and there
is no released or main-branch mpl-core that supports `anchor-lang = "1.0"`.
Anyone building an Anchor program that uses mpl-core's `Account<BaseAssetV1>` /
`Account<BaseCollectionV1>` therefore can't use anchor-lang 1.0, and so
can't use the `main` branch of anchor-litesvm either.

This branch closes that gap. It will be deprecated and archived the moment
upstream mpl-core ships an `anchor-lang = "1.0"` release.

## Scope

| | `main` | `compat/anchor-0.31` |
|---|---|---|
| `anchor-lang` | `1.0.x` | `=0.31.1` |
| `solana-*` | `3.x` (split crates) | `=2.2.1` (via `solana-sdk`) |
| `litesvm` | local path, unreleased 0.12 with `cpi_tree` | `=0.6.1` (crates.io) |
| `spl-token` | `9.0` | `=7.0` |
| Bundle / BundledPubkeys / BuildableIx | full support | full support |
| `AnchorContext`, `TestHelpers`, `AssertionHelpers`, `EventHelpers` | full support | full support |
| `print_logs_structured`, `logs_structured_string` | annotated CPI tree | **plain log dump** (API present, output downgraded) |
| Alias-annotated `send_ok` / `send_err_named` failure output | annotated CPI tree | **plain log dump** |
| Forward features (anything landing on `main` after the branch cut) | yes | no |
| Bug fixes | yes | yes |

## What you give up

The branch is bug-fix-only. You don't get:

- New helpers, ergonomics, or APIs added to `main` after the branch cut.
- The structured CPI tree on transaction failure (litesvm 0.6 doesn't
  expose `cpi_tree`; we cut the renderer rather than backport it).
- `TransactionResult::fee()` (litesvm 0.6's `TransactionMetadata`
  has no `fee` field; the method is kept so call sites compile, but
  always returns `0`).

## What you get back when you migrate

When mpl-core ships anchor 1.0 (or you stop using its `anchor` feature):

1. Update `Cargo.toml`:
   ```toml
   # Before
   anchor-litesvm = { version = "0.4.0-anchor-0.31", ... }
   # After
   anchor-litesvm = "0.5"  # or whatever main is at
   ```
2. Move solana imports from `solana_sdk::*` back to the split crates
   (`solana_keypair`, `solana_signer`, `solana_message`, `solana_transaction`).
   Or use anchor-litesvm's re-exports (`anchor_litesvm::Keypair`,
   `anchor_litesvm::Signer`) to insulate yourself from the split.
3. Bump `anchor-lang` and `anchor-spl` to `1.0`.
4. Drop your mpl-core fork if you forked it.
5. The Bundle code (`MarketplaceBundle`, the `BundledPubkeys` derives,
   `ctx.program().build_ix(bundle, ...)` calls) requires **no changes**.
   That's the whole point of having it work on both branches.

## When the deprecation lands

When upstream mpl-core publishes an `anchor-lang = "1.0"` compatible release:

- This branch gets an `@deprecated` marker in `Cargo.toml`.
- The `description` and `README` get a banner pointing at `main`.
- No further releases. Bug fixes go to `main`.
- The branch stays around (no force-delete) so anyone still on it can keep
  consuming the last release.
