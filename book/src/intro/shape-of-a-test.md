# The Shape of a Test

Every test in this book has the same three movements, the ones the [worked examples](../examples/vault.md) all show: **arrange** a scenario, **act** on it, **assert** the result. The framework fuses the busywork inside each movement, so what you actually write is short.

## Arrange: set up the scenario

A `setup()` builds the world once. It deploys the program with `AnchorLiteSVM::build_with_program(program_id, "name", bytes)`, mints the cast as deterministic actors with their token accounts and PDAs (the `ctx.svm.create_*` helpers), names them in the alias table, and hands back a scenario: the context, the bundle of pubkeys, and the signer keypairs. [Vault](../examples/vault.md#setup) and [Escrow](../examples/escrow.md#the-cast) show this end to end.

Deploying and creating accounts are one movement, not two: `setup()` returns something you can act on immediately.

## Act: build and send

```rust
ctx.tx(&[&user])
    .build(accs, vix::Initialize { amount: 1_000_000 })
    .send_ok();
```

One chain. `ctx.tx(signers)` names who signs, `.build(bundle, args)` turns your bundle and the generated args into an instruction, and `.send_ok()` sends it and asserts it succeeded. Building and executing aren't separate steps you thread together; they're one statement. (`accs` is your bundle; `vix` is your program's `instruction` module, `use my_program::instruction as vix`. To send several instructions atomically, or to inspect one before sending, `ctx.program().build_ix(accs, args)` hands back the raw `Instruction`.) See [Executing Transactions](../running/executing.md).

## Assert: check the state

```rust
ctx.svm.assert_token_balance(&token_account, 1_000_000);
```

`send_ok()` already asserted the transaction *ran*; the `ctx.svm.assert_*` helpers (`assert_account_exists`, `assert_token_balance`, `assert_sol_balance`, `assert_account_owner`, and friends), plus `ctx.try_load` for an account's fields, assert the *state* ended up where you expected. A good test pairs the two: send, then check what the send left behind. See [Assertion Helpers](../running/assertions.md).

That's the whole shape. The worked examples thread a [`Report` recorder](../examples/escrow.md) through it to narrate each movement into committable Markdown, but the bones are always arrange, act, assert. That split, a plain mechanics test versus a narrated scenario, is drawn in full in [The Mechanism, End to End](../instructions/end-to-end.md).

## A few things to get right

**Finish the chain with a terminator.** `ctx.tx(&[&user]).build(accs, vix::Initialize { amount })` builds a description of a transaction; the terminator (`.send_ok()`, `.send_err()`, `.send_err_named(...)`) is what sends it. The send is the verb.

**Match the args type to the bundle.** Each bundle is tied at the type level to one instruction's accounts, so `.build(deposit_accs, vix::Withdraw { .. })` is a compile error at the call site rather than a malformed instruction at runtime. Reach for the args type that goes with your bundle.

**Decorate the accounts struct with the `cfg_attr`.** The bundle machinery comes from `#[cfg_attr(not(target_os = "solana"), derive(BundledPubkeys), bundled_with(...))]` on your `#[derive(Accounts)]` struct; that's what gives `build` the `From<Bundle>` projection it needs. See [Bundled Pubkeys](../instructions/bundled-pubkeys.md).

**Let the bundle handle account order.** The bundle's fields are named, and Anchor's own `ToAccountMetas` produces the list in the program-defined order, so ordering is a non-issue here; the [Named Accounts](../instructions/named-accounts.md) chapter explains why.
