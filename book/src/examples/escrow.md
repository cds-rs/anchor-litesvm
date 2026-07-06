# Escrow

The escrow program has three instructions: `make` creates an escrow PDA and
deposits `mint_a` into its vault, `take` lets a counterparty pay `mint_b` and
receive the vault's `mint_a`, and `refund` returns the deposit to the maker.
Every escrow carries a 90-day expiry: `take` stops working after it, `refund`
only works after it. `make` and `take` also drive real SPL Token CPIs
(transfers, and `init_if_needed` associated-token-account creation). This
chapter drives all three through `anchor-litesvm`.

## Boot and make -> take

```rust
fn boot() -> anchor_litesvm::AnchorContext {
    AnchorLiteSVM::build_with_program(escrow::ID, "escrow", &common::fixture_bytes("escrow"))
}
```

```rust
let mut ctx = boot();
let maker = ctx.cast_actor("Alice"); // Alice makes the escrow
let taker = ctx.cast_actor("Bob"); // Bob takes it
let mint_a = ctx.cast_mint("MintA", &maker, 6);
let mint_b = ctx.cast_mint("MintB", &maker, 6);

// Fund Alice with MintA (offered) and Bob with MintB (wanted).
let _alice_a = ctx.fund_ata(&maker, &mint_a, &maker, 1_000_000);
let _bob_b = ctx.fund_ata(&taker, &mint_b, &maker, 1_000_000);

// `escrow`'s PDA seeds an ix-arg (`seed`) that the IDL's own emitted
// seed-path names `seeds` (a vendored-source quirk), so the macro can't
// resolve it at build time and demotes it to a plain bundle field: the
// caller derives and supplies it directly, here and again in `take`.
let seed = 42u64;
let (escrow_pda, _bump) = Pubkey::find_program_address(
    &[b"escrow", maker.pubkey().as_ref(), &seed.to_le_bytes()],
    &escrow::ID,
);

ctx.tx(&[&maker])
    .build(
        MakeBundle {
            maker: maker.pubkey(),
            mint_a,
            mint_b,
            token_program: TOKEN_PROGRAM,
            escrow: escrow_pda,
        },
        escrow::client::args::Make {
            seed,
            receive: 1_000_000,
            deposit: 1_000_000,
        },
    )
    .send_ok();

// take: Bob pays MintB to Alice and receives MintA from the vault.
let result = ctx
    .tx(&[&taker])
    .build(
        TakeBundle {
            taker: taker.pubkey(),
            maker: maker.pubkey(),
            mint_a,
            mint_b,
            token_program: TOKEN_PROGRAM,
            escrow: escrow_pda,
        },
        escrow::client::args::Take {},
    )
    .send_ok();
```

`bundles_from_idl!` derives most of `make` and `take`'s accounts (the vault,
the ATAs, the escrow PDA itself where it can); `escrow` is the one exception,
because the program's own seed path shadows the arg name the macro looks for,
so the caller computes `escrow_pda` with `find_program_address` and passes it
like any other bundle field.

`result.tree_string()` renders the transaction as a CPI tree:

```text
{{#include ../captured/escrow_take.txt}}
```

The two `AssociatedToken` frames are `init_if_needed` creating the taker's
and maker's associated token accounts; the `Token`/`System` frames nested
inside each are the ATA program's own calls to size, fund, and initialize the
account. The three `Token` frames after that are `take`'s own CPIs, in
program order: Bob pays Alice `mint_b`, the vault pays Bob `mint_a`, and the
vault account closes (rent back to Alice).

## Time-lock

`litesvm_utils::TestHelpers::advance_days` warps the SVM clock forward. Push
past the 90-day expiry and `take` is rejected before either ATA transfer:

```rust
let ix = ctx.program().build_ix(
    TakeBundle {
        taker: taker.pubkey(),
        maker: maker.pubkey(),
        mint_a,
        mint_b,
        token_program: TOKEN_PROGRAM,
        escrow: escrow_pda,
    },
    escrow::client::args::Take {},
);

// The escrow expires 90 days after make. Jump 91 days forward.
ctx.svm.advance_days(91);

let result = ctx.send_err_named(ix, &[&taker], "EscrowExpired");
```

```text
{{#include ../captured/escrow_expired.txt}}
```

Both `AssociatedToken` frames still run and succeed (`init_if_needed` doesn't
care about the expiry); the `✗` leaf is the program's own expiry check,
`EscrowExpired`, which fails the whole transaction after the ATAs are already
created.

Refund is the mirror image: it only works *after* expiry, so calling it
inside the window is rejected too:

```rust
// No time warp: still inside the 90-day window, so refund must be rejected.
// `refund` doesn't sign with `maker` (it's a plain `SystemAccount`), but the
// transaction still needs a fee-payer signer, so `maker` signs in that role.
let ix = ctx.program().build_ix(
    RefundBundle {
        maker: maker.pubkey(),
        mint_a,
        token_program: TOKEN_PROGRAM,
        escrow: escrow_pda,
    },
    escrow::client::args::Refund {},
);
let result = ctx.send_err_named(ix, &[&maker], "EscrowNotExpired");
```

```text
{{#include ../captured/escrow_refund_too_early.txt}}
```

## The escape hatch

`build_ix_with` builds every account honestly, then hands you a closure to
override exactly one slot. Mallory wants Bob's `take` to pay out to her
instead of the vault: she initializes her own, genuinely-owned `mint_a`
associated token account, then submits `take` with the `vault` slot pointed
at it:

```rust
// Mallory owns a real, initialized mint_a token account (the
// confused-deputy setup: valid in every way except its authority is
// Mallory, not the escrow PDA). Zero balance is fine; it only needs to
// exist and deserialize. `maker` is the mint authority as elsewhere.
let mallory_vault = ctx.fund_ata(&mallory, &mint_a, &maker, 0);

// Point vault at Mallory's ATA instead of the escrow PDA's. The bundle
// derives every account honestly; the closure then swaps exactly the
// vault slot.
let ix = ctx.program().build_ix_with(
    TakeBundle {
        taker: taker.pubkey(),
        maker: maker.pubkey(),
        mint_a,
        mint_b,
        token_program: TOKEN_PROGRAM,
        escrow: escrow_pda,
    },
    escrow::client::args::Take {},
    |accounts| accounts.vault = mallory_vault,
);

let result = ctx.send_err_named(ix, &[&taker], "ConstraintTokenOwner");
```

```text
{{#include ../captured/escrow_wrong_vault.txt}}
```

Mallory's ATA deserializes fine: real mint, real token account, right
discriminator. Both `AssociatedToken` frames still succeed, same as the
happy path. What catches it is `vault`'s `associated_token::authority =
escrow` constraint: the token account's owner is Mallory, not the escrow
PDA, so Anchor rejects with `ConstraintTokenOwner`. Same confused-deputy
lesson as the vault chapter's `ConstraintSeeds`: a substituted account valid
in every way except who it belongs to, caught by the one constraint that
checks.

The full test is `crates/anchor-litesvm/tests/book_escrow.rs`.
