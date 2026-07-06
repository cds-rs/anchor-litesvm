//! Book capture: deploy the escrow program, run make + take as Alice/Bob,
//! and snapshot the rendered CPI tree (the SPL Token transfers nested under
//! `escrow::take`) into `book/src/captured/escrow_take.txt`.
// declare_program!'s expansion gates on-chain-only branches with
// `cfg(target_os = "solana")` and `cfg(feature = "idl-build")`; off-chain those
// compile out, but check-cfg doesn't know the names, so silence the noise here.
#![allow(unexpected_cfgs)]
mod common;

// `self` binds the crate name into this test's root scope; declare_program!'s
// generated modules reach `anchor_lang` via `super::`, so it must be nameable
// here (without it, the expansion fails with "no `anchor_lang` in the root").
use anchor_lang::prelude::Pubkey;
use anchor_lang::{self};
use anchor_litesvm::AnchorLiteSVM;
use litesvm_utils::TestHelpers;
use solana_signer::Signer;

anchor_lang::declare_program!(escrow);
anchor_litesvm::bundles_from_idl!(escrow);

// escrow's Cargo graph doesn't reach `anchor-spl`, so there's no `anchor_spl::
// token::ID` to borrow; the classic SPL Token program id is a well-known
// constant instead (the mints `cast_mint`/`fund_ata` create are classic SPL,
// not Token-2022).
const TOKEN_PROGRAM: Pubkey = Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

fn boot() -> anchor_litesvm::AnchorContext {
    AnchorLiteSVM::build_with_program(escrow::ID, "escrow", &common::fixture_bytes("escrow"))
}

#[test]
fn escrow_make_then_take() {
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

    // Bob now holds the offered MintA. The vendored escrow swallows some CPI
    // `Result`s (`let _ = transfer_checked(...)`), so a silently-failed
    // transfer would show up as a wrong balance here: this assertion is the
    // real guard, not the `send_ok()` above.
    let bob_a = ctx.alias_ata(&taker.pubkey(), &mint_a);
    assert_eq!(ctx.svm.token_balance(&bob_a), Some(1_000_000));

    common::expect_capture("escrow_take", &result.tree_string());
}
