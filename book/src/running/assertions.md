# Assertion Helpers

A passing transaction tells you the program *ran*. It doesn't tell you the program was *correct*: a buggy handler can return success while leaving the wrong balance behind. So the other half of every test is checking that the world ended up in the state you expected. That's what the `ctx.svm.assert_*` family is for.

Keep the two halves distinct in your head:

- The [transaction-level assertions](executing.md) (`assert_success`, `assert_error`, ...) live on the **`TransactionResult`** and ask "did the send do what I expected?"
- The **state assertions** in this chapter live on **`ctx.svm`** and ask "is the on-chain state what I expected?"

A good test pairs them: send, assert the outcome, then assert the resulting state.

## The helpers

Six cover almost everything, all on `ctx.svm` (via the `AssertionHelpers` trait), all panicking with a useful message when they don't hold:

```rust
// Existence and lifecycle
ctx.svm.assert_account_exists(&pubkey);
ctx.svm.assert_account_closed(&pubkey);

// Balances
ctx.svm.assert_sol_balance(&account, 10_000_000_000);   // lamports
ctx.svm.assert_token_balance(&token_account, 1_000_000); // SPL token amount

// Mint and ownership
ctx.svm.assert_mint_supply(&mint, 1_000_000);
ctx.svm.assert_account_owner(&account, &program_id);
```

A few notes on the ones with sharp edges:

- **`assert_account_closed`** checks the account is *gone* (zero lamports / no data), which is what a well-behaved Anchor `close` leaves behind. It's the natural partner to `assert_account_exists` and the thing you assert after a `Refund` or `Take` that closes a vault.
- **`assert_token_balance`** takes the **token account** (the ATA or token account address), not the wallet and not the mint. Passing the wallet pubkey is the common mistake; you want the address that actually holds the balance.
- **`assert_account_owner`** is the assertion form of the [ownership graph](../inspect/graphs.md): it checks which program owns the account. This is how you'd assert in a test what that graph shows you visually, that a freshly created token account is owned by the Token program, for instance.

## Reading account data when a helper isn't enough

The six helpers cover the common state checks, but sometimes you need to assert on a *field* of an Anchor account (an escrow's `receive` amount, a vault's `bump`). For that, deserialize the account and assert on it directly:

```rust
let state: my_program::accounts::Escrow = ctx.try_load(&escrow).unwrap();
assert_eq!(state.receive, 500_000);
assert_eq!(state.maker, maker.pubkey());
```

`ctx.try_load::<T>(&pda)` fetches and deserializes (checking the Anchor discriminator); `try_load_unchecked` skips the discriminator check for the rare cases that need it. There's also `ctx.load::<T>(&pda)` / `load_unchecked` if you'd rather have it panic on a missing account than return a `Result`.

## Where to go from here

That's the core testing loop complete: build ([Part II](../instructions/named-accounts.md)), send and assert the outcome ([Executing](executing.md)), assert the state (this chapter), and when something's off, render what happened ([Part IV](../inspect/cpi-tree.md)). The [worked examples](../examples/escrow.md) put all of it together on real programs.

The exhaustive list of every helper, with exact signatures, is in the generated rustdoc (`cargo doc --no-deps --open`, the `AssertionHelpers` trait); this chapter covers the ones you'll actually reach for.
