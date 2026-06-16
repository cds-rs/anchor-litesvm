//! The shared transformation every renderer consumes.
//!
//! Dataflow: `litesvm` -> `litesvm::cpi_tree` (the tree-api) -> **this
//! module** -> renderers. Upstream's [`cpi_tree`] does the structural
//! parse (nesting, outcome classification, log attribution); this module
//! enriches that raw [`CpiFrame`] tree into a [`CpiModel`] that carries
//! *everything* a renderer needs:
//!
//! - instruction names resolved (the upstream log-derived name, or a
//!   discriminator decode for well-known programs when the log heuristic
//!   missed one),
//! - failure messages resolved (the Anchor `Error Code:` name lifted out
//!   of the frame logs, falling back to the runtime message),
//! - per-root and tx-level signers,
//! - the top-instruction header and the tx-level cu / fee / error needed
//!   for a renderer's own framing.
//!
//! Renderers read the model and nothing else: they never re-walk
//! `cpi_tree`, re-thread the inner-instruction iter, or re-decode. That
//! single source of truth is the whole point of pulling this out — the
//! tree and mermaid adapters used to do all of it twice.

use {
    super::trace::{InstructionTrace, TracedInstruction},
    litesvm::cpi_tree::{cpi_tree, CpiFrame, CpiOutcome},
    solana_message::{
        inner_instruction::{InnerInstruction, InnerInstructionsList},
        Message, MessageHeader,
    },
    solana_program::pubkey::Pubkey,
};

/// The per-frame log entries (`Program log:` / `Program data:`), passed
/// through from the tree-api so renderers can surface events without
/// reaching back to the raw stream.
pub(super) use litesvm::cpi_tree::FrameLog;

/// The name tables consulted while building the model: instruction names for
/// frame labels, error names for failure messages. Bundled and passed by
/// (copyable) reference so the build path keeps one parameter as more
/// vocabularies appear, rather than growing a positional argument each time.
/// Both default to empty, in which case resolution falls back to the built-in
/// decoders and the raw runtime strings, exactly as before any registration.
#[derive(Clone, Copy)]
pub(super) struct Vocab<'a> {
    pub instructions: &'a super::InstructionNames,
    pub errors: &'a super::ErrorNames,
    /// Decoders for registered Anchor events, so a `Program data:` payload
    /// renders by name and fields. Empty by default (events stay raw base64).
    pub events: &'a super::EventRegistry,
}

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
    /// Every failed frame's resolved failure message, in traversal order. This
    /// is what lets [`assert_error`](super::TransactionResult::assert_error)
    /// match a registered error *name*: the name is produced by
    /// [`resolve_custom_error`] during the build and lands here, even though it
    /// never appears in the raw logs or the runtime error field.
    pub(super) fn failure_messages(&self) -> Vec<String> {
        fn walk(frame: &ResolvedFrame, out: &mut Vec<String>) {
            if let Outcome::Failed { message: Some(m) } = &frame.outcome {
                out.push(m.clone());
            }
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
    /// The instruction's raw data bytes, filled from the execution trace
    /// ([`fill_from_trace`]). Empty when no trace covered the frame. The tree
    /// renderer decodes self-CPI events from this (an `emit_cpi!`-style event
    /// leaves no `Program data:` log; its payload is the inner instruction's
    /// data).
    pub data: Vec<u8>,
    pub children: Vec<ResolvedFrame>,
}

/// One account referenced by an instruction, with the role it plays.
/// `is_signer` marks an authority; `is_writable` marks an account whose
/// state (lamports / data) the instruction may change.
///
/// STOPGAP SOURCE: today this is reconstructed from the transaction
/// `Message` (account keys + header) and the inner-instruction account
/// index lists, because `litesvm::cpi_tree`'s [`CpiFrame`] does not carry
/// it. The plan is to lobby litesvm to expose account/authority metadata on
/// the frame directly (no second derivation). If that lands, only
/// [`resolve_frame`]'s population changes — this type and every renderer
/// stay put.
#[derive(Clone)]
pub(super) struct AccountRef {
    pub pubkey: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
    /// The program that owns this account (its `Account.owner`), if known.
    /// `None` until [`fill_owners`] populates it: the owner is post-execution
    /// account state, not present in the message or the logs, so `build`
    /// leaves it unset and the ownership-graph entry point fills it via an svm
    /// lookup. Same stopgap as the rest of this type; the litesvm-metadata win
    /// removes the separate lookup here too.
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

/// Build the full model from a transaction's raw pieces. This is the
/// transformation all renderers share; see the module docs.
#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    header_ix: Option<&super::InstructionInfo>,
    logs: &[String],
    inner_instructions: &InnerInstructionsList,
    message: &Message,
    signers: &super::signers::SignerInfo,
    error: Option<String>,
    compute_units: u64,
    fee: u64,
    vocab: Vocab<'_>,
) -> CpiModel {
    let header = header_ix.map(|info| Header {
        program: info.program_id,
        // Resolution order: built-in decoder (System / SPL / ATA) -> the
        // `Program log: Instruction: <Name>` line Anchor emits -> the
        // registered discriminator table. The registry is last so it never
        // shadows a name the runtime or a built-in already knows.
        instruction_name: decode_instruction(&info.program_id.to_string(), &info.data)
            .map(str::to_string)
            .or_else(|| {
                logs.iter().find_map(|log| {
                    log.strip_prefix("Program log: Instruction: ")
                        .map(str::to_string)
                })
            })
            .or_else(|| {
                vocab
                    .instructions
                    .resolve(&info.program_id.to_string(), &info.data)
                    .map(str::to_string)
            }),
    });
    CpiModel {
        header,
        roots: resolve_roots(logs, inner_instructions, message, signers, vocab),
        tx_signers: signers.tx_signers.clone(),
        error,
        compute_units,
        fee,
        events: vocab.events.clone(),
    }
}

/// Parse and resolve the CPI forest only (no header / cu / fee). Exposed
/// so the renderer body tests can drive the same resolution the full
/// `build` uses without fabricating tx-level metadata.
pub(super) fn resolve_roots(
    logs: &[String],
    inner_instructions: &InnerInstructionsList,
    message: &Message,
    signers: &super::signers::SignerInfo,
    vocab: Vocab<'_>,
) -> Vec<Root> {
    let tree = cpi_tree(logs);
    tree.iter()
        .enumerate()
        .map(|(i, root)| {
            // Solana emits inner_instructions in DFS pre-order per root,
            // matching the log stream's invocation order; the iter
            // advances once per CPI child as resolve_frame descends.
            let mut ix_iter = inner_instructions.get(i).map(|v| v.iter());
            // The root frame's accounts come from the i-th top-level
            // instruction (cpi_tree roots line up with message instructions
            // in order); CPI children pull theirs from the inner-instruction
            // list inside resolve_frame.
            let root_accounts = message
                .instructions
                .get(i)
                .map(|ci| ci.accounts.as_slice())
                .unwrap_or(&[]);
            // The root frame's data, for the registry name lookup: a Pinocchio
            // root emits no `Instruction:` log line, so its name comes from the
            // discriminator in this data, same as a CPI child's does from the
            // inner instruction's data.
            let root_data = message
                .instructions
                .get(i)
                .map(|ci| ci.data.as_slice())
                .unwrap_or(&[]);
            Root {
                signers: signers.per_root.get(i).cloned().unwrap_or_default(),
                frame: resolve_frame(
                    root,
                    1,
                    &mut ix_iter,
                    root_accounts,
                    root_data,
                    message,
                    vocab,
                ),
            }
        })
        .collect()
}

fn resolve_frame(
    frame: &CpiFrame,
    depth: usize,
    ix_iter: &mut Option<std::slice::Iter<'_, InnerInstruction>>,
    root_accounts: &[u8],
    root_data: &[u8],
    message: &Message,
    vocab: Vocab<'_>,
) -> ResolvedFrame {
    // For CPI children (depth > 1), pull the next inner instruction so the
    // iter stays positionally aligned with the tree's DFS pre-order; that
    // same inner instruction supplies the frame's account index list and the
    // data we decode its name from. The advance must happen even when we use
    // neither, or the iter desyncs from subsequent siblings. Root frames
    // (depth 1) take their accounts from `root_accounts` and decode their name
    // from `root_data` instead.
    let mut decoded: Option<String> = None;
    let accounts = if depth > 1 {
        match ix_iter.as_mut().and_then(|it| it.next()) {
            Some(inner) => {
                if frame.instruction_name.is_none() {
                    decoded = resolve_name(
                        vocab.instructions,
                        &frame.program_id,
                        &inner.instruction.data,
                    );
                }
                resolve_accounts(&inner.instruction.accounts, message)
            }
            None => Vec::new(),
        }
    } else {
        if frame.instruction_name.is_none() {
            decoded = resolve_name(vocab.instructions, &frame.program_id, root_data);
        }
        resolve_accounts(root_accounts, message)
    };
    let instruction_name = frame.instruction_name.clone().or(decoded);

    let outcome = match &frame.outcome {
        CpiOutcome::Success => Outcome::Success,
        CpiOutcome::Truncated => Outcome::Truncated,
        CpiOutcome::Failed { message } => {
            // Resolution order, best name first:
            //   1. Anchor's `AnchorError ... Error Code: <Name>` log line
            //      (`EscrowExpired`, or `AccountNotSigner on authority` when a
            //      constraint names the offending account);
            //   2. the registered error table, keyed by the failing frame's
            //      program and the `custom program error: 0x<code>` it returned
            //      (the Pinocchio path: `InvalidAmount` instead of `0x7`);
            //   3. the runtime's raw message, unchanged.
            let anchor = resolve_anchor_failure(&frame.logs);
            let registered = anchor
                .is_none()
                .then(|| resolve_custom_error(vocab.errors, &frame.program_id, message.as_deref()));
            Outcome::Failed {
                message: anchor
                    .or_else(|| registered.flatten())
                    .or_else(|| message.clone()),
            }
        }
    };

    let children = frame
        .children
        .iter()
        // Children take both accounts and data from the inner-instruction iter,
        // so `root_accounts` / `root_data` are unused for them (`&[]`).
        .map(|c| resolve_frame(c, depth + 1, ix_iter, &[], &[], message, vocab))
        .collect();

    ResolvedFrame {
        program: frame.program_id,
        instruction_name,
        outcome,
        compute_units: frame.compute_units.as_ref().map(|cu| cu.consumed),
        accounts,
        logs: frame.logs.clone(),
        data: Vec::new(),
        children,
    }
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

/// Fill in each account's `owner` via `lookup` (typically
/// `|pk| svm.get_account(pk).map(|a| a.owner)`). A post-build enrichment
/// step: the owner is account state the message/logs don't carry, so the
/// ownership graph supplies it here rather than at `build` time. Taking a
/// closure keeps this module free of any svm dependency; if litesvm starts
/// carrying owner metadata on the frame, `build` fills `owner` directly and
/// this goes away.
pub(super) fn fill_owners(model: &mut CpiModel, lookup: impl Fn(&Pubkey) -> Option<Pubkey>) {
    for root in &mut model.roots {
        fill_frame_owners(&mut root.frame, &lookup);
    }
}

fn fill_frame_owners(frame: &mut ResolvedFrame, lookup: &impl Fn(&Pubkey) -> Option<Pubkey>) {
    for acct in &mut frame.accounts {
        acct.owner = lookup(&acct.pubkey);
    }
    for child in &mut frame.children {
        fill_frame_owners(child, lookup);
    }
}

/// Override each account's `is_signer` / `is_writable` / `owner` with the
/// runtime's recorded facts from the instruction trace. [`build`] derives those
/// flags from the message header, which only knows *top-level* privileges; a
/// CPI frame that signs as its PDA via `invoke_signed` is invisible there. The
/// trace records the privilege each frame actually presented, so this is what
/// makes the authority view correct for inner frames.
///
/// Model frames (DFS pre-order) align with the trace's frames (execution
/// order); within a frame, accounts are matched by pubkey. A frame the trace
/// doesn't cover keeps its message-derived flags, so a desync degrades
/// gracefully rather than corrupting.
pub(super) fn fill_from_trace(model: &mut CpiModel, trace: &InstructionTrace, vocab: Vocab<'_>) {
    let mut frames = trace.0.iter();
    for root in &mut model.roots {
        fill_frame_from_trace(&mut root.frame, &mut frames, vocab);
    }
}

fn fill_frame_from_trace<'a>(
    frame: &mut ResolvedFrame,
    frames: &mut impl Iterator<Item = &'a TracedInstruction>,
    vocab: Vocab<'_>,
) {
    if let Some(traced) = frames.next() {
        for acct in &mut frame.accounts {
            if let Some(t) = traced.accounts.iter().find(|t| t.pubkey == acct.pubkey) {
                acct.is_signer = t.is_signer;
                acct.is_writable = t.is_writable;
                acct.owner = Some(t.owner);
            }
        }
        // The trace is the only carrier of an inner frame's instruction data
        // (logs don't have it); the tree renderer decodes self-CPI events from it.
        frame.data = traced.data.clone();
        // On the engine-neutral backend path `inner_instructions` is empty, so
        // `resolve_frame` never gets to name a CPI child; the trace is the only
        // place the inner data surfaces. Resolve the name here too, with the same
        // resolver (built-in System/Token/ATA decoders, then the registry), so an
        // inner `Transfer` / `CreateAccount` names itself instead of rendering as
        // `unnamed`. Only when the build path left it open, so a log-derived or
        // discriminator-decoded name is never shadowed.
        if frame.instruction_name.is_none() {
            frame.instruction_name = resolve_name(vocab.instructions, &frame.program, &traced.data);
        }
    }
    for child in &mut frame.children {
        fill_frame_from_trace(child, frames, vocab);
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

/// Lift the friendly error name out of an Anchor-thrown error log when
/// present.
///
/// Anchor's `#[error_code]` macro emits a structured log line on failure:
///
/// ```text
/// AnchorError thrown in <file>:<line>. Error Code: <Name>. Error Number: 6000. Error Message: <Name>.
/// ```
///
/// (Other variants: `AnchorError caused by account: ...`, `AnchorError
/// occurred. ...`. All carry the `Error Code: <Name>.` segment.) Returns
/// `None` for non-Anchor failures or a missing/malformed line.
fn extract_anchor_error_name(logs: &[FrameLog]) -> Option<String> {
    for entry in logs {
        let FrameLog::Msg(text) = entry else { continue };
        if !text.starts_with("AnchorError") {
            continue;
        }
        let Some(after_code) = text.split_once("Error Code: ").map(|(_, s)| s) else {
            continue;
        };
        // The name terminates at the next `.` (Anchor's separator between
        // `Error Code: <Name>` and `Error Number: <N>`).
        let Some((name, _)) = after_code.split_once('.') else {
            continue;
        };
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// The account Anchor blames for a constraint failure, when its log names one:
///
/// ```text
/// AnchorError caused by account: <field>. Error Code: <Name>. ...
/// ```
///
/// Returns `None` for failure forms that name no account (a `require!`'s
/// `thrown in <file>` form, native-program failures). Mirrors
/// [`extract_anchor_error_name`]'s scanning rules.
fn extract_anchor_error_account(logs: &[FrameLog]) -> Option<String> {
    for entry in logs {
        let FrameLog::Msg(text) = entry else { continue };
        if !text.starts_with("AnchorError") {
            continue;
        }
        let Some(after) = text.split_once("caused by account: ").map(|(_, s)| s) else {
            continue;
        };
        // The field terminates at Anchor's next `.` separator (before
        // `Error Code:`).
        let Some((field, _)) = after.split_once('.') else {
            continue;
        };
        let trimmed = field.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Resolve an Anchor failure to a frame label: the error name, plus the
/// offending account when Anchor names one. So a constraint failure renders as
/// `AccountNotSigner on authority` (the extra entropy a failed frame can carry
/// without cluttering the happy path), while a `require!` failure stays just
/// `EscrowExpired`. `None` for non-Anchor failures, so the caller falls back to
/// the runtime message.
fn resolve_anchor_failure(logs: &[FrameLog]) -> Option<String> {
    let name = extract_anchor_error_name(logs)?;
    match extract_anchor_error_account(logs) {
        Some(account) => Some(format!("{name} on {account}")),
        None => Some(name),
    }
}

/// Resolve a frame's `custom program error: 0x<code>` failure message to the
/// program's registered error name. Returns `None` when the message isn't a
/// custom-error code, the code doesn't parse, or the program registered no name
/// for it (the caller then keeps the raw message). The failure-path twin of
/// [`resolve_name`]; both consult the registry only as a fallback.
fn resolve_custom_error(
    errors: &super::ErrorNames,
    program_id: &Pubkey,
    message: Option<&str>,
) -> Option<String> {
    if errors.is_empty() {
        return None;
    }
    let hex = message?.strip_prefix("custom program error: 0x")?;
    let code = u32::from_str_radix(hex.trim(), 16).ok()?;
    errors
        .resolve(&program_id.to_string(), code)
        .map(str::to_string)
}

/// Resolve a frame's instruction name when the logs didn't carry one: the
/// built-in decoder (System / SPL Token / ATA) first, then the registered
/// [`InstructionNames`](super::InstructionNames) table. This is the per-frame
/// twin of the header's resolution in [`build`]; both put the registry last so
/// a name the runtime already knows is never shadowed.
fn resolve_name(
    names: &super::InstructionNames,
    program_id: &solana_program::pubkey::Pubkey,
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
mod tests;
