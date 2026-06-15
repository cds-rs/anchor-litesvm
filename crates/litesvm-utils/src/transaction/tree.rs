//! Render a transaction's CPI invocation tree as an annotated box-drawing
//! tree.
//!
//! The structural parse from a flat log stream into [`CpiFrame`]s lives
//! in [`crate::cpi_tree`], which on this branch is a local port of
//! litesvm 0.12's `cpi_tree` module (see that file's header). This
//! module owns presentation: alias resolution via a user-supplied
//! [`Aliases`](super::aliases::Aliases),
//! discriminator-based instruction-name decoding for well-known programs
//! (System, SPL Token, ATA) when the parser's log-line heuristic
//! didn't catch one, signer annotations, and the box-drawing tree itself.
//!
//! Program IDs and signer pubkeys are substituted with friendly names via
//! the [`LegendCollector`], which both resolves and records each
//! `(name, Pubkey)` pair seen during a render pass; the caller drains the
//! recorded pairs to print the legend footer. Unaliased pubkeys are
//! truncated to `<8>…<4>` so trees stay narrow.

use {
    crate::cpi_tree::{cpi_tree, CpiFrame, CpiOutcome, FrameLog},
    indexmap::IndexMap,
    solana_message::inner_instruction::{InnerInstruction, InnerInstructionsList},
    solana_program::pubkey::Pubkey,
    std::fmt::Write,
};

const TREE_BRANCH: &str = "├── ";
const TREE_END: &str = "└── ";
const TREE_CONT: &str = "│   ";
const TREE_EMPTY: &str = "    ";

/// Wraps an [`Aliases`](super::aliases::Aliases) for the duration of a
/// render pass and records, in insertion order, each `(name, Pubkey)` pair
/// that was actually resolved.
///
/// The render fns call into the collector instead of [`Aliases`] directly,
/// so the legend reflects only aliases that appeared in this particular
/// tree (not every alias the user supplied). One entry per name; the
/// first-seen `Pubkey` wins (later occurrences are silently deduplicated).
///
/// `seen` is an [`IndexMap`] (hash map with insertion-order iteration)
/// rather than a `HashMap` so the legend prints in the order names first
/// appeared in the tree above. A plain `HashMap`'s randomized iteration
/// would also make snapshot tests flake. Dedup goes through
/// [`IndexMap::entry`]`.or_insert(..)`: first-seen wins, O(1) check.
pub(super) struct LegendCollector<'a> {
    aliases: &'a super::aliases::Aliases,
    events: &'a super::events::EventRegistry,
    seen: IndexMap<&'a str, Pubkey>,
}

impl<'a> LegendCollector<'a> {
    pub(super) fn new(
        aliases: &'a super::aliases::Aliases,
        events: &'a super::events::EventRegistry,
    ) -> Self {
        Self {
            aliases,
            events,
            seen: IndexMap::new(),
        }
    }

    /// Decode a `Program data:` base64 payload into a named, field-formatted
    /// event, with `Pubkey`s in the fields substituted to their aliases. `None`
    /// when no decoder is registered for the payload's discriminator (the
    /// renderer then keeps the raw form). The fields arrive from the decoder
    /// with base58 keys; this is the one render-time place they're aliased,
    /// since a decoded event is free text, not a typed `Pubkey` to resolve.
    pub(super) fn decode_event(&self, payload: &str) -> Option<super::events::EventInfo> {
        let mut info = self.events.decode(payload)?;
        info.fields = self.aliases.substitute_in_text(&info.fields);
        Some(info)
    }

    /// The recorded `(name, Pubkey)` pairs in insertion order.
    pub(super) fn into_entries(self) -> IndexMap<&'a str, Pubkey> {
        self.seen
    }

    /// First-seen wins: later occurrences of `name` keep the original
    /// `Pubkey` and don't shift its position in the legend.
    fn record(&mut self, name: &'a str, pk: Pubkey) {
        self.seen.entry(name).or_insert(pk);
    }

    /// Resolve a `Pubkey` to a name (recording the alias for the legend)
    /// or fall back to `<first 8>…<last 4>` truncation. Called for every
    /// program ID and signer pubkey rendered through the tree.
    pub(super) fn render_pubkey(&mut self, pubkey: &Pubkey) -> String {
        if let Some(name) = self.aliases.resolve_by_pubkey(pubkey) {
            self.record(name, *pubkey);
            return name.to_string();
        }
        super::aliases::short_pubkey(pubkey)
    }
}

/// Render the invocation tree from a transaction's logs and inner
/// instructions.
///
/// The flat log stream is parsed into a nested [`CpiFrame`] tree by
/// [`crate::cpi_tree`]. This function walks that tree and renders each
/// frame as a box-drawing line, threading the `inner_instructions` list
/// per root so child frames can pull their discriminator bytes for
/// [`decode_instruction`] when the log-derived name on the frame is `None`.
///
/// Returns an empty string if the log stream contains no invocations;
/// otherwise returns the tree body prefixed by `"Transaction\n"`. Aliases
/// resolved during rendering are recorded on `collector` so the caller
/// can render a footer legend.
pub(super) fn render(
    logs: &[String],
    inner_instructions: &InnerInstructionsList,
    collector: &mut LegendCollector<'_>,
    signers: &super::signers::SignerInfo,
    style: super::style::Style,
) -> String {
    let tree = cpi_tree(logs);
    fmt_tree(&tree, inner_instructions, collector, signers, style)
}

fn fmt_tree(
    tree: &[CpiFrame],
    inner_instructions: &InnerInstructionsList,
    collector: &mut LegendCollector<'_>,
    signers: &super::signers::SignerInfo,
    style: super::style::Style,
) -> String {
    if tree.is_empty() {
        return String::new();
    }
    let mut out = String::from("Transaction");
    if !signers.tx_signers.is_empty() {
        let names: Vec<String> = signers
            .tx_signers
            .iter()
            .map(|pk| collector.render_pubkey(pk))
            .collect();
        let _ = write!(out, "  signers=[{}]", names.join(", "));
    }
    out.push('\n');

    let last = tree.len() - 1;
    for (i, root) in tree.iter().enumerate() {
        let signer_set = signers.per_root.get(i);
        // Solana emits inner_instructions in DFS pre-order per root, matching
        // the log stream's invocation order; the iter here advances once
        // per CPI child (depth > 1) as render_frame descends through the
        // root's subtree.
        let mut ix_iter = inner_instructions.get(i).map(|v| v.iter());
        render_frame(
            root,
            "",
            i == last,
            1,
            collector,
            signer_set,
            &mut ix_iter,
            style,
            &mut out,
        );
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn render_frame(
    frame: &CpiFrame,
    ancestor_prefix: &str,
    is_last: bool,
    depth: usize,
    collector: &mut LegendCollector<'_>,
    signer_set: Option<&Vec<Pubkey>>,
    ix_iter: &mut Option<std::slice::Iter<'_, InnerInstruction>>,
    style: super::style::Style,
    out: &mut String,
) {
    // For CPI children (depth > 1), pull the next inner instruction so the
    // iter stays positionally aligned with the tree's DFS pre-order. If the
    // frame doesn't already have a log-derived `instruction_name` from
    // upstream, try the discriminator decoder against the inner ix data.
    // The advance has to happen even when we wouldn't use the decoded name,
    // because skipping it would desync the iter from subsequent siblings.
    let mut decoded: Option<String> = None;
    if depth > 1 {
        if let Some(it) = ix_iter.as_mut() {
            if let Some(inner) = it.next() {
                if frame.instruction_name.is_none() {
                    decoded =
                        decode_instruction(&frame.program_id.to_string(), &inner.instruction.data)
                            .map(str::to_string);
                }
            }
        }
    }
    let instruction = frame.instruction_name.as_deref().or(decoded.as_deref());

    let connector = if is_last { TREE_END } else { TREE_BRANCH };
    let program_display = collector.render_pubkey(&frame.program_id);
    let _ = match instruction {
        Some(name) => write!(
            out,
            "{}{}{}::{} [{}]",
            ancestor_prefix, connector, program_display, name, depth
        ),
        None => write!(
            out,
            "{}{}{} [{}]",
            ancestor_prefix, connector, program_display, depth
        ),
    };
    match &frame.outcome {
        CpiOutcome::Success => {
            out.push(' ');
            out.push_str(&style.green("✓"));
        }
        CpiOutcome::Failed { .. } => {
            out.push(' ');
            out.push_str(&style.red("✗"));
        }
        CpiOutcome::Truncated => {
            out.push(' ');
            out.push_str(&style.dim("(truncated)"));
        }
    }
    // `compute_units = None` means the log stream had no
    // `Program X consumed N of M compute units` line for this frame.
    // Native programs (System, BPF Loader, etc.) don't emit it; see
    // agave/cli/src/cluster_query.rs around `transaction_total_cu`,
    // which makes the same observation at the transaction level.
    // Surface the absence explicitly so a reader doesn't mistake it
    // for a parser drop.
    match frame.compute_units {
        Some(cu) => {
            let _ = write!(out, " {}cu", cu.consumed);
        }
        None => {
            out.push(' ');
            out.push_str(&style.dim("(no cu)"));
        }
    }
    // Signer annotation: top-level frames only (depth == 1) and only when
    // a signer_set is supplied. Per the spec, signer=X means "X is a
    // tx-required signer whose pubkey is referenced in this ix's
    // accounts", NOT "X authorized this ix". Fee payers that appear in
    // many ixs they didn't authorize will still show up here.
    if depth == 1 {
        if let Some(set) = signer_set {
            if !set.is_empty() {
                let names: Vec<String> = set.iter().map(|pk| collector.render_pubkey(pk)).collect();
                if names.len() == 1 {
                    let _ = write!(out, "  signer={}", names[0]);
                } else {
                    let _ = write!(out, "  signer=[{}]", names.join(", "));
                }
            }
        }
    }
    out.push('\n');

    let descendant_prefix = format!(
        "{}{}",
        ancestor_prefix,
        if is_last { TREE_EMPTY } else { TREE_CONT }
    );

    // Decoded events this frame emitted, as annotation lines indented under it.
    // Only *registered* events render here; an unregistered event is a raw
    // base64 blob with no readable form, so the tree omits it (the mermaid view
    // keeps a raw arrow for those). Placed before children: the frame announced
    // the event, then its sub-calls ran.
    for entry in &frame.logs {
        let FrameLog::Data(payload) = entry else {
            continue;
        };
        let Some(info) = collector.decode_event(payload) else {
            continue;
        };
        let _ = writeln!(out, "{descendant_prefix}{}", info.badge());
    }

    // Order under a frame: children first (in invocation order), then
    // the failure line. Solana logs the inner CPIs before the parent's
    // post-CPI check fires, so chronologically children precede the
    // error; rendering them in that order keeps the tree honest. The
    // `last` flag picks the connector: only the truly-last node at this
    // depth gets `└──`. When there's an error, no child can be last
    // (the error follows); when there isn't, the last child is last.
    let has_error_msg = matches!(
        &frame.outcome,
        CpiOutcome::Failed { message } if message.is_some()
    );
    let children = &frame.children;
    for (i, child) in children.iter().enumerate() {
        let child_is_last = !has_error_msg && i + 1 == children.len();
        // CPI children never get signer annotations (None).
        render_frame(
            child,
            &descendant_prefix,
            child_is_last,
            depth + 1,
            collector,
            None,
            ix_iter,
            style,
            out,
        );
    }
    if let CpiOutcome::Failed { message } = &frame.outcome {
        // Prefer the Anchor-decoded error name (`EscrowExpired`) over
        // the runtime's raw `custom program error: 0x1770`. Falls back
        // to the runtime message when the frame is from a non-Anchor
        // program or the AnchorError log line is absent.
        let anchor_name = extract_anchor_error_name(&frame.logs);
        let best_msg = anchor_name.as_deref().or(message.as_deref());
        render_failure(best_msg, &descendant_prefix, style, out);
    }
}

/// Render the optional error-message line under a failed frame. Shares
/// `descendant_prefix` (carrying the parent frame's vertical bar) and uses
/// the same └── connector as a sole child, so the error aligns with the
/// rest of the subtree. Long messages are split on `. ` for readability.
/// The whole "Error: ..." line is wrapped in red when `style` is `On`.
fn render_failure(
    message: Option<&str>,
    descendant_prefix: &str,
    style: super::style::Style,
    out: &mut String,
) {
    let Some(msg) = message else { return };
    let item = format!("Error: {}", msg);
    let mut chunks = item.split(". ");
    if let Some(first) = chunks.next() {
        let _ = writeln!(
            out,
            "{}{}{}",
            descendant_prefix,
            TREE_END,
            style.red(first)
        );
    }
    for chunk in chunks {
        let _ = writeln!(
            out,
            "{}{} {}",
            descendant_prefix,
            TREE_EMPTY,
            style.red(chunk)
        );
    }
}

/// Lift the friendly error name out of an Anchor-thrown error log
/// when present.
///
/// Anchor's `#[error_code]` macro emits a structured log line on
/// failure, of the shape:
///
/// ```text
/// AnchorError thrown in <file>:<line>. Error Code: <Name>. Error Number: 6000. Error Message: <Name>.
/// ```
///
/// (Other variants exist: `AnchorError caused by account: ...` for
/// constraint failures, `AnchorError occurred. ...` for raw `err!`
/// calls. All of them carry the `Error Code: <Name>.` segment.)
///
/// Scans `logs` for the first `Msg` entry beginning with
/// `"AnchorError"` and extracts the `<Name>` between `Error Code: `
/// and the next `.`. Returns `None` for failures from non-Anchor
/// programs (native programs, raw `solana_program::msg!` users), or
/// when the AnchorError line is missing or malformed.
///
/// Used by both the structured tree renderer and the Mermaid emitter
/// to replace the runtime's `custom program error: 0x1770` with the
/// developer-meaningful name. The two formats then read the same way
/// (`Error: EscrowExpired` vs `✗ EscrowExpired`).
pub(super) fn extract_anchor_error_name(logs: &[FrameLog]) -> Option<String> {
    for entry in logs {
        let FrameLog::Msg(text) = entry else { continue };
        if !text.starts_with("AnchorError") {
            continue;
        }
        let Some(after_code) = text.split_once("Error Code: ").map(|(_, s)| s) else {
            continue;
        };
        // The name terminates at the next `.` (Anchor's separator
        // between `Error Code: <Name>` and `Error Number: <N>`).
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

/// Decode an instruction's name from its data discriminator, given the
/// invoking program's base58 ID. Returns `None` for programs we don't have
/// a table for (e.g. user programs), or for data shapes we don't recognize.
///
/// The intent is to make CPI trees readable without consulting an external
/// program registry: a tree with two `Token` frames at the same depth becomes
/// `Token::TransferChecked` vs `Token::MintTo`, the difference visible at a
/// glance. The top-level header in `logs_structured_string` reuses this same
/// table so its `Instruction: ...` line stays consistent with the tree's
/// `Program::Name` rendering for inner frames.
pub(super) fn decode_instruction(program_id: &str, data: &[u8]) -> Option<&'static str> {
    match program_id {
        // SPL Token (legacy) and Token-2022 share their first 25 instruction
        // discriminators, so the same decoder serves both.
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
        | "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb" => spl_token_instruction_name(data),
        "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL" => spl_ata_instruction_name(data),
        "11111111111111111111111111111111" => system_instruction_name(data),
        _ => None,
    }
}

/// SPL Token uses a 1-byte discriminator. The variants here cover the
/// stable instructions from `spl_token::instruction::TokenInstruction` and
/// the corresponding additions in Token-2022's prefix.
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

/// AssociatedToken's `Create` instruction has historically had empty data
/// (pre-1.1) or a single discriminator byte (1.1+). Both shapes resolve to
/// the same name here.
fn spl_ata_instruction_name(data: &[u8]) -> Option<&'static str> {
    Some(match data.first().copied() {
        None | Some(0) => "Create",
        Some(1) => "CreateIdempotent",
        Some(2) => "RecoverNested",
        _ => return None,
    })
}

/// System program serializes its instruction enum with a 4-byte u32 LE tag
/// (the bincode/Borsh enum variant index), unlike SPL Token's 1-byte form.
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
