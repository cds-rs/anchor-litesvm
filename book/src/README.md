# Testing Anchor Programs with LiteSVM

This is the guide for `anchor-litesvm`: a thin layer over [LiteSVM](https://github.com/LiteSVM/litesvm) that lets you test Anchor programs without the ceremony. No mock RPC, no validator, no manually ordered `Vec<AccountMeta>`. You write named structs, you send instructions, and you assert on the result.

## Testing Premise

Everything persistent in Solana is an account, and a transaction is a collaboration among them: signers authorize, programs execute, PDAs hold state, token accounts move value. So describe a test by its cast (maker, taker, vault, market), not by the pubkeys the runtime happens to assign. The framework carries those names through its diagnostics: inspect a CPI tree, an authority graph, or a trace and you see the actors you introduced, not a wall of base58. That idea runs the whole book, and Part V puts it to work on real programs.

We model before we test. First draw the accounts, the authorities they hold, and the interactions between them as [PlantUML diagrams](modeling.md); those become the blueprint for both the program and its tests. This mirrors the runtime: a Solana transaction must declare every account it touches before it runs, so naming the cast up front is the discipline the validator already enforces. Name the participants, then exercise them, and the behavior you test is the behavior you reasoned about.

## How this book is organized

It reads front to back as a tutorial, building on one idea: model the accounts before you test them. The [Modeling with Diagrams](modeling.md) primer comes first; then:

- **Part I** gets you from an empty `Cargo.toml` to a passing test in five minutes, then maps how the pieces fit together.
- **Part II** covers building instructions: named accounts, the builder, PDAs and token helpers, and the `BundledPubkeys` derive that smooths the repetitive cases.
- **Part III** covers running and asserting, and introduces the actors model that the rest of the book leans on.
- **Part IV** covers the four ways a `TransactionResult` can render what happened: the CPI tree, a Mermaid sequence diagram, and the authority and ownership graphs.
- **Part V** opens with the scandals (the failures worth testing), then works three examples (Vault, Escrow, CPAMM), each built around its cast.
- **Appendix** holds the migration guide, the test-output conventions, and pointers to the architecture notes.

## A note on the other docs

The API reference (every type, trait, and method) is *generated* from the source, not written here. Run `cargo doc --no-deps --open` against your checkout.
