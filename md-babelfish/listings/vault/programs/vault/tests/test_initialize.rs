// ANCHOR: setup
// tests/test_initialize.rs
use anchor_litesvm::{
    AnchorContext, AnchorLiteSVM, AssertionHelpers, Keypair, Pubkey, Report, Signer,
};
use vault::{instruction as vix, state::VaultState, test_helpers::VaultAccs};

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

fn setup() -> (AnchorContext, Keypair) {
    // The world every vault test starts from: the deployed program and one
    // funded actor, Alice, who opens a vault, moves SOL through it, and closes
    // it. The vault's two PDAs depend on Alice's key, so each test derives them
    // itself (see `pdas`).
    let mut ctx = AnchorLiteSVM::build_with_program(
        vault::ID,
        "Vault",
        include_bytes!("../../../target/deploy/vault.so"),
    );
    let alice = ctx.cast_actor_with_sol("Alice", 10 * LAMPORTS_PER_SOL);
    (ctx, alice)
}
// ANCHOR_END: setup

#[test]
fn lifecycle_initialize_deposit_withdraw_close() {
    let (mut ctx, alice) = setup();
    let user = alice.pubkey();

    // ANCHOR: pdas
    let (vault_state_pda, state_bump) =
        Pubkey::find_program_address(&[b"state", user.as_ref()], &vault::ID);
    let (vault_pda, vault_bump) =
        Pubkey::find_program_address(&[b"vault", vault_state_pda.as_ref()], &vault::ID);

    ctx.alias(vault_pda, "vault").alias(vault_state_pda, "vault_state");

    let accs = VaultAccs { user, vault: vault_pda, vault_state: vault_state_pda };
    // ANCHOR_END: pdas

    // ANCHOR: init
    ctx.tx(&[&alice]).build(accs, vix::Initialize {}).send_ok();

    let state: VaultState = ctx.load(&vault_state_pda);
    assert_eq!(state.vault_bump, vault_bump);
    assert_eq!(state.state_bump, state_bump);
    // ANCHOR_END: init

    // ANCHOR: deposit
    let deposit = LAMPORTS_PER_SOL;
    ctx.tx(&[&alice]).build(accs, vix::Deposit { amount: deposit }).send_ok();
    ctx.svm.assert_sol_balance(&vault_pda, deposit);
    // ANCHOR_END: deposit

    // ANCHOR: withdraw
    let withdraw = LAMPORTS_PER_SOL / 2;
    ctx.tx(&[&alice]).build(accs, vix::Withdraw { amount: withdraw }).send_ok();
    ctx.svm.assert_sol_balance(&vault_pda, deposit - withdraw);
    // ANCHOR_END: withdraw

    // ANCHOR: close
    let alice_before_close = ctx.svm.get_balance(&user).unwrap();
    ctx.tx(&[&alice]).build(accs, vix::Close {}).send_ok();

    assert!(!ctx.account_exists(&vault_pda));
    assert!(!ctx.account_exists(&vault_state_pda));
    assert!(ctx.svm.get_balance(&user).unwrap() > alice_before_close);
    // ANCHOR_END: close

    // ANCHOR: report
    // One call recovers the execution snapshot from the four sends above: the
    // authority flow (who signed each transfer, and which transfers the program
    // signed as the vault PDA), the account index (every account by owner), and
    // the structured logs. It writes target/md-reports/<slug>.md, byte-stable
    // because every identity is deterministic.
    let mut md = Report::new(
        "Vault: the deposit and withdraw lifecycle",
        "Alice opens a vault, deposits a SOL, withdraws half, then closes it. \
         The views below are recovered from the executed transactions, not hand-drawn.",
    );
    ctx.report_execution(&mut md);
    // ANCHOR_END: report
}

// ANCHOR: negative
// tests/test_initialize.rs
#[test]
fn initialize_rejects_placeholder_pdas() {
    let (mut ctx, alice) = setup();

    // Pin only the signer; `vault` and `vault_state` fall to throwaway
    // placeholders that can't match the PDAs the program derives.
    let accs = VaultAccs { user: alice.pubkey(), ..VaultAccs::default() };

    ctx.tx(&[&alice])
        .build(accs, vix::Initialize {})
        .send_err_named("ConstraintSeeds");
}
// ANCHOR_END: negative
