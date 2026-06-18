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

#[test]
fn fill_from_trace_names_inner_native_frames_and_never_shadows() {
    use {
        crate::transaction::{
            ErrorNames, EventRegistry, InstructionNames, InstructionTrace, TracedInstruction,
        },
        solana_program::pubkey::Pubkey,
    };

    // The engine-neutral backend path leaves `inner_instructions` empty, so the
    // inner System frame reaches `fill_from_trace` with no name. The trace is
    // the only carrier of its data; the System program id is the all-zero key.
    let program = Pubkey::new_unique();
    let system = Pubkey::default();

    let mut model = CpiModel {
        header: None,
        roots: vec![Root {
            signers: vec![],
            frame: ResolvedFrame {
                program,
                // A name the build path already resolved (a log-derived or
                // discriminator-decoded one). The trace pass must not shadow it.
                instruction_name: Some("Withdraw".into()),
                outcome: Outcome::Success,
                compute_units: None,
                accounts: vec![],
                logs: vec![],
                data: vec![],
                children: vec![ResolvedFrame {
                    program: system,
                    // The unnamed inner frame this fix targets.
                    instruction_name: None,
                    outcome: Outcome::Success,
                    compute_units: None,
                    accounts: vec![],
                    logs: vec![],
                    data: vec![],
                    children: vec![],
                }],
            },
        }],
        tx_signers: vec![],
        error: None,
        compute_units: 0,
        fee: 0,
        events: Default::default(),
    };

    // Flat DFS trace: the program root, then its System `CreateAccount` CPI
    // (4-byte u32 LE tag 0).
    let trace = InstructionTrace(vec![
        TracedInstruction {
            program_id: program,
            stack_height: 1,
            accounts: vec![],
            data: vec![9, 9, 9, 9],
        },
        TracedInstruction {
            program_id: system,
            stack_height: 2,
            accounts: vec![],
            data: vec![0, 0, 0, 0],
        },
    ]);

    let instructions = InstructionNames::default();
    let errors = ErrorNames::default();
    let events = EventRegistry::default();
    let vocab = Vocab {
        instructions: &instructions,
        errors: &errors,
        events: &events,
    };

    fill_from_trace(&mut model, &trace, vocab);

    let root = &model.roots[0].frame;
    // The build-path name survives: the trace pass only fills an open name.
    assert_eq!(
        root.instruction_name.as_deref(),
        Some("Withdraw"),
        "an already-resolved frame name must not be shadowed by the trace pass"
    );
    // The inner System frame, formerly `unnamed`, now decodes from its data.
    assert_eq!(
        root.children[0].instruction_name.as_deref(),
        Some("CreateAccount"),
        "the inner native-program frame should resolve its name from the traced data"
    );
}

#[test]
fn from_transaction_sources_inner_accounts_and_names_from_the_trace() {
    use {
        solana_message::{compiled_instruction::CompiledInstruction, Message, MessageHeader},
        solana_program::pubkey::Pubkey,
        testsvm::{
            frame::{Frame, Outcome as FrameOutcome},
            trace::{InstructionTrace, TracedAccount, TracedInstruction},
        },
    };

    let program = Pubkey::new_unique();
    let system = Pubkey::default();
    let payer = Pubkey::new_unique();
    let new_acct = Pubkey::new_unique();

    // One top-level instruction to `program`, accounts [payer (signer), new_acct].
    let message = Message {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 1,
        },
        account_keys: vec![payer, new_acct, program],
        instructions: vec![CompiledInstruction {
            program_id_index: 2,
            accounts: vec![0, 1],
            data: vec![9, 9, 9, 9],
        }],
        ..Default::default()
    };

    // Engine-neutral frames: a named root with one unnamed System child.
    // Neither carries accounts; the trace is their only source (this is the
    // shape the `From<model::Transaction>` bridge produces, with empty
    // `inner_instructions`).
    let frames = vec![Frame {
        program_id: program,
        outcome: FrameOutcome::Success,
        compute_units: None,
        instruction_name: Some("Withdraw".into()),
        logs: vec![],
        children: vec![Frame {
            program_id: system,
            outcome: FrameOutcome::Success,
            compute_units: None,
            instruction_name: None,
            logs: vec![],
            children: vec![],
        }],
    }];

    // Flat DFS trace carrying per-frame accounts (with owners) and the inner
    // System `CreateAccount` data (4-byte u32 LE tag 0). new_acct ends up
    // owned by `program` (the inner frame's owner differs from the root's).
    let trace = InstructionTrace(vec![
        TracedInstruction {
            program_id: program,
            stack_height: 1,
            data: vec![9, 9, 9, 9],
            accounts: vec![
                TracedAccount {
                    pubkey: payer,
                    is_signer: true,
                    is_writable: true,
                    owner: system,
                },
                TracedAccount {
                    pubkey: new_acct,
                    is_signer: false,
                    is_writable: true,
                    owner: system,
                },
            ],
        },
        TracedInstruction {
            program_id: system,
            stack_height: 2,
            data: vec![0, 0, 0, 0],
            accounts: vec![
                TracedAccount {
                    pubkey: payer,
                    is_signer: true,
                    is_writable: true,
                    owner: system,
                },
                TracedAccount {
                    pubkey: new_acct,
                    is_signer: false,
                    is_writable: true,
                    owner: program,
                },
            ],
        },
    ]);

    let tx = testsvm::model::Transaction::assemble(
        frames,
        message,
        vec![],
        None,
        0,
        None,
        Some(trace),
        None,
        &Default::default(),
        &Default::default(),
        &testsvm::model::AnchorFailures,
        testsvm::aliases::Aliases::default(),
        Default::default(),
    );

    let model = from_transaction(&tx);

    let root = &model.roots[0].frame;
    assert_eq!(root.program, program);
    assert_eq!(root.instruction_name.as_deref(), Some("Withdraw"));
    assert_eq!(
        root.accounts.iter().map(|a| a.pubkey).collect::<Vec<_>>(),
        vec![payer, new_acct],
        "root accounts come from the trace",
    );
    assert!(root.accounts[0].is_signer && root.accounts[0].is_writable);

    // The whole point: the inner frame's accounts come from the trace (the
    // build path leaves them empty without `inner_instructions`), with the
    // trace's owners, and the name decodes from the traced data.
    let child = &root.children[0];
    assert_eq!(child.program, system);
    assert_eq!(
        child.instruction_name.as_deref(),
        Some("CreateAccount"),
        "the inner frame's name decodes from the trace's data",
    );
    assert_eq!(
        child.accounts.iter().map(|a| a.pubkey).collect::<Vec<_>>(),
        vec![payer, new_acct],
        "inner-frame accounts come from the trace, not empty inner_instructions",
    );
    assert_eq!(
        child.accounts[1].owner,
        Some(program),
        "the inner frame's owner comes from the trace",
    );
}
