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
    let empty_events = crate::transaction::EventRegistry::new();
    let mut collector = crate::transaction::renderer::LegendCollector::new(aliases, &empty_events);
    let roots = crate::transaction::model::resolve_roots(
        logs,
        inner_instructions,
        &solana_message::Message::default(),
        signers,
        crate::transaction::model::Vocab {
            instructions: &crate::transaction::InstructionNames::new(),
            errors: &crate::transaction::ErrorNames::new(),
            events: &crate::transaction::EventRegistry::new(),
        },
    );
    fmt_tree(
        &roots,
        &signers.tx_signers,
        &mut collector,
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
fn a_registered_event_renders_as_a_tree_line_with_aliased_fields() {
    use base64::{engine::general_purpose, Engine as _};
    use std::sync::Arc;

    let maker = Pubkey::new_unique();
    let escrow = Pubkey::new_unique();

    // A decoder whose fields embed the maker's base58 key, to prove the tree
    // substitutes it for the alias just as the mermaid note does.
    let mut reg = crate::transaction::EventRegistry::new();
    let maker_b58 = maker.to_string();
    reg.register(
        [7u8; 8],
        "Transfer",
        Arc::new(move |_b: &[u8]| {
            Some(vec![
                ("from".to_string(), maker_b58.clone()),
                ("amount".to_string(), "100".to_string()),
            ])
        }),
    );
    let mut raw = [7u8; 8].to_vec();
    raw.extend_from_slice(&100u64.to_le_bytes());
    let payload = general_purpose::STANDARD.encode(&raw);

    let frame = crate::transaction::model::ResolvedFrame {
        program: escrow,
        instruction_name: Some("Make".to_string()),
        outcome: crate::transaction::model::Outcome::Success,
        compute_units: Some(5000),
        accounts: vec![],
        logs: vec![crate::transaction::model::FrameLog::Data(payload)],
        data: vec![],
        children: vec![],
    };
    let model = crate::transaction::model::CpiModel {
        header: None,
        roots: vec![crate::transaction::model::Root {
            signers: vec![],
            frame,
        }],
        tx_signers: vec![],
        error: None,
        compute_units: 5000,
        fee: 0,
        events: reg,
    };
    let aliases = crate::Aliases::with_well_known()
        .with(maker, "maker")
        .with(escrow, "escrow");

    let out = TreeRenderer {
        style: crate::transaction::style::Style::Off,
    }
    .render(&model, &aliases);

    assert!(
        out.contains("🔔 Transfer"),
        "expected a decoded event header in the tree; got:\n{out}"
    );
    // The `from` field, on its own aligned line, shows the alias not base58.
    assert!(
        out.lines()
            .any(|l| l.contains("from:") && l.contains("maker")),
        "alias not substituted on the from line; got:\n{out}"
    );
    assert!(
        !out.contains(&maker.to_string()),
        "raw base58 leaked into the tree; got:\n{out}"
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
