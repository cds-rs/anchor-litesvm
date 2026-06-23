# Testing Pinocchio Programs

This book teaches Anchor, but the engine under it is not Anchor-specific. The execution layer (`litesvm-utils`, the `TestSVM` trait) drives any Solana program; a raw [Pinocchio](https://github.com/anza-xyz/pinocchio) program tests through the same engine, the same observability, the same assertions. What changes is the sugar: Anchor generates types the bundle derive bridges to, Pinocchio generates none, so a handful of conveniences are Anchor-only and you build instructions more directly.

## What's shared

The whole execution and observability surface:

- `LiteSvmBackend` (and the other `TestSVM` engines) deploy, fund, and send.
- `svm.actor("name", lamports)` casts a funded, deterministic, aliased signer; `prop_mint` / `prop_token_account` fabricate token state. The cast vocabulary, at the trait level.
- The [CPI tree](../inspect/cpi-tree.md), [mermaid diagrams](../inspect/mermaid.md), [authority and ownership graphs](../inspect/graphs.md), compute and fees.
- `send_ok` / `send_err_named` and the assertion helpers.

A Pinocchio test reads like an Anchor one until it builds an instruction.

## What's Anchor-only

The bundle machinery of Part II leans, all of it, on types Anchor generates:

- **[Bundled Pubkeys](../instructions/bundled-pubkeys.md)**: the derive bridges your bundle to Anchor's `accounts::X` / `instruction::X`. Pinocchio generates neither, so there is no projection to derive.
- **[The builder](../instructions/builder.md)**: `build_ix(bundle, args)`, and the program-defined ordering it relies on.
- The Anchor account loaders (`ctx.load` reads by Anchor's discriminator).

## Building an instruction

With no generated types, you construct the `Instruction` yourself: the program id, the account metas in the order the program reads them, and the data (the discriminator byte, then the args). Then send it through the engine:

```rust
let ix = Instruction {
    program_id: MY_PROGRAM_ID,
    accounts: vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new(state, false),
        AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
    ],
    data: vec![/* discriminator byte, then args */],
};
svm.send(&[ix], &[&payer]);
```

That is the raw form an Anchor bundle hides; on Pinocchio you write it.

## Names from one declaration

A Pinocchio program has no IDL. The runtime logs an instruction by its discriminator byte and a failure by `custom program error: 0x<code>`, never `Make` or `InvalidAmount`, so the renderers and `send_err_named` need a `code -> name` table. `#[derive(Discriminator)]` on the instruction enum is the source: one declaration generates the on-chain discriminators *and* the host-side name table, keyed to declaration order, so they can't drift (`litesvm-pinocchio-idl` reads the same enum to emit a solita IDL). Register the table, and the tree shows `Make`, not `[0]`, and `send_err_named("InvalidAmount")` works.

## The dependency shape

Testing must not change how the program ships. Pinocchio testing is an optional dependency behind a `testing` feature, the same feature-gated derive pattern [Dependencies](../agents/dependencies.md) covers, so `cargo tree` shows the testing crates absent from the release and SBF graph.
