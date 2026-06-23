//! Shared test scaffolding for the escrow integration tests.
//!
//! The `EscrowBundle` and its `BundledPubkeys` / `AliasMirror` derives live in
//! `escrow::test_helpers` (the program crate, so the per-ix `BundledPubkeys`
//! impls on the Accounts structs satisfy the orphan rule). This module
//! re-exports the bundle and provides [`setup`], which builds a ready-to-use
//! escrow scenario with deterministic identities.
//!
//! Two actors (maker, taker) and two mints, each cast by name so the emitted
//! reports and structured logs are byte-stable across runs. `cast_actor` and
//! `cast_mint` derive every identity deterministically from its name and alias
//! it in one call, so the cast list and the setup are the same lines.
//!
//! NOTE (compilation dimension): this module is compiled into all three test
//! binaries, and not every binary reads every field/helper. Silence the
//! per-binary dead-code noise for this scaffolding module.
#![allow(dead_code)]

use anchor_litesvm::{
    AnchorContext, Keypair, MarkdownBlock, Pubkey, Signer, TestHelpers, ToMarkdown,
};
// anchor-litesvm has no pure ATA-derivation helper (it only offers
// `create_associated_token_account`, which *creates*), so we keep a direct dep
// to derive the vault and the init-if-needed ATAs *before* the program creates
// them. (Candidate to hoist as `TestHelpers::ata(owner, mint)`.)
use spl_associated_token_account::get_associated_token_address;

// Re-exported so tests keep importing from `common::EscrowBundle`.
pub use escrow::test_helpers::EscrowBundle;

/// Scenario constants. `DEPOSIT != RECEIVE` and the two mints differ in decimals
/// so a `take` bug that confused `escrow.receive`/`mint_b.decimals` with
/// `vault.amount`/`mint_a.decimals` could not stay hidden.
pub const SEED: u64 = 42;
pub const MINT_A_DECIMALS: u8 = 6;
pub const MINT_B_DECIMALS: u8 = 9;
pub const DEPOSIT: u64 = 1_000_000;
pub const RECEIVE: u64 = 2_000_000_000;

/// A ready-to-use escrow scenario: the populated [`EscrowBundle`], the maker and
/// taker signers, and the two mint pubkeys.
pub struct EscrowWorld {
    pub bundle: EscrowBundle,
    pub maker: Keypair,
    pub taker: Keypair,
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
}

/// Build the scenario: funded maker/taker, two mints with distinct decimals at
/// deterministic keypairs, the maker's and taker's funded source ATAs, and every
/// derived address (escrow PDA, vault, and the two `init_if_needed` ATAs).
///
/// Panics on any infrastructure failure; a panic here is a broken fixture, not a
/// failure of the code under test.
// ANCHOR: setup
// tests/common/mod.rs
pub fn setup(ctx: &mut AnchorContext, seed: u64) -> EscrowWorld {
    // The trade this scenario sets up: the maker locks DEPOSIT of mint A and
    // wants RECEIVE of mint B in return; a taker holding mint B fills it. We
    // build both sides so a test can run make / take / refund against a real
    // cast.

    // The two parties. `cast_actor` rolls the deterministic keypair, the SOL
    // airdrop, and the alias into one call, so each name appears once.
    let maker = ctx.cast_actor("Maker");
    let taker = ctx.cast_actor("Taker");

    // The two tokens being traded, at distinct decimals so a take bug that
    // confused mint A with mint B couldn't hide. Each party is the authority
    // for the mint it brings. `cast_mint` derives the mint at a deterministic
    // address, creates it, and registers the leaf alias ("A", "B").
    let mint_a = ctx.cast_mint("A", &maker, MINT_A_DECIMALS);
    let mint_b = ctx.cast_mint("B", &taker, MINT_B_DECIMALS);

    // What each party brings to the trade, funded in their own ATA: the maker's
    // mint-A deposit (what `make` locks away) and the taker's mint-B payment
    // (what `take` hands over).
    let maker_ata_a = ctx.svm.create_associated_token_account(&mint_a, &maker).unwrap();
    ctx.svm.mint_to(&mint_a, &maker_ata_a, &maker, DEPOSIT).unwrap();
    let taker_ata_b = ctx.svm.create_associated_token_account(&mint_b, &taker).unwrap();
    ctx.svm.mint_to(&mint_b, &taker_ata_b, &taker, RECEIVE).unwrap();

    let maker_key = maker.pubkey();
    let taker_key = taker.pubkey();

    // Addresses the program owns or creates, derived now but not yet on chain:
    // `escrow` is the PDA holding the terms; `vault` is the escrow's own ATA for
    // mint A, custodying the locked deposit; `taker_ata_a` and `maker_ata_b` are
    // the settlement destinations, created `init_if_needed` during `take`, so we
    // only derive their addresses here.
    let escrow = ctx
        .svm
        .get_pda(&[escrow::ESCROW_SEED, maker_key.as_ref(), &seed.to_le_bytes()], &escrow::ID);
    let vault = get_associated_token_address(&escrow, &mint_a);
    let taker_ata_a = get_associated_token_address(&taker_key, &mint_a);
    let maker_ata_b = get_associated_token_address(&maker_key, &mint_b);

    // The full cast as pubkeys: the bundle every instruction builds from.
    let bundle = EscrowBundle {
        maker: maker_key,
        taker: taker_key,
        mint_a,
        mint_b,
        maker_ata_a,
        maker_ata_b,
        taker_ata_a,
        taker_ata_b,
        escrow,
        vault,
    };

    // The leaves named themselves as they were cast (Maker, Taker, A, B); name
    // the escrow PDA, then compose each token-account name from its owner and
    // mint with `alias_ata`, so the trace reads "Maker/A", "Escrow/A" (the
    // vault), and so on. The cast order already aliased every leaf before these
    // ATAs compose off them.
    ctx.alias(escrow, "Escrow");
    ctx.alias_ata(&maker_key, &mint_a); // Maker/A
    ctx.alias_ata(&maker_key, &mint_b); // Maker/B
    ctx.alias_ata(&taker_key, &mint_a); // Taker/A
    ctx.alias_ata(&taker_key, &mint_b); // Taker/B
    ctx.alias_ata(&escrow, &mint_a); // Escrow/A (the vault)

    EscrowWorld { bundle, maker, taker, mint_a, mint_b }
}
// ANCHOR_END: setup

/// A frozen, render-ready view of the escrow's token accounts. `None` (account
/// doesn't exist yet, e.g. an init-if-needed ATA before settlement) renders `—`,
/// distinct from a present-but-empty `Some(0)`.
///
/// Hand-rolled here, as in the AMM (`Balances`) and vault (`SolBalances`): this
/// is the THIRD consumer to write the same "labelled balances -> kv table"
/// `ToMarkdown`. Strong signal that a generic balances view belongs upstream.
pub struct TokenBalances {
    rows: Vec<(String, Option<u64>)>,
}

impl TokenBalances {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn row(mut self, label: &str, amount: Option<u64>) -> Self {
        self.rows.push((label.to_string(), amount));
        self
    }
}

impl ToMarkdown for TokenBalances {
    fn to_markdown(&self) -> MarkdownBlock {
        MarkdownBlock::kv(
            ["account", "amount"],
            self.rows
                .iter()
                .map(|(k, v)| (k.clone(), v.map_or_else(|| "—".into(), |n| n.to_string()))),
        )
    }
}

/// Snapshot the five token accounts that the escrow flow moves between: the
/// maker's and taker's holdings of each mint, plus the vault.
pub fn balances(ctx: &AnchorContext, w: &EscrowWorld) -> TokenBalances {
    TokenBalances::new()
        .row("Maker mint_a", ctx.svm.token_balance(&w.bundle.maker_ata_a))
        .row("Maker mint_b", ctx.svm.token_balance(&w.bundle.maker_ata_b))
        .row("Taker mint_a", ctx.svm.token_balance(&w.bundle.taker_ata_a))
        .row("Taker mint_b", ctx.svm.token_balance(&w.bundle.taker_ata_b))
        .row("Vault mint_a", ctx.svm.token_balance(&w.bundle.vault))
}
