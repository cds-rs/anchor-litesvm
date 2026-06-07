use {super::*, proptest::prelude::*};

fn default_aliases() -> crate::Aliases {
    crate::Aliases::with_well_known()
}

fn empty_signers() -> crate::transaction::signers::SignerInfo {
    crate::transaction::signers::SignerInfo {
        tx_signers: vec![],
        per_root: vec![],
    }
}

fn render_with(
    logs: &[String],
    inner_instructions: &solana_message::inner_instruction::InnerInstructionsList,
    aliases: &crate::Aliases,
    signers: &crate::transaction::signers::SignerInfo,
) -> String {
    let mut collector = LegendCollector::new(aliases);
    render(
        logs,
        inner_instructions,
        &mut collector,
        signers,
        crate::transaction::style::Style::Off,
    )
}

#[test]
fn render_substitutes_program_ids() {
    let logs = vec![
        "Program CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5 invoke [1]".to_string(),
        "Program 11111111111111111111111111111111 invoke [2]".to_string(),
        "Program 11111111111111111111111111111111 success".to_string(),
        "Program CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5 success".to_string(),
    ];
    let out = render_with(&logs, &Vec::new(), &default_aliases(), &empty_signers());
    assert!(
        out.contains("System"),
        "expected System substitution; got:\n{out}"
    );
    assert!(
        out.contains("CYbYnHW7"),
        "expected unknown program ID to pass through; got:\n{out}"
    );
    assert!(
        !out.contains("Program 11111111111111111111111111111111"),
        "raw System pubkey leaked through; got:\n{out}"
    );
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
fn render_failure_prefers_anchor_name_over_runtime_message() {
    // End-to-end through the parser + renderer: a log stream that
    // contains both the AnchorError line and the runtime's
    // "custom program error: 0x1770" should render with the friendly
    // name as the `Error: ...` line, not the raw runtime message.
    use {crate::transaction::signers::SignerInfo, std::str::FromStr};
    let escrow_id = "H1GjRKWSauAuupurDtGiY5uvhLBtUngNhvrSBs75rH9o";
    let logs = vec![
        format!("Program {escrow_id} invoke [1]"),
        "Program log: Instruction: Take".to_string(),
        "Program log: AnchorError thrown in programs/escrow/src/instructions/take.rs:42. Error Code: EscrowExpired. Error Number: 6000. Error Message: EscrowExpired."
            .to_string(),
        format!("Program {escrow_id} consumed 5000 of 200000 compute units"),
        format!("Program {escrow_id} failed: custom program error: 0x1770"),
    ];
    let taker = Pubkey::new_unique();
    let aliases = crate::Aliases::with_well_known()
        .with(taker, "Taker")
        .with(Pubkey::from_str(escrow_id).unwrap(), "escrow");
    let signers = SignerInfo {
        tx_signers: vec![taker],
        per_root: vec![vec![taker]],
    };
    let out = render_with(&logs, &Vec::new(), &aliases, &signers);
    assert!(
        out.contains("Error: EscrowExpired"),
        "expected friendly name; got:\n{out}"
    );
    assert!(
        !out.contains("custom program error: 0x1770"),
        "raw runtime message should be suppressed when name available; got:\n{out}"
    );
}

#[test]
fn render_annotates_inner_instructions_via_decoder() {
    use {
        solana_message::compiled_instruction::CompiledInstruction,
        solana_message::inner_instruction::InnerInstruction,
    };

    let logs = vec![
        "Program CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5 invoke [1]".to_string(),
        "Program 11111111111111111111111111111111 invoke [2]".to_string(),
        "Program 11111111111111111111111111111111 success".to_string(),
        "Program CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5 success".to_string(),
    ];

    // System::Transfer: 4-byte little-endian tag 2.
    let inner = vec![vec![InnerInstruction {
        instruction: CompiledInstruction {
            program_id_index: 0,
            accounts: vec![],
            data: vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        },
        stack_height: 2,
    }]];

    let out = render_with(&logs, &inner, &default_aliases(), &empty_signers());
    assert!(
        out.contains("System::Transfer"),
        "expected System::Transfer annotation; got:\n{out}"
    );
}

#[test]
fn render_emits_signer_annotation_on_top_level_frame() {
    use {crate::transaction::signers::SignerInfo, std::str::FromStr};
    let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
    let logs = vec![
        format!("Program {amm_id} invoke [1]"),
        format!("Program {amm_id} consumed 4079 of 200000 compute units"),
        format!("Program {amm_id} success"),
    ];
    let admin = Pubkey::new_unique();
    let aliases = crate::Aliases::with_well_known()
        .with(admin, "admin")
        .with(Pubkey::from_str(amm_id).unwrap(), "amm");
    let signers = SignerInfo {
        tx_signers: vec![admin],
        per_root: vec![vec![admin]],
    };
    let out = render_with(&logs, &Vec::new(), &aliases, &signers);
    let expected = "\
Transaction  signers=[admin]
└── amm [1] ✓ 4079cu  signer=admin
";
    assert_eq!(out, expected);
}

#[test]
fn render_emits_per_root_signer_for_multi_signer_multi_ix() {
    use {crate::transaction::signers::SignerInfo, std::str::FromStr};
    let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
    let logs = vec![
        format!("Program {amm_id} invoke [1]"),
        format!("Program {amm_id} success"),
        format!("Program {amm_id} invoke [1]"),
        format!("Program {amm_id} success"),
    ];
    let alice = Pubkey::new_unique();
    let bob = Pubkey::new_unique();
    let aliases = crate::Aliases::with_well_known()
        .with(alice, "alice")
        .with(bob, "bob")
        .with(Pubkey::from_str(amm_id).unwrap(), "amm");
    let signers = SignerInfo {
        tx_signers: vec![alice, bob],
        per_root: vec![vec![alice], vec![bob]],
    };
    let out = render_with(&logs, &Vec::new(), &aliases, &signers);
    let expected = "\
Transaction  signers=[alice, bob]
├── amm [1] ✓ (no cu)  signer=alice
└── amm [1] ✓ (no cu)  signer=bob
";
    assert_eq!(out, expected);
}

#[test]
fn render_omits_signer_annotation_on_cpi_frames() {
    use {crate::transaction::signers::SignerInfo, std::str::FromStr};
    let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
    let token_id = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    let logs = vec![
        format!("Program {amm_id} invoke [1]"),
        format!("Program {token_id} invoke [2]"),
        format!("Program {token_id} success"),
        format!("Program {amm_id} success"),
    ];
    let admin = Pubkey::new_unique();
    let aliases = crate::Aliases::with_well_known()
        .with(admin, "admin")
        .with(Pubkey::from_str(amm_id).unwrap(), "amm");
    let signers = SignerInfo {
        tx_signers: vec![admin],
        per_root: vec![vec![admin]],
    };
    let out = render_with(&logs, &Vec::new(), &aliases, &signers);
    assert!(
        out.contains("└── amm [1] ✓ (no cu)  signer=admin\n"),
        "expected amm[1] to have signer=admin; got:\n{out}"
    );
    assert!(
        out.contains("└── Token [2] ✓ (no cu)\n"),
        "expected Token[2] frame without signer= annotation; got:\n{out}"
    );
    assert!(
        !out.contains("Token [2] ✓ (no cu)  signer="),
        "Token CPI should not carry signer= annotation; got:\n{out}"
    );
}

#[test]
fn render_fee_payer_signer_appears_on_all_frames_referencing_it() {
    // Documents the honest semantic: signer=X means "X is a tx-required
    // signer whose pubkey is referenced in this ix's accounts", NOT
    // "X authorized this ix". A fee payer that appears in every ix's
    // account list (common) shows up in signer= everywhere.
    use {crate::transaction::signers::SignerInfo, std::str::FromStr};
    let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
    let logs = vec![
        format!("Program {amm_id} invoke [1]"),
        format!("Program {amm_id} success"),
        format!("Program {amm_id} invoke [1]"),
        format!("Program {amm_id} success"),
    ];
    let admin = Pubkey::new_unique();
    let aliases = crate::Aliases::with_well_known()
        .with(admin, "admin")
        .with(Pubkey::from_str(amm_id).unwrap(), "amm");
    let signers = SignerInfo {
        tx_signers: vec![admin],
        per_root: vec![vec![admin], vec![admin]],
    };
    let out = render_with(&logs, &Vec::new(), &aliases, &signers);
    assert!(out.contains("├── amm [1] ✓ (no cu)  signer=admin\n"));
    assert!(out.contains("└── amm [1] ✓ (no cu)  signer=admin\n"));
}

#[test]
fn render_truncates_unaliased_pubkeys_in_rich_path() {
    use crate::transaction::signers::SignerInfo;
    let user_program = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
    let logs = vec![
        format!("Program {user_program} invoke [1]"),
        format!("Program {user_program} success"),
    ];
    let aliases = crate::Aliases::with_well_known();
    let signers = SignerInfo {
        tx_signers: vec![],
        per_root: vec![vec![]],
    };
    let out = render_with(&logs, &Vec::new(), &aliases, &signers);
    assert!(
        out.contains("CYbYnHW7…2yf5"),
        "expected truncated form; got:\n{out}"
    );
    assert!(
        !out.contains(user_program),
        "raw form should not appear when truncating; got:\n{out}"
    );
}

#[test]
fn lock_attack_trace_reads_as_english_with_aliases() {
    // Golden output: three top-level ixs (set_locked, swap, set_locked) all
    // signed by admin. The middle swap has two Token::TransferChecked CPIs.
    // With the alias map populated for admin and amm, the trace should make
    // the "three admin-signed ixs in one tx, one a swap" pattern visible at
    // a glance.
    use {crate::transaction::signers::SignerInfo, std::str::FromStr};

    let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
    let token_id = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

    let logs = vec![
        format!("Program {amm_id} invoke [1]"),
        format!("Program {amm_id} consumed 4081 of 200000 compute units"),
        format!("Program {amm_id} success"),
        format!("Program {amm_id} invoke [1]"),
        format!("Program {token_id} invoke [2]"),
        format!("Program {token_id} consumed 105 of 200000 compute units"),
        format!("Program {token_id} success"),
        format!("Program {token_id} invoke [2]"),
        format!("Program {token_id} consumed 105 of 200000 compute units"),
        format!("Program {token_id} success"),
        format!("Program {amm_id} consumed 23615 of 200000 compute units"),
        format!("Program {amm_id} success"),
        format!("Program {amm_id} invoke [1]"),
        format!("Program {amm_id} consumed 4079 of 200000 compute units"),
        format!("Program {amm_id} success"),
    ];

    let admin = Pubkey::new_unique();
    let aliases = crate::Aliases::with_well_known()
        .with(admin, "admin")
        .with(Pubkey::from_str(amm_id).unwrap(), "amm");
    let signers = SignerInfo {
        tx_signers: vec![admin],
        per_root: vec![vec![admin], vec![admin], vec![admin]],
    };

    let out = render_with(&logs, &Vec::new(), &aliases, &signers);
    let expected = "\
Transaction  signers=[admin]
├── amm [1] ✓ 4081cu  signer=admin
├── amm [1] ✓ 23615cu  signer=admin
│   ├── Token [2] ✓ 105cu
│   └── Token [2] ✓ 105cu
└── amm [1] ✓ 4079cu  signer=admin
";
    assert_eq!(out, expected);
}

#[test]
fn failed_frame_with_children_renders_children_first_then_error() {
    // Mirrors the escrow `take_and_close_fails_after_expiry` shape: a
    // top-level frame fails after one or more CPI children completed.
    // Solana logs the children in invocation order before the parent's
    // post-CPI check fires, so chronologically children precede the
    // error. The renderer must:
    //   1. Render children first, error last.
    //   2. Mark each child `├──` (since the error follows) and the
    //      error `└──` (since it's the actual last node).
    // The pre-fix bug was to render the error before children and use
    // `└──` for it, producing two `└──` connectors at the same depth.
    use std::str::FromStr;
    let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
    let logs = vec![
        format!("Program {amm_id} invoke [1]"),
        "Program 11111111111111111111111111111111 invoke [2]".to_string(),
        "Program 11111111111111111111111111111111 success".to_string(),
        format!("Program {amm_id} consumed 1234 of 200000 compute units"),
        format!("Program {amm_id} failed: custom program error: 0x42"),
    ];
    let aliases = default_aliases().with(Pubkey::from_str(amm_id).unwrap(), "amm");
    let out = render_with(&logs, &Vec::new(), &aliases, &empty_signers());

    // Child gets `├──` because the error follows it at the same depth.
    assert!(
        out.contains("├── System"),
        "child must use ├── when an error follows; got:\n{out}"
    );
    // Error gets `└──` as the last node.
    assert!(
        out.contains("└── Error: custom program error: 0x42"),
        "error must use └── as the last node; got:\n{out}"
    );
    // Chronology: child appears before error in the rendered text.
    let child_pos = out.find("System").expect("child line present");
    let error_pos = out.find("Error:").expect("error line present");
    assert!(
        child_pos < error_pos,
        "child must render before error (Solana logs children first); got:\n{out}"
    );
    // And there's only one `└──` per parent (the error) at the child depth.
    let inner_last_markers = out.matches("\n    └──").count();
    assert_eq!(
        inner_last_markers, 1,
        "expected exactly one └── at the child depth (the error); got:\n{out}"
    );
}

#[test]
fn failed_frame_with_no_children_still_uses_end_connector_for_error() {
    // The simpler case: a frame fails without any CPI children. The
    // error is the only child, gets `└──`, and nothing precedes it.
    use std::str::FromStr;
    let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
    let logs = vec![
        format!("Program {amm_id} invoke [1]"),
        format!("Program {amm_id} consumed 100 of 200000 compute units"),
        format!("Program {amm_id} failed: custom program error: 0x7"),
    ];
    let aliases = default_aliases().with(Pubkey::from_str(amm_id).unwrap(), "amm");
    let out = render_with(&logs, &Vec::new(), &aliases, &empty_signers());
    assert!(
        out.contains("└── Error: custom program error: 0x7"),
        "error must be the └── leaf; got:\n{out}"
    );
}

proptest! {
    /// render on arbitrary garbage must never panic and must either produce
    /// empty output or start with "Transaction\n".
    #[test]
    fn render_well_formed(logs in prop::collection::vec(".*", 0..50)) {
        let out = render_with(&logs, &Vec::new(), &default_aliases(), &empty_signers());
        prop_assert!(out.is_empty() || out.starts_with("Transaction\n"));
    }
}
