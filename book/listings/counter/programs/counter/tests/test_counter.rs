// ANCHOR: test
// tests/test_counter.rs
use anchor_litesvm::{AnchorLiteSVM, Signer, TestHelpers};
use counter::{
    instruction as vix,
    state::Counter,
    test_helpers::{IncrementBundle, InitializeBundle},
};

#[test]
fn initialize_then_increment() {
    // Arrange: deploy the program and cast the one actor this test needs.
    // `cast_actor_with_sol` derives Alice's keypair from (program id, name),
    // funds her, and aliases her, so every identity below is byte-stable.
    let mut ctx = AnchorLiteSVM::build_with_program(
        counter::ID,
        "counter",
        include_bytes!("../../../target/deploy/counter.so"),
    );
    let alice = ctx.cast_actor_with_sol("Alice", 10_000_000_000);

    // The counter is a PDA off Alice's key; derive it and name it once.
    let counter = ctx.svm.get_pda(&[b"counter", alice.pubkey().as_ref()], &counter::ID);
    ctx.alias(counter, "Counter");

    // Act + assert: initialize the counter at 0, then increment it to 1.
    // `system_program` is absent from the bundle; it auto-injects.
    let accs = InitializeBundle { payer: alice.pubkey(), counter };
    ctx.tx(&[&alice]).build(accs, vix::Initialize { start: 0 }).send_ok();
    assert_eq!(ctx.load::<Counter>(&counter).count, 0);

    let accs = IncrementBundle { counter, payer: alice.pubkey() };
    ctx.tx(&[&alice]).build(accs, vix::Increment {}).send_ok();
    assert_eq!(ctx.load::<Counter>(&counter).count, 1);
}
// ANCHOR_END: test
