//! Record the SVM's instruction trace, with per-frame privilege flags.
//!
//! This is the data the CPI tree (logs) structurally cannot carry: for every
//! executed instruction, top-level or CPI, *which accounts the frame
//! presented as signers and writables*. The flags are what the runtime's
//! privilege check consumed; recording them is what makes authority flow
//! ("the program signed this CPI **as** PDA X") renderable from execution
//! instead of hand-drawn. See docs/design/cpi-rendering.md and
//! docs/design/litesvm-boundary.md.
//!
//! Mechanism: litesvm's `invocation-inspect-callback` feature fires
//! [`InvocationInspectCallback::after_invocation`] right after message
//! processing, while the `InvokeContext` still borrows the completed
//! `TransactionContext`. [`TraceRecorder`] walks the trace there and stashes
//! it; the [`TraceHandle`] returned by [`TraceRecorder::install`] reads it
//! back out. The callback has no return channel (it takes `&self`), hence
//! the shared-handle shape rather than a field on `TransactionMetadata`.

use litesvm::{InvocationInspectCallback, LiteSVM};
use solana_program_runtime::invoke_context::InvokeContext;
use solana_transaction::sanitized::SanitizedTransaction;
use solana_transaction_context::{IndexOfAccount, TransactionContext};
use std::sync::{Arc, Mutex};

// The trace DATA types live in the vocabulary crate (any engine can fill
// them); recording them is litesvm-specific and stays below.
pub use testsvm::trace::{InstructionTrace, TracedAccount, TracedInstruction};

/// Walk a completed `TransactionContext` into an [`InstructionTrace`].
///
/// This is the privilege-preserving sibling of litesvm's own
/// `inner_instructions_list_from_instruction_trace`, which compiles each
/// frame down to a `CompiledInstruction` and loses the flags at that step.
fn extract_trace(transaction_context: &TransactionContext) -> InstructionTrace {
    svm_witness::extract(transaction_context)
}

/// Cloneable reader for traces recorded by a [`TraceRecorder`].
#[derive(Clone, Default)]
pub struct TraceHandle {
    latest: Arc<Mutex<Option<InstructionTrace>>>,
}

impl TraceHandle {
    /// Take the most recently recorded trace, leaving `None` behind.
    ///
    /// "Most recent" means the last transaction the SVM executed, and
    /// *every* transaction records one: airdrops and token-setup helpers
    /// included. Take the trace immediately after the send you care about,
    /// not at the end of a scenario.
    pub fn take_latest(&self) -> Option<InstructionTrace> {
        self.latest.lock().expect("trace mutex poisoned").take()
    }
}

/// The [`InvocationInspectCallback`] that records instruction traces.
///
/// Install via [`TraceRecorder::install`]; the SVM owns the recorder, the
/// returned [`TraceHandle`] is the reader.
pub struct TraceRecorder {
    handle: TraceHandle,
}

impl TraceRecorder {
    /// Install a recorder on `svm` and return the handle that reads what
    /// it records.
    ///
    /// Replaces any previously installed `InvocationInspectCallback` (the
    /// SVM holds exactly one).
    pub fn install(svm: &mut LiteSVM) -> TraceHandle {
        let handle = TraceHandle::default();
        svm.set_invocation_inspect_callback(TraceRecorder {
            handle: handle.clone(),
        });
        handle
    }
}

impl InvocationInspectCallback for TraceRecorder {
    fn before_invocation(
        &self,
        _: &LiteSVM,
        _: &SanitizedTransaction,
        _: &[IndexOfAccount],
        _: &InvokeContext,
    ) {
    }

    fn after_invocation(&self, _: &LiteSVM, invoke_context: &InvokeContext, _: bool) {
        // Fires for failed transactions too: the trace holds every frame
        // that started, including the one that failed. That is required for
        // the authority view's rejected-vs-accepted contrast.
        let trace = extract_trace(&invoke_context.transaction_context);
        *self.handle.latest.lock().expect("trace mutex poisoned") = Some(trace);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_keypair::Keypair;
    use solana_program::pubkey::Pubkey;
    use solana_signer::Signer;
    use solana_transaction::Transaction;

    const SYSTEM_PROGRAM_ID: Pubkey =
        solana_program::pubkey::Pubkey::from_str_const("11111111111111111111111111111111");

    fn transfer_tx(svm: &LiteSVM, from: &Keypair, to: &Pubkey, lamports: u64) -> Transaction {
        let ix = solana_system_interface::instruction::transfer(&from.pubkey(), to, lamports);
        Transaction::new_signed_with_payer(
            &[ix],
            Some(&from.pubkey()),
            &[from],
            svm.latest_blockhash(),
        )
    }

    #[test]
    fn records_top_level_frame_with_privilege_flags() {
        let mut svm = LiteSVM::new();
        let handle = TraceRecorder::install(&mut svm);

        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();

        let recipient = Pubkey::new_unique();
        let tx = transfer_tx(&svm, &payer, &recipient, 1_000_000);
        svm.send_transaction(tx).unwrap();

        let trace = handle.take_latest().expect("trace recorded");
        assert_eq!(trace.0.len(), 1, "one top-level frame, no CPIs");

        let frame = &trace.0[0];
        assert_eq!(frame.program_id, SYSTEM_PROGRAM_ID);
        assert_eq!(frame.stack_height, 1);

        // System transfer metas: [from (signer, writable), to (writable)].
        assert_eq!(frame.accounts.len(), 2);
        assert_eq!(frame.accounts[0].pubkey, payer.pubkey());
        assert!(frame.accounts[0].is_signer);
        assert!(frame.accounts[0].is_writable);
        assert_eq!(frame.accounts[1].pubkey, recipient);
        assert!(!frame.accounts[1].is_signer);
        assert!(frame.accounts[1].is_writable);

        // No invoke_signed anywhere in a plain transfer.
        assert!(trace.program_signed_accounts(&[payer.pubkey()]).is_empty());
    }

    #[test]
    fn take_latest_drains() {
        let mut svm = LiteSVM::new();
        let handle = TraceRecorder::install(&mut svm);

        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();

        // The airdrop is itself a transaction, so a trace is already stashed.
        assert!(handle.take_latest().is_some());
        assert!(handle.take_latest().is_none(), "second take finds nothing");
    }

    #[test]
    fn failed_transaction_still_records_a_trace() {
        let mut svm = LiteSVM::new();
        let handle = TraceRecorder::install(&mut svm);

        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
        handle.take_latest();

        // Transfer more than the balance: executes and fails inside the SVM.
        let recipient = Pubkey::new_unique();
        let tx = transfer_tx(&svm, &payer, &recipient, u64::MAX / 2);
        assert!(svm.send_transaction(tx).is_err());

        let trace = handle
            .take_latest()
            .expect("failed transactions record their trace too");
        assert_eq!(trace.0.len(), 1);
        assert_eq!(trace.0[0].program_id, SYSTEM_PROGRAM_ID);
    }

    #[test]
    fn program_signed_accounts_detection_rule() {
        // Pure unit test of the rule on hand-built traces: a CPI frame
        // presents a PDA as signer; the PDA is not a tx-level signer.
        let human = Pubkey::new_unique();
        let pda = Pubkey::new_unique();
        let target = Pubkey::new_unique();

        // owner is irrelevant to the signer-detection rule; any value works.
        let acct = |pubkey, is_signer, is_writable| TracedAccount {
            pubkey,
            is_signer,
            is_writable,
            owner: Pubkey::default(),
        };
        let trace = InstructionTrace(vec![
            TracedInstruction {
                program_id: Pubkey::new_unique(),
                stack_height: 1,
                accounts: vec![acct(human, true, true), acct(pda, false, true)],
                data: vec![],
            },
            TracedInstruction {
                program_id: Pubkey::new_unique(),
                stack_height: 2,
                // The invoke_signed: the PDA is a signer at CPI level only.
                accounts: vec![acct(pda, true, false), acct(target, false, true)],
                data: vec![],
            },
        ]);

        assert_eq!(trace.program_signed_accounts(&[human]), vec![pda]);
        // The human's own signature extending into a CPI is not invoke_signed.
        assert!(InstructionTrace(vec![trace.0[0].clone()])
            .program_signed_accounts(&[human])
            .is_empty());
    }
}
