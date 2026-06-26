use super::*;
use crate::test_helpers::TestHelpers;
use solana_system_interface::instruction as system_instruction;

#[test]
fn test_transaction_result_success() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    // Create a simple transfer instruction
    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);

    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    assert!(result.is_success());
    assert_eq!(result.error(), None);
    result.assert_success();
}

#[test]
fn test_transaction_result_has_log() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    // System program logs typically contain "invoke" messages
    assert!(result.has_log("invoke"));
}

#[test]
fn test_transaction_result_find_log() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    // Should find a log containing "invoke"
    let log = result.find_log("invoke");
    assert!(log.is_some());
}

#[test]
fn test_transaction_result_failure() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new(); // Unfunded account

    // This should fail due to insufficient funds
    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    assert!(!result.is_success());
    assert!(result.error().is_some());
}

#[test]
fn test_transaction_result_assert_failure() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new(); // Unfunded account

    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    // Should not panic when asserting failure on a failed transaction
    result.assert_failure();
}

#[test]
#[should_panic(expected = "Expected transaction to fail")]
fn test_transaction_result_assert_failure_on_success() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();

    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    // Should panic when asserting failure on a successful transaction
    result.assert_failure();
}

#[test]
fn test_transaction_result_assert_error() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new(); // Unfunded account

    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    // Should contain "AccountNotFound" in the error (account doesn't exist)
    result.assert_error("AccountNotFound");
}

#[test]
#[should_panic(expected = "Expected error containing 'this error does not exist'")]
fn test_transaction_result_assert_error_wrong_message() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new(); // Unfunded account

    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    // Should panic when expecting wrong error message
    result.assert_error("this error does not exist");
}

#[test]
fn test_send_multiple_instructions() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient1 = Keypair::new();
    let recipient2 = Keypair::new();

    // Send two transfers in one transaction
    let ix1 = system_instruction::transfer(&payer.pubkey(), &recipient1.pubkey(), 1_000_000);
    let ix2 = system_instruction::transfer(&payer.pubkey(), &recipient2.pubkey(), 2_000_000);

    let result = svm.send_instructions(&[ix1, ix2], &[&payer]).unwrap();
    result.assert_success();

    // Verify both transfers succeeded
    let balance1 = svm.get_balance(&recipient1.pubkey()).unwrap();
    let balance2 = svm.get_balance(&recipient2.pubkey()).unwrap();
    assert_eq!(balance1, 1_000_000);
    assert_eq!(balance2, 2_000_000);
}

#[test]
fn test_send_instruction_no_signers() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);

    // Should error when no signers provided
    let result = svm.send_instruction(ix, &[]);
    assert!(result.is_err());
    match result {
        Err(TransactionError::BuildError(msg)) => {
            assert!(msg.contains("No signers"));
        }
        _ => panic!("Expected BuildError"),
    }
}

#[test]
fn test_send_instructions_no_signers() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);

    // Should error when no signers provided
    let result = svm.send_instructions(&[ix], &[]);
    assert!(result.is_err());
}

#[test]
fn test_transaction_result_print_logs() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    // Should not panic when printing logs
    result.print_logs();
}

#[test]
fn test_send_transaction_result() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );

    let result = svm.send_transaction_result(tx).unwrap();
    result.assert_success();
}

#[test]
fn test_print_logs_structured() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    println!("\n\n--- STRUCTURED LOG OUTPUT ---\n");
    result.print_logs_structured();
    println!("\n--- END STRUCTURED LOG OUTPUT ---\n");
}

#[test]
fn test_send_ok_returns_result_for_chaining() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    // send_ok returns the wrapped TransactionResult so callers can inspect logs / cu.
    let result = svm.send_ok(ix, &[&payer], &crate::Aliases::default());
    assert!(result.is_success());
    assert!(result.compute_units() > 0);
}

#[test]
#[should_panic(expected = "Transaction failed")]
fn test_send_ok_panics_on_program_error() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new(); // unfunded; transfer will fail

    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    svm.send_ok(ix, &[&payer], &crate::Aliases::default());
}

#[test]
#[should_panic(expected = "send_ok: transaction build failed")]
fn test_send_ok_panics_on_build_error() {
    let mut svm = LiteSVM::new();
    let ix = system_instruction::transfer(
        &Keypair::new().pubkey(),
        &Keypair::new().pubkey(),
        1_000_000,
    );
    // Empty signer slice trips the build-time guard in send_instruction.
    svm.send_ok(ix, &[], &crate::Aliases::default());
}

#[test]
fn test_send_err_named_passes_on_matching_error() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new(); // unfunded

    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    // "AccountNotFound" appears in the system-program error path.
    let _result = svm.send_err_named(ix, &[&payer], &crate::Aliases::default(), "AccountNotFound");
}

#[test]
fn test_assert_success_with_passes_when_predicate_holds() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let _result = svm
        .send_instruction(ix, &[&payer])
        .unwrap()
        .assert_success_with(|r| r.compute_units() > 0);
}

#[test]
#[should_panic(expected = "Predicate failed on successful transaction")]
fn test_assert_success_with_panics_when_predicate_fails() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    svm.send_instruction(ix, &[&payer])
        .unwrap()
        .assert_success_with(|_| false);
}

#[test]
fn test_assert_failure_with_passes_when_predicate_holds() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();

    // Unfunded payer fails at validation, before execution: compute_units == 0.
    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    let _result = svm
        .send_instruction(ix, &[&payer])
        .unwrap()
        .assert_failure_with(|r| r.compute_units() == 0);
}

#[test]
#[should_panic(expected = "Predicate failed on failed transaction")]
fn test_assert_failure_with_panics_when_predicate_fails() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    svm.send_instruction(ix, &[&payer])
        .unwrap()
        .assert_failure_with(|_| false);
}

#[test]
#[should_panic(expected = "Expected error containing 'SomeErrorThatNeverAppears'")]
fn test_send_err_named_panics_on_wrong_error_name() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    svm.send_err_named(
        ix,
        &[&payer],
        &crate::Aliases::default(),
        "SomeErrorThatNeverAppears",
    );
}

#[test]
#[should_panic(expected = "Expected transaction to fail")]
fn test_send_err_named_panics_on_success() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();

    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    svm.send_err_named(ix, &[&payer], &crate::Aliases::default(), "AnyError");
}

/// On failure, send_ok prints the structured CPI tree to stderr before
/// the assert_success panic, so test output shows which program frame
/// the error came from (the underlying panic only embeds flat logs).
#[test]
fn test_send_ok_prints_structured_tree_on_failure() {
    use std::panic;
    let mut svm = LiteSVM::new();
    let payer = Keypair::new(); // unfunded; will fail

    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    let panic_result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        svm.send_ok(ix, &[&payer], &crate::Aliases::default());
    }));
    assert!(panic_result.is_err(), "send_ok must panic on failed tx");
    // The structured tree itself is emitted via eprintln! before the panic,
    // so it's not captured by catch_unwind. Visual inspection via
    // `cargo test -- --nocapture` confirms the tree renders. The panic
    // payload itself comes from assert_success and contains the flat log
    // dump as documented.
    let payload = panic_result.unwrap_err();
    let msg = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default();
    assert!(
        msg.contains("Transaction failed"),
        "panic should be from assert_success: got `{}`",
        msg
    );
}

/// Same shape for send_err_named: when the assertion is about to fail
/// (either because the tx succeeded or because the error name didn't
/// match), the structured tree is printed before assert_error panics.
#[test]
fn test_send_err_named_prints_structured_tree_on_mismatch() {
    use std::panic;
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &Keypair::new().pubkey(), 1_000_000);
    let panic_result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        svm.send_err_named(
            ix,
            &[&payer],
            &crate::Aliases::default(),
            "SomeErrorThatNeverAppears",
        );
    }));
    assert!(panic_result.is_err());
    let payload = panic_result.unwrap_err();
    let msg = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default();
    assert!(
        msg.contains("Expected error containing 'SomeErrorThatNeverAppears'"),
        "got `{}`",
        msg
    );
}

#[test]
fn send_instruction_threads_message_into_transaction_result() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(1_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();
    assert_eq!(result.message.header.num_required_signatures, 1);
    assert_eq!(result.message.account_keys[0], payer.pubkey());
}

#[test]
fn print_logs_structured_produces_annotated_output_for_real_transaction() {
    use crate::Aliases;
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(1_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    let aliases = Aliases::default().with(payer.pubkey(), "payer");
    let out = result.with_aliases(aliases).logs_structured_string();

    // Header line carries the resolved program AND the decoded ix name
    // (System uses a u32 LE tag; `2` is Transfer). With the fill-rule
    // header format the title is followed by `─` to HEADER_WIDTH.
    assert!(
        out.lines().any(|l| l.starts_with("── System::Transfer ─")),
        "expected resolved header with decoded ix name; got:\n{out}"
    );
    assert!(
        out.contains("signers=[payer]"),
        "expected payer in header; got:\n{out}"
    );
    assert!(
        out.contains("signer=payer"),
        "expected payer on root frame; got:\n{out}"
    );
    // The root frame decodes its instruction name from the data discriminator
    // (same resolution the CPI children and the header use), so it reads
    // `System::Transfer`, not a bare `System`.
    assert!(
        out.contains("└── System::Transfer [1]"),
        "expected System alias + decoded ix name on the root frame; got:\n{out}"
    );
}

/// The legend footer lists each user-supplied alias that was actually
/// resolved during rendering, with its full base58 pubkey on the right.
/// Well-known programs (System, Token, etc.) are filtered out so the
/// legend stays focused on test-specific actors.
#[test]
fn logs_structured_string_legend_lists_user_alias_and_omits_well_known() {
    use crate::Aliases;
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(1_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    let aliases = Aliases::default().with(payer.pubkey(), "payer");
    let out = result.with_aliases(aliases).logs_structured_string();

    assert!(
        out.contains("Legend (1):\n"),
        "expected single-entry legend; got:\n{out}"
    );
    assert!(
        out.contains(&format!("  payer = {}\n", payer.pubkey())),
        "expected payer entry with full pubkey; got:\n{out}"
    );
    // The transaction touched System; System would appear in the legend
    // without the well-known filter. Assert it's filtered.
    assert!(
        !out.contains("System ="),
        "well-known programs should not appear in legend; got:\n{out}"
    );
}

/// A user who renames a well-known program via `.with(system_pk, "X")`
/// gets X surfaced in the legend: the filter is name-based, so a renamed
/// entry no longer matches the well-known set.
#[test]
fn logs_structured_string_renamed_well_known_appears_in_legend() {
    use crate::Aliases;
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(1_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    let system_pk =
        solana_program::pubkey::Pubkey::from_str_const("11111111111111111111111111111111");
    let aliases = Aliases::default().with(system_pk, "MyNamedSystem");
    let out = result.with_aliases(aliases).logs_structured_string();

    assert!(
        out.contains("MyNamedSystem"),
        "expected the renamed System alias to render in the tree; got:\n{out}"
    );
    assert!(
        out.contains(&format!("  MyNamedSystem = {}\n", system_pk)),
        "expected renamed well-known to appear in legend; got:\n{out}"
    );
}

/// For programs without a decoder table entry (user programs, Anchor
/// programs without an IDL registry), the header falls back to just the
/// program name and never produces the `Program::SomethingWrong` form.
/// This case is exercised by sending a transfer through System with
/// `System` renamed (so the decode_instruction table miss path is what
/// drives the fallback, not the program rename); for a true "unknown
/// program" we'd need to deploy a fake program, which is more setup than
/// the assertion warrants. The decode path keys off the base58 program
/// ID, not the alias, so an unrecognized ID hits the same `None` branch.
#[test]
fn logs_structured_string_header_falls_back_when_decoder_absent() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(1_000_000_000).unwrap();
    let recipient = Keypair::new();

    // Build a transfer whose first 4 bytes (the u32 LE tag) decode as a
    // tag that isn't in `system_instruction_name`'s table. System's
    // recognized tags are 0..=12; we use 0xFF_FF_FF_FF (max u32) as a
    // tag the table doesn't know.
    let mut data = vec![0xFFu8; 4];
    // Append a transfer's lamports payload (u64 LE) so the message is
    // valid enough to send; the runtime will reject this but we just
    // want a populated TransactionResult to render.
    data.extend_from_slice(&1_000_000u64.to_le_bytes());
    let ix = solana_program::instruction::Instruction {
        program_id: solana_program::pubkey::Pubkey::from_str_const(
            "11111111111111111111111111111111",
        ),
        accounts: vec![
            solana_program::instruction::AccountMeta::new(payer.pubkey(), true),
            solana_program::instruction::AccountMeta::new(recipient.pubkey(), false),
        ],
        data,
    };
    let result = svm.send_instruction(ix, &[&payer]).unwrap();
    let out = result.logs_structured_string();

    // Header has just the program name, no `::Name` suffix. With the
    // fill-rule header format the line is `── System ────…`; assert on
    // the title-up-to-first-fill-dash shape.
    assert!(
        out.lines()
            .any(|l| l.starts_with("── System ─") && !l.starts_with("── System::")),
        "expected bare-program fallback header; got:\n{out}"
    );
    assert!(
        !out.contains("── System::"),
        "expected no decoded ix name for unrecognized tag; got:\n{out}"
    );
}

/// A transaction whose only resolved aliases are well-known programs
/// produces no legend footer at all (the section is omitted, not just
/// rendered with "Legend (0):"). Keeps the common case quiet.
#[test]
fn logs_structured_string_omits_legend_when_no_user_aliases_used() {
    use crate::Aliases;
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(1_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    let aliases = Aliases::default(); // no user aliases
    let out = result.with_aliases(aliases).logs_structured_string();

    assert!(
        !out.contains("Legend"),
        "expected no legend section when only well-known aliases resolved; got:\n{out}"
    );
}

#[test]
fn test_tap_executes_closure_and_returns_self() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    let mut side_effect_ran = false;

    // tap should execute the closure and allow chaining
    result
        .tap(|r| {
            assert!(r.is_success());
            side_effect_ran = true;
        })
        .assert_success();

    assert!(side_effect_ran, "tap closure should have executed");
}

/// `with_aliases` attaches the table to the result, and a subsequent
/// `logs_structured_string()` reads it without a per-call parameter.
/// The legend entry confirms the alias actually flowed through.
#[test]
fn with_aliases_flows_into_no_arg_logs_structured_string() {
    use crate::Aliases;
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(1_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    let aliases = Aliases::default().with(payer.pubkey(), "payer-via-storage");
    let out = result.with_aliases(aliases).logs_structured_string();

    assert!(
        out.contains("signer=payer-via-storage"),
        "stashed alias should resolve on the root frame; got:\n{out}"
    );
    assert!(
        out.contains("  payer-via-storage = "),
        "stashed alias should appear in legend; got:\n{out}"
    );
}

/// With no `with_aliases` call, `logs_structured_string()` falls back
/// to `Aliases::default()` (well-known programs only). System should
/// still resolve; no user alias should appear in the legend.
#[test]
fn logs_structured_string_without_attached_aliases_uses_default() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(1_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    let out = result.logs_structured_string();
    assert!(
        out.contains("└── System::Transfer [1]"),
        "well-known System alias + decoded ix name should resolve via default; got:\n{out}"
    );
    assert!(
        !out.contains("Legend"),
        "no user aliases should mean no legend; got:\n{out}"
    );
}

/// `send_ok` stashes the alias table on the returned result, so a
/// downstream chained `print_logs_structured()` (no arg) sees it.
#[test]
fn send_ok_stashes_aliases_for_chained_no_arg_print() {
    use crate::Aliases;
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(1_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let aliases = Aliases::default().with(payer.pubkey(), "from-send-ok");
    let result = svm.send_ok(ix, &[&payer], &aliases);

    let out = result.logs_structured_string();
    assert!(
        out.contains("signer=from-send-ok"),
        "send_ok-stashed alias should be readable post-send; got:\n{out}"
    );
}

#[test]
fn test_tap_enables_method_chaining() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);

    // Demonstrate fluent chaining with tap
    let cu = svm
        .send_instruction(ix, &[&payer])
        .unwrap()
        .tap(|r| {
            // Custom inspection/logging
            let _ = r.compute_units();
        })
        .assert_success()
        .compute_units();

    assert!(cu > 0);
}

#[test]
fn identical_sends_are_fresh_by_default() {
    // The repeated-send pattern (rate limits, spend caps): an identical
    // instruction resent in a loop. send_instruction refreshes the blockhash
    // before signing, so no send collides with its predecessor's signature
    // and nobody performs the expire_blockhash ritual. (cds-rs issue #4)
    use solana_signer::Signer;
    let mut svm = litesvm::LiteSVM::new();
    let payer = solana_keypair::Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    let dest = solana_program::pubkey::Pubkey::new_unique();
    for i in 0..3 {
        let ix = solana_system_interface::instruction::transfer(&payer.pubkey(), &dest, 2_000_000);
        let result = svm.send_instruction(ix, &[&payer]).expect("build ok");
        assert!(result.error().is_none(), "send #{i} should be fresh");
    }
}
