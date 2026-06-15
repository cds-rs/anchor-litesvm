//! The per-frame privilege trace, as DATA. For every executed instruction,
//! top-level or CPI: which accounts the frame presented as signers and
//! writables, and who owned each account after execution. This is what the
//! authority renderer draws from.
//!
//! Only the data types live here; *recording* a trace is engine-specific by
//! nature (litesvm's inspect callback, mollusk's register tracing) and lives
//! in each engine's adapter crate.

use solana_pubkey::Pubkey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracedAccount {
    pub pubkey: Pubkey,
    /// The privilege this frame presented for the account. For a CPI frame
    /// this is the meta the *calling program* constructed; an account that
    /// is a signer here but not a transaction-level signer was signed for
    /// by that program (`invoke_signed`).
    pub is_signer: bool,
    pub is_writable: bool,
    /// The account's owning program (its `owner` field), read after
    /// execution. This is the runtime's mutation-permission fact, distinct
    /// from `is_writable` (which is only an access request): a data write
    /// requires `is_writable` *and* is performed by the owner. The authority
    /// renderer uses it to draw a write-arrow only from the frame whose
    /// program owns the target, which is the deliberate writer rather than a
    /// parent frame that merely requested write access.
    ///
    /// Read post-execution, so for an account created mid-transaction this is
    /// the assigned owner (after `CreateAccount`/`Assign`), not whatever it
    /// was before. That is what "who owns it now" should mean.
    pub owner: Pubkey,
}

/// One executed instruction from the SVM's trace: top-level or CPI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracedInstruction {
    pub program_id: Pubkey,
    /// 1 for transaction-level instructions, 2+ for CPIs (matches the
    /// bracket in the runtime's `invoke [n]` log lines).
    pub stack_height: usize,
    pub accounts: Vec<TracedAccount>,
    pub data: Vec<u8>,
}

/// The full instruction trace of one executed transaction, in execution
/// order (parents precede their CPIs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstructionTrace(pub Vec<TracedInstruction>);

impl InstructionTrace {
    /// The accounts some frame presented as a signer that are *not*
    /// transaction-level signers: these were signed for by a program via
    /// `invoke_signed`. This is the authority view's detection rule.
    ///
    /// `tx_signers` is the transaction message's required-signer set (e.g.
    /// `SignerInfo::tx_signers`, or `message.account_keys[..num_required_signatures]`).
    pub fn program_signed_accounts(&self, tx_signers: &[Pubkey]) -> Vec<Pubkey> {
        let mut found: Vec<Pubkey> = Vec::new();
        for frame in &self.0 {
            for acc in &frame.accounts {
                if acc.is_signer
                    && !tx_signers.contains(&acc.pubkey)
                    && !found.contains(&acc.pubkey)
                {
                    found.push(acc.pubkey);
                }
            }
        }
        found
    }
}
