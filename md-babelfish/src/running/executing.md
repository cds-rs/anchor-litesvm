# Executing Transactions

You've built an instruction ([Part II](../instructions/named-accounts.md)). Now you send it, and you get back a `TransactionResult`: the logs, the compute, the success-or-failure, and the handle every [rendering view](../inspect/cpi-tree.md) hangs off. This chapter is about that send, the result it returns, and the transaction-level assertions you make on it. (The *world-state* assertions, "is the account balance what I expect", are the [next chapter](assertions.md); the line between them matters and we'll draw it explicitly.)

## The `Tx` builder: build and send in one chain

The send you'll reach for almost every time is the `Tx` builder. It fuses build, send, and expect into a single statement:

```rust
ctx.tx(&[&signer])
    .build(accs, vix::Transfer { amount: 500_000 })
    .send_ok();
```

`ctx.tx(signers)` names the transaction's signers and hands you a `Tx`; `.build(bundle, args)` is `build_ix` under the hood (it constructs the instruction and holds it); and `.send_ok()` is the *terminator*: it sends the transaction and asserts it succeeded. There are three terminators:

```rust
ctx.tx(&[&signer]).build(accs, args).send_ok();                    // expects success
ctx.tx(&[&signer]).build(accs, args).send_err();                   // expects any failure
ctx.tx(&[&signer]).build(accs, args).send_err_named("EscrowExpired"); // expects that named error
```

Each returns the `TransactionResult` directly (no `Result` to unwrap), and each stashes your context's alias table on it, so a chained render reads in your [actor names](accounts-as-actors.md) with no extra threading:

```rust
ctx.tx(&[&signer])
    .build(accs, args)
    .send_ok()
    .print_logs_structured();   // tree reads in your cast names
```

## Every send is its own transaction

Resending an identical instruction is always valid; the harness refreshes the blockhash before signing each send, so a repeated-send loop needs no ceremony:

```rust
for _ in 0..3 {
    ctx.tx(&[&session]).build(accs, vix::Execute { .. }).send_ok();
}
ctx.tx(&[&session])
    .build(accs, vix::Execute { .. })
    .send_err_named("RateLimitExceeded");
```

(On a real chain, two identical sends under one blockhash are one transaction arriving twice, and the second is rejected as already processed. That dedup is chain behavior worth knowing about and not something a test scenario means to invoke; if you specifically want to observe it, drive raw `litesvm` directly.)

This is the bridge from [Accounts as Actors](accounts-as-actors.md) to [Part IV](../inspect/cpi-tree.md): because the terminator carries the aliases forward onto the result, every view downstream of it speaks your cast's names. (Build a `Tx` and never call a terminator, and nothing happens: the instruction is just dropped. The terminator is the verb.)

**Prefer `send_err_named` whenever you know which error you expect.**

<details> <summary>Why single out <strong>send_err_named</strong>? </summary>

> It asserts the transaction failed *and* that the named error appears (substring-matched against the logs and the error field). That second half is what makes it a real test rather than a "something went wrong" check: if a refactor changes *which* guard fires, a bare `send_err` still passes (it failed, after all) while `send_err_named` breaks loudly and tells you the guard moved.
</details>

### Two builder escape hatches

Two `Tx` methods cover the negative path and the off-pattern case:

- **`.build_with(bundle, args, |accs| ...)`** runs your closure over the projected accounts struct before the metas are computed, so you can inject a *wrong* account on purpose. It's how you test that a guard rejects, say, a vault that isn't the maker's: build the valid bundle, then tamper one field in the closure, then `.send_err_named(...)`.
- **`.ix(instruction)`** drops a fully-formed `Instruction` into the chain instead of building one from a bundle. That lets `Tx` host things the bundle path can't express: a System instruction, a CPI from a different program, a hand-assembled ix. You still get the alias-aware terminators.

## Pre-built instructions, and the raw `Result`

When you already have an `Instruction` (from `build_ix`, or several you want to send atomically), send it directly on the context:

```rust
let ix1 = ctx.program().build_ix(accs1, vix::Deposit { amount: 1_000 });
let ix2 = ctx.program().build_ix(accs2, vix::Withdraw { amount: 400 });

ctx.send_ok(ix1, &[&signer]);                              // one pre-built ix, alias-aware
ctx.execute_instructions(vec![ix1, ix2], &[&signer]).unwrap();  // several, atomic, raw Result
```

`ctx.send_ok` / `send_err` / `send_err_named` are the same alias-aware terminators the `Tx` builder calls into; they just take a ready instruction. `ctx.execute_instruction` / `execute_instructions` are the plainer form: they return a `Result<TransactionResult, _>` you unwrap (or match on), for when a build step might legitimately error or you want to branch on the outcome yourself.

## What's in the result

The `TransactionResult` splits into read-only queries and chainable steps.

**Read-only** (`&self`, compose through `tap`):

```rust
result.is_success();          // bool
result.error();               // Option<&String>, the tx-level error
result.logs();                // &[String], the raw program logs
result.has_log("Transfer");   // bool: does any log contain this substring
result.find_log("amount: ");  // Option<&String>: first log matching
result.compute_units();       // u64 (see the compute chapter on the caveat)
result.fee();                 // u64 lamports
```

**Chainable** (take `self`, return `Self`, so they thread into one expression). The transaction-level assertions live here:

```rust
result
    .assert_success()                              // panics unless it succeeded
    .assert_success_with(|r| r.compute_units() < 200_000);  // outcome AND predicate

// the failure side:
result.assert_failure();                           // panics unless it failed
result.assert_failure_with(|r| r.error().is_some());
result.assert_error("EscrowExpired");              // failed AND error contains this string
result.assert_error_code(6001);                    // failed AND this numeric Anchor code
```

(The terminators above already assert the outcome, so you usually don't re-assert it; these are for when you hold a `TransactionResult` from a plain `execute_*` send, or want the extra predicate.) `assert_error` matches the error *name* (a substring, the human-readable Anchor name), while `assert_error_code` matches the numeric code. Use whichever your test reads more clearly; the name is usually more legible, the code more precise.

### `tap`: inspect without breaking the chain

The read-only queries are `&self`, so they don't fit a `self -> Self` chain directly. `tap` bridges that: it borrows the result for a closure and hands ownership back.

```rust
ctx.tx(&[&signer])
    .build(accs, args)
    .send_ok()
    .tap(|r| println!("CU used: {}", r.compute_units()))
    .print_logs_structured()
    .assert_success();
```

This is also how the [rendering methods](../inspect/cpi-tree.md) compose: `print_logs_structured`, `print_mermaid`, `print_authority_graph`, and friends are all chainable, so a single statement can send, render several ways, inspect, and assert.

## When you want to handle the error yourself

The terminators assert an outcome (and panic on the wrong one), which is what you want most of the time. When you'd rather branch on the result (exploring an unfamiliar program, say), drop to the plain send and match:

```rust
let ix = ctx.program().build_ix(accs, vix::Initialize { amount: 1 });
match ctx.execute_instruction(ix, &[&user]) {
    Ok(tx) if tx.is_success() => { /* ... */ }
    Ok(tx) => {
        println!("failed: {:?}", tx.error());
        tx.print_logs_structured();   // the tree is the fastest way to see why
    }
    Err(e) => println!("couldn't build/send: {e}"),
}
```

<details> <summary>Why are there <strong>two error layers</strong>? </summary>

> The outer `Err` is "couldn't even submit the transaction" (a malformed instruction, a signing problem); the inner `tx.error()` is "the transaction ran and the program rejected it." They're different failures and worth keeping straight.
</details>

## Succeeded is not verified

A green `send_ok()` tells you the transaction *ran without error*. It does **not** tell you the program did the right thing: a buggy handler can succeed while writing the wrong balance. That's the job of the world-state assertions in the [next chapter](assertions.md), and the reason a good test almost always pairs a `send_ok` with a handful of `ctx.svm.assert_*` checks on the resulting state.
