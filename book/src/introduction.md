# Introduction

`anchor-litesvm` drives Anchor programs through [LiteSVM](https://github.com/LiteSVM/litesvm),
a fast in-process Solana VM, with an ergonomic testing surface: a typed
instruction builder generated from your IDL, a named cast of actors, and
structured transaction logs rendered as a CPI tree.

This book is a tutorial and reference. Every block of program output you see is
**captured from a real test** in this repository (a deployed `.so`, a real
transaction), not hand-typed, so it cannot drift from what the code emits.

## The two crates

- **`anchor-litesvm`**: Anchor-specific testing. Generated instruction bundles,
  account/event decoding, the `AnchorContext` world.
- **`litesvm-utils`**: framework-agnostic helpers underneath it (actors, aliases,
  token setup, clock warping, the log renderer). `anchor-litesvm` re-exports it,
  so Anchor users get everything.

## What you will build

Three worked examples, each a real deployed program:

- **Vault** (`initialize`/`deposit`/`withdraw`/`close`, emits a `Deposited` event):
  the simplest happy path, event decoding, and the negative-account escape hatch.
- **Escrow** (`make`/`take`/`refund`, SPL token CPIs, a 90-day time-lock): token
  setup, the CPI tree, and time-travel via the clock helpers.
- **Stake** (mpl-core NFT staking, a freeze-period day lock): the deepest CPI tree
  and driving a program with raw hand-built instructions.
