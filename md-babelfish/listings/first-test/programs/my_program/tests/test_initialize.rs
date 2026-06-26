// ANCHOR: test
// tests/test_initialize.rs
use anchor_litesvm::{AnchorLiteSVM, Signer, TestHelpers};
use my_program::{instruction as vix, state::Data, test_helpers::InitAccs};

#[test]
fn test_my_first_instruction() {
    // 1. Setup: one-line deployment.
    let mut ctx = AnchorLiteSVM::build_with_program(
        my_program::ID,
        "my_program",
        include_bytes!("../../../target/deploy/my_program.so"),
    );

    // 2. A funded signer, and the PDA the program creates off its key.
    // Name them, so every rendered view below reads in your words, not base58.
    let user = ctx.svm.create_funded_account(10_000_000_000).unwrap();
    let data = ctx.svm.get_pda(&[b"data", user.pubkey().as_ref()], &my_program::ID);
    ctx.alias(user.pubkey(), "user").alias(data, "data");

    // 3 + 4. Build and send in one chain. The bundle names the accounts;
    // `system_program` is auto-filled, so it isn't here.
    let accs = InitAccs { user_account: user.pubkey(), data };
    let result = ctx
        .tx(&[&user])
        .build(accs, vix::Initialize { value: 42 })
        .send_ok();

    // 5. Verify the stored value.
    assert_eq!(ctx.load::<Data>(&data).value, 42);

    // 6. A passing test is the floor. The framework recorded the whole
    // transaction; render what it gave back.
    result
        .print_logs_structured()
        .print_mermaid()
        .print_authority_graph()
        .print_ownership_graph(&ctx.svm);
}
// ANCHOR_END: test
