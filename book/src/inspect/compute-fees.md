# Reading Compute & Fees

Every rendered transaction ends with a numeric footer:

```text
Compute Units (this run): 15017
Fee: 5000 lamports
```

Small as it is, there's a real caveat baked into the wording, and a right and wrong way to assert on it.

## `compute_units()` and asserting a budget

The total is available programmatically:

```rust
let result = ctx.tx(&[&signer]).build(accs, vix::Swap { amount }).send_ok();
let cu = result.compute_units();
assert!(cu < 200_000, "used too many compute units");
```

Better, fold the check into the chain with `assert_success_with`, which asserts the outcome *and* a predicate in one step (the `_with` suffix follows `Vec::with_capacity`'s convention):

```rust
ctx.tx(&[&signer])
   .build(accs, vix::Swap { amount })
   .send_ok()
   .assert_success_with(|r| r.compute_units() < 200_000);
```

Or inspect without breaking the chain using `tap`, which borrows the result for a closure and hands ownership back:

```rust
ctx.tx(&[&signer])
   .build(accs, vix::Swap { amount })
   .send_ok()
   .tap(|r| println!("CU used: {}", r.compute_units()))
   .assert_success();
```

<details> <summary>Why does it say <strong>(this run)</strong>? </summary>

> The footer says `Compute Units (this run)` on purpose. Per-frame compute is *not stable across runs*. Anchor's `find_program_address` iterates a variable number of bumps depending on the pubkeys involved, and your test's accounts are usually freshly generated each run, so the bump search (and therefore the CU) drifts.
>
> You saw this drift in the previous chapters without it being flagged: the [tree](cpi-tree.md) chapter's capture read `15017cu` and the [Mermaid](mermaid.md) chapter's read `22517cu`, for the same logical ATA creation. Same transaction, two runs, different random pubkeys, different CU. Neither is wrong.
>
> The practical rules that fall out of this:
>
> - **Assert a ceiling, not equality.** `cu < 200_000` is a meaningful regression guard; `cu == 15017` is a flaky test waiting to happen.
> - **Read a moved number as program drift, not a regression.** A different count across runs comes from the program's PDA derivation, not from `anchor-litesvm`.
> - **Pin your accounts if you need determinism.** If you derive PDAs from fixed seeds (rather than random keypairs), the bump search is stable and so is the CU. That's the only way to a fixed number, and it's rarely worth it just to tighten a budget assertion.
</details>

<details> <summary>What does <strong><code>(no cu)</code></strong> mean? </summary>

In a tree you'll see frames annotated `(no cu)`:

```text
├── System::CreateAccount [2] ✓ (no cu)
```

That's not "consumed zero compute"; it's "this program emitted no compute line at all." Native programs (System, and some others) don't print the `consumed N of M` line that Anchor programs do, so there's no number to show. The tree renders the absence explicitly rather than printing `0cu` (which would be a lie) or nothing (which would look like a parser drop).

</details>

## A note for this book's numbers

Following the project's convention, this book does not treat specific CU counts as fixed facts. The numbers in the rendered samples are real captures, shown to illustrate the *format*, and they'll differ on your machine and between runs. CU values track the *program under test*, not the framework: if a number moves unexpectedly, look at the program, not at `anchor-litesvm`.
