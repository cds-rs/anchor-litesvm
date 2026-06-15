use super::*;

#[test]
fn resolve_accounts_marks_signer_and_writable_roles() {
    use {
        solana_message::{Message, MessageHeader},
        solana_program::pubkey::Pubkey,
    };

    // 4 keys: [writable-signer, readonly-signer, writable-nonsigner,
    // readonly-nonsigner]. Header: 2 signers, 1 readonly-signed,
    // 1 readonly-unsigned.
    let keys: Vec<Pubkey> = (0..4).map(|_| Pubkey::new_unique()).collect();
    let message = Message {
        header: MessageHeader {
            num_required_signatures: 2,
            num_readonly_signed_accounts: 1,
            num_readonly_unsigned_accounts: 1,
        },
        account_keys: keys.clone(),
        ..Default::default()
    };

    let got = resolve_accounts(&[0, 1, 2, 3], &message);
    let roles: Vec<(bool, bool)> = got.iter().map(|a| (a.is_signer, a.is_writable)).collect();
    assert_eq!(
        roles,
        vec![(true, true), (true, false), (false, true), (false, false)],
        "signer/writable roles per legacy-message header rule"
    );
    // Pubkeys round-trip in order.
    assert_eq!(
        got.iter().map(|a| a.pubkey).collect::<Vec<_>>(),
        keys,
        "resolved pubkeys should match account_keys in index order"
    );

    // Out-of-range indices are skipped, not panicked on.
    assert_eq!(resolve_accounts(&[99], &message).len(), 0);
}

#[test]
fn spl_token_instruction_name_decodes_known_discriminators() {
    assert_eq!(spl_token_instruction_name(&[7, 1, 2, 3]), Some("MintTo"));
    assert_eq!(
        spl_token_instruction_name(&[12, 0, 0]),
        Some("TransferChecked")
    );
    assert_eq!(spl_token_instruction_name(&[8]), Some("Burn"));
    assert_eq!(spl_token_instruction_name(&[]), None);
    assert_eq!(spl_token_instruction_name(&[99]), None);
}

#[test]
fn system_instruction_name_decodes_u32_le_tag() {
    assert_eq!(
        system_instruction_name(&[0, 0, 0, 0, /*rest*/ 1, 2, 3]),
        Some("CreateAccount")
    );
    assert_eq!(system_instruction_name(&[8, 0, 0, 0]), Some("Allocate"));
    assert_eq!(system_instruction_name(&[1, 2, 3]), None);
}

#[test]
fn spl_ata_instruction_name_handles_empty_data() {
    assert_eq!(spl_ata_instruction_name(&[]), Some("Create"));
    assert_eq!(spl_ata_instruction_name(&[0]), Some("Create"));
    assert_eq!(spl_ata_instruction_name(&[1]), Some("CreateIdempotent"));
}

#[test]
fn extract_anchor_error_name_finds_thrown_form() {
    let logs = vec![
        FrameLog::Msg("Some unrelated msg".to_string()),
        FrameLog::Msg(
            "AnchorError thrown in programs/escrow/src/instructions/take.rs:42. Error Code: EscrowExpired. Error Number: 6000. Error Message: EscrowExpired."
                .to_string(),
        ),
    ];
    assert_eq!(
        extract_anchor_error_name(&logs).as_deref(),
        Some("EscrowExpired")
    );
}

#[test]
fn extract_anchor_error_name_finds_caused_by_account_form() {
    // Anchor's constraint-failure variant uses a different prefix
    // ("AnchorError caused by account: ..."), still carries the
    // `Error Code: <Name>.` segment.
    let logs = vec![FrameLog::Msg(
        "AnchorError caused by account: vault. Error Code: ConstraintSeeds. Error Number: 2006. Error Message: A seeds constraint was violated."
            .to_string(),
    )];
    assert_eq!(
        extract_anchor_error_name(&logs).as_deref(),
        Some("ConstraintSeeds")
    );
}

#[test]
fn extract_anchor_error_name_returns_none_for_non_anchor_failures() {
    // Failures from native programs / raw msg!() users have no
    // AnchorError line.
    let logs = vec![
        FrameLog::Msg("Some user-level diagnostic".to_string()),
        FrameLog::Msg("Program System failed: insufficient funds".to_string()),
    ];
    assert_eq!(extract_anchor_error_name(&logs), None);
}

#[test]
fn extract_anchor_error_name_ignores_data_entries() {
    // Anchor events arrive as FrameLog::Data; the extractor only
    // scans Msg entries, since AnchorError is always a Msg.
    let logs = vec![FrameLog::Data(
        "AnchorError thrown in foo.rs:1. Error Code: Spoofed. Error Number: 6000.".to_string(),
    )];
    assert_eq!(extract_anchor_error_name(&logs), None);
}

#[test]
fn extract_anchor_error_account_finds_the_offending_field() {
    // Constraint failures name the account they blame; lift the field name
    // (e.g. a transfer hook that declared `authority: Signer` and got a
    // non-signer from the runtime).
    let logs = vec![FrameLog::Msg(
        "AnchorError caused by account: authority. Error Code: AccountNotSigner. Error Number: 3010. Error Message: The given account did not sign."
            .to_string(),
    )];
    assert_eq!(
        extract_anchor_error_account(&logs).as_deref(),
        Some("authority")
    );
}

#[test]
fn extract_anchor_error_account_is_none_when_no_account_named() {
    // The `thrown in <file>` form (a `require!` failure) names no account.
    let logs = vec![FrameLog::Msg(
        "AnchorError thrown in programs/escrow/src/take.rs:42. Error Code: EscrowExpired. Error Number: 6000. Error Message: EscrowExpired."
            .to_string(),
    )];
    assert_eq!(extract_anchor_error_account(&logs), None);
}

#[test]
fn resolve_anchor_failure_appends_the_offending_account() {
    // Only on a failed frame, and only when an account is named, does the
    // label gain the offending account: this is the signal the transfer-hook
    // Signer bug needed.
    let logs = vec![FrameLog::Msg(
        "AnchorError caused by account: authority. Error Code: AccountNotSigner. Error Number: 3010. Error Message: The given account did not sign."
            .to_string(),
    )];
    assert_eq!(
        resolve_anchor_failure(&logs).as_deref(),
        Some("AccountNotSigner on authority")
    );
}

#[test]
fn resolve_anchor_failure_is_just_the_name_when_no_account() {
    let logs = vec![FrameLog::Msg(
        "AnchorError thrown in programs/escrow/src/take.rs:42. Error Code: EscrowExpired. Error Number: 6000. Error Message: EscrowExpired."
            .to_string(),
    )];
    assert_eq!(
        resolve_anchor_failure(&logs).as_deref(),
        Some("EscrowExpired")
    );
}

#[test]
fn extract_anchor_error_name_returns_first_when_multiple() {
    // Nested failures (a child fails AND the parent fails because of
    // it) can produce two AnchorError lines in one frame's logs
    // (rare but possible). First-seen wins; matches the typical
    // "leaf error is the one to report" convention.
    let logs = vec![
        FrameLog::Msg(
            "AnchorError thrown in inner.rs:1. Error Code: FirstError. Error Number: 6000."
                .to_string(),
        ),
        FrameLog::Msg(
            "AnchorError thrown in outer.rs:1. Error Code: SecondError. Error Number: 6001."
                .to_string(),
        ),
    ];
    assert_eq!(
        extract_anchor_error_name(&logs).as_deref(),
        Some("FirstError")
    );
}
