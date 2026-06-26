# Why anchor-litesvm

Testing a Solana program the raw way means standing up a lot of scaffolding before you can assert anything: a mock RPC or a local validator, hand-built `Vec<AccountMeta>` in exactly the order your program expects, manual signer wrangling, and log parsing by hand when something goes wrong. The friction is real enough that a lot of programs simply go under-tested.

> **Pinocchio:** this book teaches Anchor, but the engine under it isn't Anchor-specific. A raw Pinocchio program tests through the same harness, observability, and assertions; only the bundle sugar is Anchor-only. See [Testing Pinocchio Programs](../appendix/pinocchio.md).

`anchor-litesvm` removes most of that. It sits on top of [LiteSVM](https://github.com/LiteSVM/litesvm) (an in-process Solana VM: no network, no validator) and adds the Anchor-shaped ergonomics on top:

- **Named accounts.** You fill in a struct with named fields; Anchor's `ToAccountMetas` handles the ordering. Swapping two fields can't break your test, because the compiler doesn't care what order you write them in. (Part II's [Named Accounts](../instructions/named-accounts.md) chapter is the full treatment.)
- **One-line setup.** `AnchorLiteSVM::build_with_program(id, name, bytes)` replaces twenty-odd lines of raw setup. No mock RPC, no network dependencies.
- **Helpers for the boring parts.** Funded accounts, mints, associated token accounts, PDAs: one call each.
- **Rich debugging output built in.** When a transaction fails (or you just want to see what it did), the result renders four ways: a CPI tree, a Mermaid sequence diagram, and authority and ownership graphs. Part IV covers these.

The real win isn't fewer lines, it's a vocabulary: you stop encoding transactions (ordered metas, discriminator bytes, hand-unpacked data) and start describing a cast of actors and what they do to each other. [Accounts as Actors](../running/accounts-as-actors.md) is the home of that idea.

## What this is not

It is not a replacement for on-chain integration testing against a real cluster. LiteSVM is an in-process VM; it's fast and deterministic and perfect for unit and scenario testing, but it is not a validator. When you need to test against real network conditions, you still reach for a test validator or devnet. We recommend [Surfpool](https://www.surfpool.run), best in class in that space. This book is about the layer below that, where most of your test-writing time actually goes.

## Where to go next

If you just want a passing test, keep reading: [Installation & Setup](installation.md) then [Your First Test](first-test.md). If you're migrating an existing raw-LiteSVM suite, the [Migration appendix](../appendix/migration.md) maps the old calls to the new ones.
