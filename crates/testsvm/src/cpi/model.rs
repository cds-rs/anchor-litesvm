//! The resolved CPI model the renderers consume, built from the engine-neutral
//! [`Transaction`](crate::model::Transaction).
//!
//! [`from_transaction`] reads the already-parsed `frames` for structure (the
//! names and outcomes the backend resolved in
//! [`assemble`](crate::model::Transaction::assemble)) and the per-frame `trace`
//! for the account lists, their signer/writable/owner roles, and the inner
//! instruction data. The result is a [`CpiModel`] that carries everything a
//! renderer needs: per-frame names, outcomes, accounts with roles, compute, and
//! the tx-level header / signers / cu / fee / error for a renderer's framing.
//!
//! Because the model is built from the neutral record, *every* engine's
//! transaction renders, not just a litesvm result. A renderer reads the model
//! and nothing else; that single source of truth is the point of the split.

use {
    super::trace::TracedInstruction,
    solana_message::{Message, MessageHeader},
    solana_pubkey::Pubkey,
};

/// The per-frame log entries (`Program log:` / `Program data:`), surfaced by
/// the renderers that show events.
pub(super) use crate::frame::FrameLog;

/// The fully-resolved CPI model: the single value every renderer consumes.
#[derive(Clone)]
pub(super) struct CpiModel {
    /// The top-level instruction for the section header, or `None` for
    /// batch sends (which carry no single canonical "the instruction").
    pub header: Option<Header>,
    /// The CPI invocation forest, one entry per top-level instruction.
    pub roots: Vec<Root>,
    /// The tx's required signers in `account_keys` order (fee payer first).
    pub tx_signers: Vec<Pubkey>,
    /// The tx-level error string, if the send failed.
    pub error: Option<String>,
    /// Compute units consumed by this run.
    pub compute_units: u64,
    /// Fee paid, in lamports.
    pub fee: u64,
    /// The event decoders in force for this render, carried on the model so a
    /// renderer can decode a frame's `Program data:` payloads (the registry is
    /// `Clone` via `Arc`d closures, so this is a cheap handle, not a copy of the
    /// decoders).
    pub events: super::EventRegistry,
}

impl CpiModel {
    /// Every frame in the model, DFS pre-order (each root, then its children).
    /// The one traversal the graph and census renderers share, instead of each
    /// re-walking `roots`/`children`: consumers map or filter the flat list as
    /// they need (failure messages, the per-account graphs, the index census).
    pub(super) fn frames(&self) -> Vec<&ResolvedFrame> {
        fn walk<'a>(frame: &'a ResolvedFrame, out: &mut Vec<&'a ResolvedFrame>) {
            out.push(frame);
            for child in &frame.children {
                walk(child, out);
            }
        }
        let mut out = Vec::new();
        for root in &self.roots {
            walk(&root.frame, &mut out);
        }
        out
    }
}

/// The top-instruction descriptor for a renderer's header line.
#[derive(Clone)]
pub(super) struct Header {
    pub program: Pubkey,
    /// Resolved via the discriminator table, else the first
    /// `Program log: Instruction: <Name>` line. `None` when neither hits.
    pub instruction_name: Option<String>,
}

/// One top-level instruction's invocation subtree plus the tx-required
/// signers referenced by that instruction's accounts.
#[derive(Clone)]
pub(super) struct Root {
    pub signers: Vec<Pubkey>,
    pub frame: ResolvedFrame,
}

/// A CPI frame with names and failure messages already resolved.
#[derive(Clone)]
pub(super) struct ResolvedFrame {
    pub program: Pubkey,
    /// The upstream log-derived name, or a discriminator decode for
    /// well-known programs, or `None`.
    pub instruction_name: Option<String>,
    /// Typed operands decoded by the [interpretation layer](crate::interpret),
    /// carried from the neutral [`Frame`](crate::frame::Frame) so the rich
    /// renderers read the same fact the structured tree and fingerprint do (a
    /// `System::Transfer`'s `from`/`to`/`lamports`). Empty when none.
    pub operands: Vec<(String, crate::interpret::Operand)>,
    pub outcome: Outcome,
    /// Compute units consumed, or `None` when the frame emitted no
    /// `consumed N of M` line (native programs don't).
    pub compute_units: Option<u64>,
    /// The accounts this instruction touched, with their signer/writable
    /// roles. Consumed by the authority-graph renderer; the tree/mermaid
    /// adapters ignore it.
    pub accounts: Vec<AccountRef>,
    /// The frame's own log/data entries, for renderers that surface them.
    pub logs: Vec<FrameLog>,
    /// The instruction's raw data bytes, from the trace. Empty when no trace
    /// covered the frame. The tree renderer decodes self-CPI events from this
    /// (an `emit_cpi!`-style event leaves no `Program data:` log; its payload is
    /// the inner instruction's data).
    pub data: Vec<u8>,
    pub children: Vec<ResolvedFrame>,
}

/// One account referenced by an instruction, with the role it plays.
/// `is_signer` marks an authority; `is_writable` marks an account whose
/// state (lamports / data) the instruction may change.
///
/// Sourced from the per-frame `trace` (the runtime's recorded privileges):
/// the only neutral carrier of an inner frame's account list, so a CPI that
/// signs as its PDA via `invoke_signed` is visible here. A top-level frame
/// falls back to the message account list when no trace covered it.
#[derive(Clone)]
pub(super) struct AccountRef {
    pub pubkey: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
    /// The program that owns this account (its `Account.owner`), from the
    /// trace. `None` only when no trace covered the frame (the ownership graph
    /// then has nothing to draw for it).
    pub owner: Option<Pubkey>,
}

/// A frame's outcome with the failure message already resolved to the
/// Anchor error name when one was present in the logs.
#[derive(Clone)]
pub(super) enum Outcome {
    Success,
    Failed { message: Option<String> },
    Truncated,
}

/// Resolve an instruction's account-index list (indices into the message's
/// `account_keys`) into [`AccountRef`]s carrying the signer/writable role.
/// Out-of-range indices are skipped (defensive; shouldn't happen for a
/// well-formed message).
fn resolve_accounts(indices: &[u8], message: &Message) -> Vec<AccountRef> {
    indices
        .iter()
        .filter_map(|&idx| {
            let i = idx as usize;
            message.account_keys.get(i).map(|pk| AccountRef {
                pubkey: *pk,
                is_signer: is_signer(i, &message.header),
                is_writable: is_writable(i, &message.header, message.account_keys.len()),
                owner: None,
            })
        })
        .collect()
}

/// Build the model from an engine-neutral [`crate::model::Transaction`] (the
/// record every engine produces), instead of from litesvm's raw pieces.
///
/// This is the neutral twin of [`build`] + [`fill_from_trace`]: it reads the
/// already-parsed `frames` for structure (the `cpi_tree` work the backend
/// already did in [`assemble`](crate::model::Transaction::assemble)) and the
/// per-frame `trace` for the account lists, their signer/writable/owner roles,
/// and the inner instruction data. Where `build` sources child accounts and
/// names from litesvm's `inner_instructions`, this sources them from the trace,
/// the only neutral carrier of inner-frame data. Output is byte-identical to
/// `build` for the litesvm path (a populated trace lines up with the inner
/// instructions); engines without a trace render account-less inner frames, the
/// same graceful degradation `fill_from_trace` documents.
pub(super) fn from_transaction(tx: &crate::model::Transaction) -> CpiModel {
    let signers = super::signers::extract(&tx.message);
    let mut trace = tx.trace.as_ref().map(|t| t.0.iter());
    let roots = tx
        .frames
        .iter()
        .enumerate()
        .map(|(i, frame)| {
            // The root frame's accounts and name-decode data come from the
            // i-th top-level instruction (frames line up with message
            // instructions in order), matching `resolve_roots`.
            let ci = tx.message.instructions.get(i);
            Root {
                signers: signers.per_root.get(i).cloned().unwrap_or_default(),
                frame: convert_frame(
                    frame,
                    1,
                    &mut trace,
                    ci.map(|c| c.accounts.as_slice()).unwrap_or(&[]),
                    ci.map(|c| c.data.as_slice()).unwrap_or(&[]),
                    tx,
                ),
            }
        })
        .collect();

    // The header mirrors `build`'s: present for single-instruction sends, the
    // program and decoded name of that one instruction (built-in decode, then
    // the `Instruction:` log line, then the registry).
    let header = (tx.message.instructions.len() == 1).then(|| {
        let ci = &tx.message.instructions[0];
        let program = tx.message.account_keys[ci.program_id_index as usize];
        let pid = program.to_string();
        Header {
            program,
            instruction_name: decode_instruction(&pid, &ci.data)
                .map(str::to_string)
                .or_else(|| {
                    tx.logs.iter().find_map(|log| {
                        log.strip_prefix("Program log: Instruction: ")
                            .map(str::to_string)
                    })
                })
                .or_else(|| {
                    tx.instruction_names
                        .resolve(&pid, &ci.data)
                        .map(str::to_string)
                }),
        }
    });

    CpiModel {
        header,
        roots,
        tx_signers: signers.tx_signers.clone(),
        error: tx.error.clone(),
        compute_units: tx.compute_units,
        fee: tx.fee.unwrap_or(0),
        events: tx.events.clone(),
    }
}

/// Convert one neutral [`Frame`](crate::frame::Frame) into a
/// [`ResolvedFrame`], pulling accounts and inner data from the trace in DFS
/// pre-order lockstep (each frame consumes one traced instruction, exactly as
/// [`fill_from_trace`] correlates them). `root_accounts` / `root_data` feed the
/// no-trace root fallback and the name decode, mirroring [`resolve_frame`].
fn convert_frame(
    frame: &crate::frame::Frame,
    depth: usize,
    trace: &mut Option<std::slice::Iter<'_, TracedInstruction>>,
    root_accounts: &[u8],
    root_data: &[u8],
    tx: &crate::model::Transaction,
) -> ResolvedFrame {
    let traced = trace.as_mut().and_then(|it| it.next());

    // Accounts and inner data come from the trace when present (the only
    // neutral carrier of an inner frame's account list and data). Without a
    // trace, a root frame falls back to its message account list; an inner
    // frame cannot be reconstructed and renders account-less.
    let (accounts, data) = match traced {
        Some(t) => (
            t.accounts
                .iter()
                .map(|a| AccountRef {
                    pubkey: a.pubkey,
                    is_signer: a.is_signer,
                    is_writable: a.is_writable,
                    owner: Some(a.owner),
                })
                .collect(),
            t.data.clone(),
        ),
        None if depth == 1 => (resolve_accounts(root_accounts, &tx.message), Vec::new()),
        None => (Vec::new(), Vec::new()),
    };

    // The name resolves with the same precedence as `resolve_frame`: a name the
    // neutral frame already carries (an `Instruction:` log line) wins; otherwise
    // decode from this frame's data (the root's message data at depth 1, the
    // trace's inner data below) through the built-in decoders and the registry.
    let name_data: &[u8] = if depth == 1 { root_data } else { &data };
    let instruction_name = frame
        .instruction_name
        .clone()
        .or_else(|| resolve_name(&tx.instruction_names, &frame.program_id, name_data));

    let children = frame
        .children
        .iter()
        .map(|c| convert_frame(c, depth + 1, trace, &[], &[], tx))
        .collect();

    ResolvedFrame {
        program: frame.program_id,
        instruction_name,
        operands: frame.operands.clone(),
        outcome: match &frame.outcome {
            crate::frame::Outcome::Success => Outcome::Success,
            crate::frame::Outcome::Truncated => Outcome::Truncated,
            crate::frame::Outcome::Failed { message } => Outcome::Failed {
                message: message.clone(),
            },
        },
        compute_units: frame.compute_units.map(|cu| cu.consumed),
        accounts,
        logs: frame
            .logs
            .iter()
            .map(|l| match l {
                crate::frame::FrameLog::Msg(s) => FrameLog::Msg(s.clone()),
                crate::frame::FrameLog::Data(s) => FrameLog::Data(s.clone()),
            })
            .collect(),
        data,
        children,
    }
}

/// Legacy-message signer rule: the first `num_required_signatures` account
/// keys are the signers.
fn is_signer(index: usize, header: &MessageHeader) -> bool {
    index < header.num_required_signatures as usize
}

/// Legacy-message writability rule. Signers split into writable (the first
/// `S - num_readonly_signed`) then readonly; non-signers split into writable
/// then the last `num_readonly_unsigned` readonly.
fn is_writable(index: usize, header: &MessageHeader, n_accounts: usize) -> bool {
    let signers = header.num_required_signatures as usize;
    if index < signers {
        index < signers.saturating_sub(header.num_readonly_signed_accounts as usize)
    } else {
        index < n_accounts.saturating_sub(header.num_readonly_unsigned_accounts as usize)
    }
}

/// Resolve a frame's instruction name when the logs didn't carry one: the
/// built-in decoder (System / SPL Token / ATA) first, then the registered
/// [`InstructionNames`](super::InstructionNames) table. This is the per-frame
/// twin of the header's resolution in [`build`]; both put the registry last so
/// a name the runtime already knows is never shadowed.
fn resolve_name(
    names: &super::InstructionNames,
    program_id: &solana_pubkey::Pubkey,
    data: &[u8],
) -> Option<String> {
    let pid = program_id.to_string();
    decode_instruction(&pid, data)
        .map(str::to_string)
        .or_else(|| names.resolve(&pid, data).map(str::to_string))
}

/// Decode an instruction's name from its data discriminator, given the
/// invoking program's base58 ID. Returns `None` for programs without a
/// table (e.g. user programs) or unrecognized data shapes.
///
/// Makes CPI trees readable without an external registry: two `Token`
/// frames at the same depth become `Token::TransferChecked` vs
/// `Token::MintTo`. The header reuses this so its `Instruction: ...` line
/// stays consistent with the inner frames' `Program::Name` rendering.
pub(super) fn decode_instruction(program_id: &str, data: &[u8]) -> Option<&'static str> {
    match program_id {
        // SPL Token (legacy) and Token-2022 share their first 25
        // instruction discriminators, so one decoder serves both.
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
        | "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb" => spl_token_instruction_name(data),
        "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL" => spl_ata_instruction_name(data),
        "11111111111111111111111111111111" => system_instruction_name(data),
        _ => None,
    }
}

/// SPL Token uses a 1-byte discriminator. Covers the stable instructions
/// from `spl_token::instruction::TokenInstruction` and Token-2022's prefix.
fn spl_token_instruction_name(data: &[u8]) -> Option<&'static str> {
    Some(match *data.first()? {
        0 => "InitializeMint",
        1 => "InitializeAccount",
        2 => "InitializeMultisig",
        3 => "Transfer",
        4 => "Approve",
        5 => "Revoke",
        6 => "SetAuthority",
        7 => "MintTo",
        8 => "Burn",
        9 => "CloseAccount",
        10 => "FreezeAccount",
        11 => "ThawAccount",
        12 => "TransferChecked",
        13 => "ApproveChecked",
        14 => "MintToChecked",
        15 => "BurnChecked",
        16 => "InitializeAccount2",
        17 => "SyncNative",
        18 => "InitializeAccount3",
        19 => "InitializeMultisig2",
        20 => "InitializeMint2",
        21 => "GetAccountDataSize",
        22 => "InitializeImmutableOwner",
        23 => "AmountToUiAmount",
        24 => "UiAmountToAmount",
        _ => return None,
    })
}

/// AssociatedToken's `Create` has historically had empty data (pre-1.1) or
/// a single discriminator byte (1.1+). Both shapes resolve here.
fn spl_ata_instruction_name(data: &[u8]) -> Option<&'static str> {
    Some(match data.first().copied() {
        None | Some(0) => "Create",
        Some(1) => "CreateIdempotent",
        Some(2) => "RecoverNested",
        _ => return None,
    })
}

/// System program serializes its instruction enum with a 4-byte u32 LE tag
/// (the bincode enum variant index), unlike SPL Token's 1-byte form.
fn system_instruction_name(data: &[u8]) -> Option<&'static str> {
    if data.len() < 4 {
        return None;
    }
    let tag = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    Some(match tag {
        0 => "CreateAccount",
        1 => "Assign",
        2 => "Transfer",
        3 => "CreateAccountWithSeed",
        4 => "AdvanceNonceAccount",
        5 => "WithdrawNonceAccount",
        6 => "InitializeNonceAccount",
        7 => "AuthorizeNonceAccount",
        8 => "Allocate",
        9 => "AllocateWithSeed",
        10 => "AssignWithSeed",
        11 => "TransferWithSeed",
        12 => "UpgradeNonceAccount",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::frame::{Frame, Outcome as FrameOutcome},
        crate::model::{AnchorFailures, Transaction},
        crate::trace::{InstructionTrace, TracedAccount, TracedInstruction},
        solana_message::{compiled_instruction::CompiledInstruction, Message, MessageHeader},
        solana_pubkey::Pubkey,
    };

    /// Assemble a one-frame-or-tree neutral [`Transaction`] from frames + trace.
    /// The thin wrapper every `from_transaction` unit test shares.
    fn assemble(message: Message, frames: Vec<Frame>, trace: Option<InstructionTrace>) -> Transaction {
        Transaction::assemble(
            frames,
            message,
            vec![],
            None,
            0,
            None,
            trace,
            None,
            &Default::default(),
            &Default::default(),
            &AnchorFailures,
            crate::aliases::Aliases::default(),
            Default::default(),
        )
    }

    #[test]
    fn resolve_accounts_marks_signer_and_writable_roles() {
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
    fn from_transaction_sources_inner_accounts_and_names_from_the_trace() {
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
        // Neither carries accounts; the trace is their only source.
        let frames = vec![Frame {
            program_id: program,
            outcome: FrameOutcome::Success,
            compute_units: None,
            instruction_name: Some("Withdraw".into()),
            operands: Vec::new(),
            logs: vec![],
            children: vec![Frame {
                program_id: system,
                outcome: FrameOutcome::Success,
                compute_units: None,
                instruction_name: None,
                operands: Vec::new(),
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

        let tx = assemble(message, frames, Some(trace));
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

        // The whole point: the inner frame's accounts come from the trace, with
        // the trace's owners, and the name decodes from the traced data.
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
            "inner-frame accounts come from the trace",
        );
        assert_eq!(
            child.accounts[1].owner,
            Some(program),
            "the inner frame's owner comes from the trace",
        );
    }

    #[test]
    fn from_transaction_decodes_inner_name_without_shadowing_resolved_name() {
        // The trace pass fills an OPEN inner name but must never overwrite a
        // name the frame already carries (a log-derived or upstream one). The
        // root carries `Withdraw`; the System child is unnamed and decodes from
        // the trace's `CreateAccount` data.
        let program = Pubkey::new_unique();
        let system = Pubkey::default();

        let frames = vec![Frame {
            program_id: program,
            outcome: FrameOutcome::Success,
            compute_units: None,
            instruction_name: Some("Withdraw".into()),
            operands: Vec::new(),
            logs: vec![],
            children: vec![Frame {
                program_id: system,
                outcome: FrameOutcome::Success,
                compute_units: None,
                instruction_name: None,
                operands: Vec::new(),
                logs: vec![],
                children: vec![],
            }],
        }];
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

        let tx = assemble(Message::default(), frames, Some(trace));
        let model = from_transaction(&tx);
        let root = &model.roots[0].frame;
        assert_eq!(
            root.instruction_name.as_deref(),
            Some("Withdraw"),
            "an already-resolved frame name must not be shadowed by the trace pass"
        );
        assert_eq!(
            root.children[0].instruction_name.as_deref(),
            Some("CreateAccount"),
            "the inner native-program frame resolves its name from the traced data"
        );
    }

    #[test]
    fn from_transaction_builds_one_root_per_top_level_instruction() {
        // Batch send: two top-level frames -> two roots, and no single header
        // (a batch carries no one canonical instruction).
        let prog_a = Pubkey::new_unique();
        let prog_b = Pubkey::new_unique();
        let message = Message {
            account_keys: vec![prog_a, prog_b],
            instructions: vec![
                CompiledInstruction {
                    program_id_index: 0,
                    accounts: vec![],
                    data: vec![],
                },
                CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![],
                    data: vec![],
                },
            ],
            ..Default::default()
        };
        let frames = vec![
            Frame {
                program_id: prog_a,
                outcome: FrameOutcome::Success,
                compute_units: None,
                instruction_name: Some("First".into()),
                operands: Vec::new(),
                logs: vec![],
                children: vec![],
            },
            Frame {
                program_id: prog_b,
                outcome: FrameOutcome::Success,
                compute_units: None,
                instruction_name: Some("Second".into()),
                operands: Vec::new(),
                logs: vec![],
                children: vec![],
            },
        ];

        let tx = assemble(message, frames, None);
        let model = from_transaction(&tx);
        assert_eq!(model.roots.len(), 2, "one root per top-level instruction");
        assert_eq!(model.roots[0].frame.program, prog_a);
        assert_eq!(model.roots[1].frame.program, prog_b);
        assert!(
            model.header.is_none(),
            "a batch send carries no single header"
        );
    }

    #[test]
    fn from_transaction_degrades_inner_frames_to_account_less_without_a_trace() {
        // No trace: a root falls back to its message account list, but an inner
        // CPI frame can't be reconstructed and renders account-less (the
        // documented graceful degradation for trace-less engines).
        let program = Pubkey::new_unique();
        let system = Pubkey::default();
        let payer = Pubkey::new_unique();
        let message = Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![payer, program],
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0],
                data: vec![],
            }],
            ..Default::default()
        };
        let frames = vec![Frame {
            program_id: program,
            outcome: FrameOutcome::Success,
            compute_units: None,
            instruction_name: Some("Withdraw".into()),
            operands: Vec::new(),
            logs: vec![],
            children: vec![Frame {
                program_id: system,
                outcome: FrameOutcome::Success,
                compute_units: None,
                instruction_name: None,
                operands: Vec::new(),
                logs: vec![],
                children: vec![],
            }],
        }];

        let tx = assemble(message, frames, None);
        let model = from_transaction(&tx);
        let root = &model.roots[0].frame;
        // The root recovers its accounts from the message.
        assert_eq!(
            root.accounts.iter().map(|a| a.pubkey).collect::<Vec<_>>(),
            vec![payer],
            "the root frame falls back to its message account list",
        );
        // The inner frame has nothing to reconstruct from: account-less.
        assert!(
            root.children[0].accounts.is_empty(),
            "an inner frame renders account-less without a trace",
        );
    }

    #[test]
    fn from_transaction_carries_a_failed_frame_message() {
        // A failed frame's resolved message rides onto the CpiModel so the
        // renderers can surface it. The Anchor `Error Code:` resolution runs in
        // `assemble`; here we pin that the failed outcome and its message
        // survive the conversion.
        let program = Pubkey::new_unique();
        let message = Message {
            account_keys: vec![program],
            instructions: vec![CompiledInstruction {
                program_id_index: 0,
                accounts: vec![],
                data: vec![],
            }],
            ..Default::default()
        };
        let frames = vec![Frame {
            program_id: program,
            outcome: FrameOutcome::Failed {
                message: Some("custom program error: 0x1770".into()),
            },
            compute_units: None,
            instruction_name: Some("Take".into()),
            operands: Vec::new(),
            logs: vec![crate::frame::FrameLog::Msg(
                "AnchorError thrown in take.rs:1. Error Code: EscrowExpired. Error Number: 6000."
                    .into(),
            )],
            children: vec![],
        }];

        let tx = assemble(message, frames, None);
        let model = from_transaction(&tx);
        match &model.roots[0].frame.outcome {
            Outcome::Failed { message } => assert_eq!(
                message.as_deref(),
                Some("EscrowExpired"),
                "the Anchor error name resolves onto the failed frame",
            ),
            other => panic!("expected a failed frame, got {other:?}"),
        }
    }

    #[test]
    fn from_transaction_carries_inner_frame_events() {
        // A `Program data:` payload on an inner frame survives onto the model's
        // inner ResolvedFrame, so an event a CPI emitted is renderable.
        let program = Pubkey::new_unique();
        let system = Pubkey::default();
        let message = Message {
            account_keys: vec![program],
            instructions: vec![CompiledInstruction {
                program_id_index: 0,
                accounts: vec![],
                data: vec![],
            }],
            ..Default::default()
        };
        let frames = vec![Frame {
            program_id: program,
            outcome: FrameOutcome::Success,
            compute_units: None,
            instruction_name: Some("Make".into()),
            operands: Vec::new(),
            logs: vec![],
            children: vec![Frame {
                program_id: system,
                outcome: FrameOutcome::Success,
                compute_units: None,
                instruction_name: None,
                operands: Vec::new(),
                logs: vec![crate::frame::FrameLog::Data("AAAAAAAAAAA=".into())],
                children: vec![],
            }],
        }];

        let tx = assemble(message, frames, None);
        let model = from_transaction(&tx);
        let child = &model.roots[0].frame.children[0];
        assert_eq!(
            child.logs,
            vec![FrameLog::Data("AAAAAAAAAAA=".to_string())],
            "the inner frame's event payload rides onto the model",
        );
    }
}

#[cfg(test)]
impl std::fmt::Debug for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::Success => write!(f, "Success"),
            Outcome::Truncated => write!(f, "Truncated"),
            Outcome::Failed { message } => write!(f, "Failed {{ message: {message:?} }}"),
        }
    }
}
