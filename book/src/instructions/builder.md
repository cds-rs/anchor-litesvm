# The Instruction Builder

The previous chapter showed *what* you fill in (a named bundle); this one is the call that turns it into an `Instruction`.

## The one-liner

```rust
let ix = ctx.program().build_ix(accs, vix::Transfer { amount: 500_000 });
```

- **`ctx.program()`** hands you a `Program` bound to your deployed program's id. (Standalone, `Program::new(program_id)` does the same.)
- **`build_ix(bundle, args)`** takes your bundle of pubkeys and the generated args struct, projects the bundle into the full account list (auto-injecting the canonical program IDs), serializes the args, and returns a ready `Instruction`.

Note it returns an `Instruction`, not a `Result<Instruction, _>`. Building can't fail here: an `InstructionData` value always serializes, and the account projection is total, so there's no error case to unwrap. (`vix` is the conventional short alias for your program's `instruction` module: `use my_program::instruction as vix`.)

> **Pinocchio:** `build_ix` projects an Anchor bundle into Anchor's generated account and arg types. A Pinocchio program has neither, so you build the `Instruction` directly (program id, account metas, data) and send that. See [Testing Pinocchio Programs](../appendix/pinocchio.md).

## Where the bundle comes from

`accs` is a bundle struct you define once in a host-only `test_helpers` module: a flat `#[derive(anchor_litesvm::Bundle)]` of pubkeys, bound to the instruction's accounts by a `cfg_attr` carrying `bundled_with(...)`. That `cfg_attr` generates the projection from the bundle into the account list and the type-level link to the args, which is what `build_ix` runs. The [next chapter](bundled-pubkeys.md) is the full tour of both pieces; here, all `build_ix` needs is the finished bundle.

## Why `build_ix` and not a send

`build_ix` stops at an `Instruction`; it doesn't send. That's deliberate: testing doesn't need the RPC abstractions a real client wraps around instruction-building, and keeping build and send separate means you can assemble several instructions and decide how to send them.

When you *do* just want to send one and assert it worked, the [`Tx` builder](../running/executing.md) folds build and send together:

```rust
ctx.tx(&[&alice])
    .build(accs, vix::Transfer { amount: 500_000 })
    .send_ok();
```

`.build(bundle, args)` here is `build_ix` under the hood; `.send_ok()` sends the transaction with the listed signers and asserts success. Most tests use this form and never name the intermediate `Instruction` at all.

## Several instructions, one transaction

When you need atomicity, build each instruction and send them together:

```rust
let ix1 = ctx.program().build_ix(accs1, vix::Deposit { amount: 1_000 });
let ix2 = ctx.program().build_ix(accs2, vix::Withdraw { amount: 400 });

ctx.execute_instructions(vec![ix1, ix2], &[&alice]).assert_success();
```

(Atomic meaning the usual Solana guarantee: either the whole transaction lands or none of it does.)

## The escape hatch, and what's underneath

`build_ix` is a convenience over a lower-level builder that takes the account metas and args directly:

```rust
let ix = ctx.program()
    .accounts(some_to_account_metas)   // anything implementing ToAccountMetas
    .args(some_instruction_data)       // anything implementing InstructionData
    .instruction()?;                   // Result<Instruction, _>
```

This is a provided escape hatch, not the common path: with a bundle, the `ToAccountMetas` value comes from the projection, so you reach for the hand-written builder only when your account list comes from some other source. `build_ix` composes this builder once the bundle has been projected.

To adjust what the projection produced before the instruction is assembled, `build_ix_with(bundle, args, |accs| { ... })` runs your closure over the projected accounts struct, then assembles the instruction from the result. The framework is still developing and we don't yet know how this UX will land, so rather than claim you'll rarely need it, treat it as an escape hatch for more sophisticated usage: if you reach for it, please open an issue so we can track the case and see whether the projection should grow to cover it.
