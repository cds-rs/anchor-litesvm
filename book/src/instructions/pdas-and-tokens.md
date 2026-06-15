# PDAs & Token Helpers

Most instructions need two kinds of address you don't have lying around: program-derived addresses (PDAs) and token accounts. `anchor-litesvm` has helpers for both, on `ctx.svm`, so you spend your setup naming actors rather than wrangling SPL boilerplate.

## PDAs

A PDA is derived from seeds plus a program id. For your own program's PDAs, `ctx.pda` supplies the id, so you pass only the seeds:

```rust
// Just the address (the common case):
let escrow = ctx.pda(&[b"escrow", maker.pubkey().as_ref(), &seed.to_le_bytes()]);

// Address and bump, when the instruction wants the bump too:
let (vault, bump) = ctx.pda_with_bump(&[b"vault", escrow.as_ref()]);
```

`ctx.pda` derives against this context's program id, so you can't hand it the wrong one. When you need *another* program's PDA (a Metaplex metadata account, say), reach for the generic `ctx.svm.get_pda(seeds, &program_id)`, which takes the id explicitly. (`Pubkey::find_program_address(&[...], &program_id)` is the same thing one layer down, if you'd rather not go through `ctx.svm`.)

Once you have the address, it's just another field in your bundle: `EscrowBundle { escrow, vault, .. }`. The bundle doesn't care whether a pubkey came from a keypair, an ATA derivation, or a PDA; it's all pubkeys by the time you build.

## Token helpers

The SPL setup dance, create a mint, create accounts to hold it, mint some supply, is four calls:

```rust
// A mint with 6 decimals, controlled by `authority`:
let mint = ctx.svm.create_token_mint(&authority, 6).unwrap();

// An associated token account (ATA) for an owner. Returns the ATA address:
let ata = ctx.svm.create_associated_token_account(&mint.pubkey(), &owner).unwrap();

// Mint supply into an account (authority must be the mint's authority):
ctx.svm.mint_to(&mint.pubkey(), &ata, &authority, 1_000_000).unwrap();

// Read a balance back (None if the account doesn't exist, e.g. after a close):
let balance = ctx.svm.token_balance(&ata).unwrap_or(0);
```

Two notes:

- `create_token_mint` *returns the mint keypair*, so you reach for `mint.pubkey()` when you need the address. `create_associated_token_account` returns the ATA *pubkey* directly (an ATA is a derived address, not a fresh keypair).
- `create_token_account` is the non-associated cousin (a token account at a fresh keypair rather than the derived ATA), for the rarer cases that need one.
- These helpers return a `Result`, so each call ends in `.unwrap()` or `?`. This book uses `.unwrap()` because its tests return `()`; give a test a `-> Result<(), Box<dyn std::error::Error>>` signature instead and `?` works just as well. It's a per-suite style choice, not a framework requirement; you'll see both spellings in the wild (the [migration appendix](../appendix/migration.md) leans on `?`), and the helpers don't care which you pick.

<details> <summary>Need <strong>deterministic</strong> addresses for committed output? </summary>

The [Deterministic Identities](../intro/determinism.md) concept covers this in full; in short: by default, generated keypairs (and the mints and PDAs derived from them) are random each run. That's fine for most tests, but it means any *committed* test output (a structured-log snapshot, a report) churns its addresses on every run and won't diff cleanly.

When you want stable addresses, `create_token_mint_at` takes a caller-supplied mint keypair, and `deterministic_keypair("myapp/v1", "mint:x")` produces a stable keypair from a namespace and label:

```rust
use anchor_litesvm::deterministic_keypair;

let mint_kp = deterministic_keypair("escrow/v1", "mint:a");
ctx.svm.create_token_mint_at(&authority, &mint_kp, 6).unwrap();
// mint_kp.pubkey() is the same on every run, so derived ATAs/PDAs are too
```

In a test context, `ctx.cast_mint("A", &authority, 6)` rolls that pair into one call: it derives the mint from its name, creates it, and aliases it.

You don't need this to write tests; reach for it only when you're committing rendered output and want it to diff cleanly. (It also stabilizes the compute-unit drift discussed in [Reading Compute & Fees](../inspect/compute-fees.md), since stable pubkeys mean a stable PDA bump search.)

</details>

## Putting it together

A token-transfer test wires all of this: set up the mints and accounts, name them in a bundle, send, and [assert](../running/assertions.md).

```rust
let mint = ctx.svm.create_token_mint(&authority, 9).unwrap();
let from = ctx.svm.create_associated_token_account(&mint.pubkey(), &alice).unwrap();
let to   = ctx.svm.create_associated_token_account(&mint.pubkey(), &bob).unwrap();
ctx.svm.mint_to(&mint.pubkey(), &from, &authority, 1_000_000).unwrap();

// `token_program` isn't in the bundle: it's auto-injected from the
// accounts struct's `Interface<TokenInterface>` field.
let accs = TransferBundle { from, to, authority: alice.pubkey() };
ctx.tx(&[&alice])
    .build(accs, vix::Transfer { amount: 500_000 })
    .send_ok();

ctx.svm.assert_token_balance(&from, 500_000);
ctx.svm.assert_token_balance(&to, 500_000);
```
