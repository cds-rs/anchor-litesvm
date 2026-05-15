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
fn test_transaction_result_compute_units() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    // Simple transfer should consume some compute units
    let cu = result.compute_units();
    assert!(cu > 0);
    assert!(cu < 1_000_000); // Should be reasonable
}

#[test]
fn test_transaction_result_logs() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    let logs = result.logs();
    assert!(!logs.is_empty());
}

#[test]
fn test_transaction_result_inner() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    // Should be able to access inner metadata
    let _inner = result.inner();
    assert!(_inner.compute_units_consumed > 0);
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
#[should_panic(expected = "Transaction failed with unexpected error")]
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
fn test_transaction_result_debug() {
    let mut svm = LiteSVM::new();
    let payer = svm.create_funded_account(10_000_000_000).unwrap();
    let recipient = Keypair::new();

    let ix = system_instruction::transfer(&payer.pubkey(), &recipient.pubkey(), 1_000_000);
    let result = svm.send_instruction(ix, &[&payer]).unwrap();

    // Should be able to format as debug
    let debug_str = format!("{:?}", result);
    assert!(debug_str.contains("TransactionResult"));
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

