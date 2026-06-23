//! End-to-end demo of the authority and ownership graph renderers.
//!
//! Builds and sends a real associated-token-account creation, then prints the
//! structured CPI tree, the authority graph, and the ownership graph for the
//! same transaction.
//!
//! ATA creation is a good showcase on two counts:
//!
//!   1. It CPIs: `AssociatedToken -> System (CreateAccount) -> Token
//!      (InitializeImmutableOwner, InitializeAccount3)`, so the graphs have
//!      real depth to walk.
//!   2. The account it creates is *written* by the System program (which does
//!      the `CreateAccount`) but ends up *owned* by the Token program. That
//!      owner-vs-writer gap is exactly what the ownership graph exists to make
//!      visible, and it is the case for "give litesvm the metadata" since you
//!      cannot see the owner from the logs alone.
//!
//! Run: `cargo run -p anchor-litesvm --example account_graphs`

use {
    anchor_litesvm::{ActorRegistry, Aliases, TestHelpers, TransactionHelpers},
    litesvm::LiteSVM,
    solana_signer::Signer,
    spl_associated_token_account::{
        get_associated_token_address, instruction::create_associated_token_account,
    },
};

fn main() {
    // No custom program needed: this demo exercises the built-in SPL CPIs only,
    // so a plain LiteSVM (which bundles the System / Token / AssociatedToken
    // programs) is enough.
    let mut svm = LiteSVM::new();

    // Seeded actors (deterministic keypairs) so the addresses in the graphs below
    // are identical on every run: the output does not churn, which turns it into a
    // committable snapshot where a diff means the behavior changed (worth
    // scrutinizing), not just that the keypairs rolled. payer funds + signs;
    // wallet owns the new ATA.
    let actors = ActorRegistry::new("account-graphs/v1");
    let payer = actors.keypair("payer");
    let wallet = actors.keypair("wallet");
    let mint_authority = actors.keypair("mint_authority");
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&wallet.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&mint_authority.pubkey(), 1_000_000_000)
        .unwrap();
    let mint = actors.keypair("mint");
    svm.create_token_mint_at(&mint_authority, &mint, 9).unwrap();
    let ata = get_associated_token_address(&wallet.pubkey(), &mint.pubkey());

    // Friendly names so the graphs read in English. `with_well_known` seeds
    // the program ids (System, Token, AssociatedToken, ...).
    let aliases = Aliases::with_well_known()
        .with(payer.pubkey(), "payer")
        .with(wallet.pubkey(), "wallet")
        .with(mint.pubkey(), "mint")
        .with(ata, "wallet_ata");

    let ix = create_associated_token_account(
        &payer.pubkey(),
        &wallet.pubkey(),
        &mint.pubkey(),
        &spl_token::id(),
    );

    let result = svm
        .send_instruction(ix, &[&payer])
        .expect("ATA creation should succeed")
        .with_aliases(aliases);

    result
        .tap(|_| println!("\n========== Structured CPI tree =========="))
        .print_logs_structured()
        .tap(|_| {
            println!("\n========== Authority graph: who signs what, who writes what ==========")
        })
        .print_authority_graph()
        .tap(|_| println!("\n========== Ownership graph: who owns what was written =========="))
        .print_ownership_graph();
}
