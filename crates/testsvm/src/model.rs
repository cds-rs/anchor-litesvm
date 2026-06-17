//! The unified transaction model: what one execution yielded, on any engine.
//!
//! Adapter-produced; the consumer names and renders it. The code-level name
//! is module-qualified (`model::Transaction`) to avoid colliding with
//! `solana_transaction::Transaction`.

use {
    crate::{aliases::Aliases, frame::Frame, trace::InstructionTrace},
    solana_message::Message,
    solana_pubkey::Pubkey,
};

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
        aliases: Aliases,
        events: crate::events::EventRegistry,
    ) -> Self {
        name_top_level_frames(&mut frames, &message, instruction_names);
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
        let label = |pk: &solana_pubkey::Pubkey| self.aliases.label(pk);
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
        crate::frame::format_cpi_tree_with_events(&header, &self.frames, &label, &self.events)
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
}
