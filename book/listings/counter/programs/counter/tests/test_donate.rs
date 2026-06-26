// ANCHOR: test
// tests/test_donate.rs
use anchor_litesvm::{AnchorLiteSVM, AssertionHelpers, Signer};
use counter::{instruction as vix, test_helpers::DonateBundle};

#[test]
fn donate_transfers_tokens() {
    let mut ctx = AnchorLiteSVM::build_with_program(
        counter::ID,
        "counter",
        include_bytes!("../../../target/deploy/counter.so"),
    );

    // Model the token world deterministically: a mint Alice controls, a donor
    // ATA funded with 1 token, an empty recipient ATA for Bob.
    let alice = ctx.cast_actor_with_sol("Alice", 10_000_000_000);
    let bob = ctx.cast_actor("Bob");
    let mint = ctx.cast_mint("USDC", &alice, 6);
    let donor_ata = ctx.fund_ata(&alice, &mint, &alice, 1_000_000);
    let recipient_ata = ctx.fund_ata(&bob, &mint, &alice, 0);

    // `token_program` is absent from the bundle; it auto-injects, exactly like
    // `system_program` did for the counter.
    let accs = DonateBundle { donor: alice.pubkey(), mint, donor_ata, recipient_ata };
    ctx.tx(&[&alice]).build(accs, vix::Donate { amount: 250_000 }).send_ok();

    ctx.svm.assert_token_balance(&donor_ata, 750_000);
    ctx.svm.assert_token_balance(&recipient_ata, 250_000);
}
// ANCHOR_END: test
