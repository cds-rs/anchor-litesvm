//! The unified transaction model: what one execution yielded, on any engine.
//!
//! Adapter-produced; the consumer names and renders it. The code-level name
//! is module-qualified (`model::Transaction`) to avoid colliding with
//! `solana_transaction::Transaction`.

use {
    crate::{
        aliases::Aliases,
        frame::{Frame, FrameLog},
        trace::InstructionTrace,
    },
    solana_message::Message,
    solana_pubkey::Pubkey,
};

/// Producer-side resolution of a failed frame's display name from the signal a
/// frame carries: its `logs` (where an Anchor program writes its
/// `Error Code: <Name>` line) and the runtime's `raw` message. The provided
/// method resolves the Anchor line (plus the offending account when Anchor
/// names one), which is log-sourced and so identical on every engine; that
/// universality is why it's a default rather than per-backend boilerplate.
///
/// Each backend implements this trait. The default fits every engine whose
/// frame failures reach us as the runtime's own logs (litesvm, mollusk,
/// quasar); a backend whose engine expresses failures differently (an RPC that
/// only hands back a `TransactionError` string, say) overrides
/// [`resolve_frame_failure`](Self::resolve_frame_failure) to normalize its own
/// surface. The [`ErrorNames`](crate::errors::ErrorNames) registry is a
/// separate, engine-neutral tier consulted *after* this one (see
/// [`assemble`](Transaction::assemble)), so a bare-code Pinocchio failure still
/// resolves by name without a resolver override.
pub trait FailureResolver {
    /// Resolve `logs` + the `raw` runtime message into a frame's display name,
    /// or `None` to fall through to the registry tier and then the raw message.
    /// `raw` is the runtime's message for the frame (the default ignores it; an
    /// override that normalizes engine-specific error text reads it).
    fn resolve_frame_failure(&self, logs: &[FrameLog], raw: Option<&str>) -> Option<String> {
        let _ = raw;
        resolve_anchor_failure(logs)
    }
}

/// The default resolver, for the assembly call sites that have no backend in
/// hand (the litesvm `TransactionResult` re-render path and the model unit
/// tests). Carries only the provided Anchor decode.
pub struct AnchorFailures;
impl FailureResolver for AnchorFailures {}

/// What an engine witnessed about one transaction.
#[derive(Debug, Clone)]
pub struct Transaction {
    /// The nested CPI structure: program, name slot, CU, outcome per frame.
    /// Filled by the adapter (the engine's native structure converted, or
    /// the vendored log parse); renderers consume this, never log text.
    pub frames: Vec<Frame>,
    /// The account list frame indices resolve against (never ship indices
    /// without their frame).
    pub account_keys: Vec<Pubkey>,
    /// Raw log lines: the floor on every engine, and the raw evidence even
    /// when `frames` came in structured.
    pub logs: Vec<String>,
    /// Transaction-level failure message, if the tx carried a program error.
    pub error: Option<String>,
    pub compute_units: u64,
    /// `None` where the engine does not model fees. Absent, not zero.
    pub fee: Option<u64>,
    /// The transaction message, for resolving top-level signer/writable facts.
    pub message: Message,
    /// Per-frame privilege trace. `Some` where the engine witnessed it.
    pub trace: Option<InstructionTrace>,
    pub return_data: Option<Vec<u8>>,
    /// The naming vocabulary in effect when the backend sent this: the
    /// backend owns the table (seeded with the well-known programs,
    /// extended via `TestSVM::register_alias`) and stamps every send, so
    /// scenarios never thread an alias table by hand.
    pub aliases: Aliases,
    /// The instruction-name table in effect when the backend sent this. Carried
    /// (like `aliases`) so the rich renderers reached via `From<Transaction>`
    /// resolve names from the backend's registry without the scenario
    /// re-attaching it.
    pub instruction_names: crate::instructions::InstructionNames,
    /// The error-name table in effect when the backend sent this.
    pub error_names: crate::errors::ErrorNames,
    /// The event-decode registry in effect when the backend sent this, so a
    /// `TestSVM::register_event_decoder` / `register_cpi_event` on the backend
    /// reaches the rendered events with no per-result attachment.
    pub events: crate::events::EventRegistry,
}

impl Transaction {
    /// Assemble a named transaction record from an adapter's raw extraction:
    /// resolve top-level and failed frame names from the program tables, then
    /// build the struct. The shared tail of every [`TestSVM::send`](crate::TestSVM::send):
    /// the adapter extracts `frames` from its engine's native structure (or the
    /// vendored log parser) and supplies the outcome fields; this owns the
    /// naming and assembly, so a change to either (CPI-frame naming, a new
    /// field) touches one place rather than every adapter.
    #[allow(clippy::too_many_arguments)]
    pub fn assemble(
        mut frames: Vec<Frame>,
        message: Message,
        logs: Vec<String>,
        error: Option<String>,
        compute_units: u64,
        fee: Option<u64>,
        trace: Option<InstructionTrace>,
        return_data: Option<Vec<u8>>,
        instruction_names: &crate::instructions::InstructionNames,
        error_names: &crate::errors::ErrorNames,
        failure_resolver: &dyn FailureResolver,
        aliases: Aliases,
        events: crate::events::EventRegistry,
    ) -> Self {
        name_top_level_frames(&mut frames, &message, instruction_names);
        // Failure naming runs in two tiers, best name first: the backend's
        // resolver (the Anchor `Error Code:` line by default) rewrites the
        // message to a name, then the registry tier resolves any frame still
        // carrying a bare `custom program error: 0x<code>`. The order is what
        // gives Anchor precedence over the registry; a frame the resolver named
        // no longer matches the custom-code shape, so the registry skips it.
        resolve_failed_frames(&mut frames, failure_resolver);
        name_failed_frames(&mut frames, error_names);
        Self {
            account_keys: message.account_keys.clone(),
            frames,
            logs,
            error,
            compute_units,
            fee,
            message,
            trace,
            return_data,
            aliases,
            instruction_names: instruction_names.clone(),
            error_names: error_names.clone(),
            events,
        }
    }

    /// The structured CPI tree, rendered. Works on every engine because it
    /// draws from `frames`, never from an engine type: this is the
    /// vocabulary's own renderer (the richer aliased renderers live with the
    /// litesvm adapter, on `TransactionResult`).
    pub fn pretty_cpi_tree(&self) -> String {
        use crate::frame::{transaction_compute_budget, transaction_total_cu, with_commas};
        // Same header litesvm's pretty_cpi_tree builds: transaction-total BPF
        // CU and the budget, or an explicit no-data note. Never "0 CU":
        // native programs don't emit `consumed` lines, and reporting that
        // absence as zero would misstate the cost.
        let header = match (
            transaction_total_cu(&self.frames),
            transaction_compute_budget(&self.frames),
        ) {
            (Some(total), Some(budget)) => format!(
                "CPI Tree ({} BPF CU / {} budget):",
                with_commas(total),
                with_commas(budget)
            ),
            _ => "CPI Tree (no compute units in logs):".to_string(),
        };
        crate::frame::format_cpi_tree_with_events(
            &header,
            &self.frames,
            &self.aliases,
            &self.events,
        )
    }

    /// Every failed frame's resolved message, in DFS pre-order. The resolution
    /// (the Anchor `Error Code:` name, or a registered error name) already
    /// happened in [`assemble`](Self::assemble), so this carries the *named*
    /// failure even though only the raw `0x<code>` appears in the logs; an
    /// error-name assertion matches against it.
    pub fn failure_messages(&self) -> Vec<String> {
        fn walk(frames: &[Frame], out: &mut Vec<String>) {
            for frame in frames {
                if let crate::frame::Outcome::Failed {
                    message: Some(message),
                } = &frame.outcome
                {
                    out.push(message.clone());
                }
                walk(&frame.children, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.frames, &mut out);
        out
    }
}

/// Producer-side error naming: walk every frame (failures live at any depth)
/// and rewrite `custom program error: 0x<code>` messages through the
/// registry, keyed by the failing frame's own program: `InvalidAmount (0x7)`
/// instead of the bare code. Adapters call this after building frames.
pub fn name_failed_frames(frames: &mut [crate::frame::Frame], errors: &crate::errors::ErrorNames) {
    if errors.is_empty() {
        return;
    }
    for frame in frames {
        if let crate::frame::Outcome::Failed {
            message: Some(message),
        } = &mut frame.outcome
        {
            if let Some(idx) = message.find("custom program error: 0x") {
                let hex = &message[idx + "custom program error: 0x".len()..];
                let hex: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                if let Ok(code) = u32::from_str_radix(&hex, 16) {
                    if let Some(name) = errors.resolve(&frame.program_id.to_string(), code) {
                        *message = format!("{name} (0x{code:x})");
                    }
                }
            }
        }
        name_failed_frames(&mut frame.children, errors);
    }
}

/// Producer-side failure naming, resolver tier: walk every frame and let
/// `resolver` rewrite a failed frame's message from the signal it carries
/// (its logs, by default the Anchor `Error Code:` line). Runs ahead of
/// [`name_failed_frames`]; a frame named here no longer matches the bare
/// `custom program error: 0x<code>` shape, so the registry tier leaves it
/// alone, which is how Anchor names take precedence over the registry.
pub fn resolve_failed_frames(frames: &mut [Frame], resolver: &dyn FailureResolver) {
    for frame in frames {
        if let crate::frame::Outcome::Failed { message } = &mut frame.outcome {
            if let Some(name) = resolver.resolve_frame_failure(&frame.logs, message.as_deref()) {
                *message = Some(name);
            }
        }
        resolve_failed_frames(&mut frame.children, resolver);
    }
}

/// Lift the friendly error name out of an Anchor-thrown error log when present,
/// plus the offending account when Anchor names one: a constraint failure
/// renders as `AccountNotSigner on authority` (the extra entropy a failed frame
/// can carry without cluttering the happy path), a `require!` failure stays just
/// `EscrowExpired`. `None` for non-Anchor failures, so the caller falls back to
/// the registry tier and then the runtime message. This is the default
/// [`FailureResolver`] body, log-sourced and so identical on every engine.
pub fn resolve_anchor_failure(logs: &[FrameLog]) -> Option<String> {
    let name = extract_anchor_error_name(logs)?;
    match extract_anchor_error_account(logs) {
        Some(account) => Some(format!("{name} on {account}")),
        None => Some(name),
    }
}

/// Anchor's `#[error_code]` macro emits a structured log line on failure:
///
/// ```text
/// AnchorError thrown in <file>:<line>. Error Code: <Name>. Error Number: 6000. Error Message: <Name>.
/// ```
///
/// (Other variants: `AnchorError caused by account: ...`, `AnchorError
/// occurred. ...`. All carry the `Error Code: <Name>.` segment.) Returns `None`
/// for non-Anchor failures or a missing/malformed line.
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

/// Producer-side naming: fill each top-level frame's `instruction_name` from
/// the registry, correlating frame order with the message's instruction order
/// (top-level frames are emitted in execution order, which is message order).
/// Adapters call this after building frames; names already present (an engine
/// or log-sourced name) are never overwritten.
pub fn name_top_level_frames(
    frames: &mut [Frame],
    message: &Message,
    names: &crate::instructions::InstructionNames,
) {
    if names.is_empty() {
        return;
    }
    for (frame, ix) in frames.iter_mut().zip(message.instructions.iter()) {
        if frame.instruction_name.is_some() {
            continue;
        }
        let Some(program_id) = message.account_keys.get(ix.program_id_index as usize) else {
            continue;
        };
        if let Some(name) = names.resolve(&program_id.to_string(), &ix.data) {
            frame.instruction_name = Some(name.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_carries_structured_frames_and_their_frame() {
        let logs = vec![
            "Program 11111111111111111111111111111111 invoke [1]".to_string(),
            "Program 11111111111111111111111111111111 success".to_string(),
        ];
        let tx = Transaction {
            frames: crate::frame::frames_from_logs(&logs),
            account_keys: vec![Pubkey::new_unique()],
            logs,
            error: None,
            compute_units: 150,
            fee: None,
            message: Message::default(),
            trace: None,
            return_data: None,
            aliases: Aliases::with_well_known(),
            instruction_names: Default::default(),
            error_names: Default::default(),
            events: Default::default(),
        };
        assert_eq!(tx.frames.len(), 1, "one top-level frame parsed");
        assert!(
            tx.fee.is_none(),
            "an engine that does not model fees says so"
        );

        let rendered = tx.pretty_cpi_tree();
        assert!(
            rendered.contains("System"),
            "the model renders its own frames through the alias table:\n{rendered}"
        );
    }

    #[test]
    fn failed_frames_resolve_through_the_error_registry() {
        let program = Pubkey::new_unique();
        let mut errors = crate::errors::ErrorNames::new();
        errors.register(program, 7, "InvalidAmount");

        let mut frames = vec![crate::frame::Frame {
            program_id: program,
            outcome: crate::frame::Outcome::Failed {
                message: Some("custom program error: 0x7".to_string()),
            },
            compute_units: None,
            instruction_name: None,
            logs: vec![],
            children: vec![],
        }];
        name_failed_frames(&mut frames, &errors);
        assert_eq!(
            match &frames[0].outcome {
                crate::frame::Outcome::Failed { message } => message.as_deref(),
                _ => None,
            },
            Some("InvalidAmount (0x7)"),
        );
    }

    // ---- Anchor-error log extraction --------------------------------------
    // The producer-side resolution every engine shares, log-sourced so it is
    // identical on litesvm / mollusk / quasar.

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
        // Nested failures (a child fails AND the parent fails because of it)
        // can produce two AnchorError lines in one frame's logs. First-seen
        // wins; matches the typical "leaf error is the one to report".
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
        // label gain the offending account: the signal the transfer-hook
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

    // ---- assemble() naming seam -------------------------------------------
    // The central naming seam, exercised directly rather than via an adapter.

    /// Drive [`Transaction::assemble`] with the defaults a unit test wants:
    /// caller supplies frames + message + the name tables.
    fn assemble_with(
        frames: Vec<Frame>,
        message: Message,
        instruction_names: &crate::instructions::InstructionNames,
        error_names: &crate::errors::ErrorNames,
    ) -> Transaction {
        Transaction::assemble(
            frames,
            message,
            vec![],
            None,
            0,
            None,
            None,
            None,
            instruction_names,
            error_names,
            &AnchorFailures,
            Aliases::with_well_known(),
            Default::default(),
        )
    }

    #[test]
    fn assemble_names_a_top_level_frame_from_the_registry() {
        // A top-level frame with no name gets one from the instruction registry,
        // correlated by message-instruction order.
        use solana_message::compiled_instruction::CompiledInstruction;
        let program = Pubkey::new_unique();
        let mut names = crate::instructions::InstructionNames::new();
        names.register(program, vec![5, 0, 0, 0], "DoTheThing");

        let message = Message {
            account_keys: vec![program],
            instructions: vec![CompiledInstruction {
                program_id_index: 0,
                accounts: vec![],
                data: vec![5, 0, 0, 0],
            }],
            ..Default::default()
        };
        let frames = vec![Frame {
            program_id: program,
            outcome: crate::frame::Outcome::Success,
            compute_units: None,
            instruction_name: None,
            logs: vec![],
            children: vec![],
        }];

        let tx = assemble_with(frames, message, &names, &Default::default());
        assert_eq!(
            tx.frames[0].instruction_name.as_deref(),
            Some("DoTheThing"),
            "the top-level frame is named from the registry by ix order",
        );
    }

    #[test]
    fn assemble_resolves_a_failed_frame_to_its_registered_error_name() {
        // A failed frame carrying a bare `custom program error: 0x<code>` is
        // resolved to the program's registered error name (the registry tier,
        // after the Anchor resolver tier finds nothing).
        use solana_message::compiled_instruction::CompiledInstruction;
        let program = Pubkey::new_unique();
        let mut errors = crate::errors::ErrorNames::new();
        errors.register(program, 7, "InvalidAmount");

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
            outcome: crate::frame::Outcome::Failed {
                message: Some("custom program error: 0x7".into()),
            },
            compute_units: None,
            instruction_name: None,
            logs: vec![],
            children: vec![],
        }];

        let tx = assemble_with(frames, message, &Default::default(), &errors);
        assert_eq!(
            tx.failure_messages(),
            vec!["InvalidAmount (0x7)".to_string()],
            "the failed frame resolves to its registered error name",
        );
    }
}
