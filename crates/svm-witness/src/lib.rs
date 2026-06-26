//! The neutral SVM execution witness, as DATA. For every executed instruction,
//! top-level or CPI: which accounts the frame presented as signers and
//! writables, and who owned each account after execution. This is the
//! version-free contract every backend produces and the renderers consume.
//!
//! The types live here (default build, `solana-pubkey` only). The `extract`
//! feature adds the walk that produces them from a `TransactionContext`.

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
        // O(1) membership for both the tx-signer exclusion and the dedup; the
        // result stays in first-encounter order via `found`.
        let tx_signers: std::collections::HashSet<Pubkey> = tx_signers.iter().copied().collect();
        let mut seen = std::collections::HashSet::new();
        let mut found = Vec::new();
        for frame in &self.0 {
            for acc in &frame.accounts {
                if acc.is_signer && !tx_signers.contains(&acc.pubkey) && seen.insert(acc.pubkey) {
                    found.push(acc.pubkey);
                }
            }
        }
        found
    }
}

#[cfg(feature = "extract")]
use solana_account::ReadableAccount;
#[cfg(feature = "extract")]
use solana_transaction_context::TransactionContext;

/// Walk a completed `TransactionContext` into an [`InstructionTrace`], the one
/// correct extraction: the high-level instruction-context API resolves every
/// account against the full transaction context, so no caller-supplied key
/// table can be wrong. Owner is read from post-execution account state in the
/// same pass.
#[cfg(feature = "extract")]
pub fn extract(transaction_context: &TransactionContext) -> InstructionTrace {
    let mut frames = Vec::new();
    for index_in_trace in 0..transaction_context.get_instruction_trace_length() {
        let Ok(ictx) =
            transaction_context.get_instruction_context_at_index_in_trace(index_in_trace)
        else {
            // Trace indices come from the length query above; a miss here would
            // be a runtime invariant violation, not a caller error.
            continue;
        };
        let Ok(program_id) = ictx.get_program_key() else {
            continue;
        };

        let n_accounts = ictx.get_number_of_instruction_accounts();
        let mut accounts = Vec::with_capacity(usize::from(n_accounts));
        for i in 0..n_accounts {
            let (Ok(index_in_tx), Ok(is_signer), Ok(is_writable)) = (
                ictx.get_index_of_instruction_account_in_transaction(i),
                ictx.is_instruction_account_signer(i),
                ictx.is_instruction_account_writable(i),
            ) else {
                continue;
            };
            let Ok(pubkey) = transaction_context.get_key_of_account_at_index(index_in_tx) else {
                continue;
            };
            // The owner is read straight from account state (free, already in
            // the context); a borrow failure would mean the runtime handed us
            // an out-of-range index, so default to the system program rather
            // than dropping the account.
            let owner = transaction_context
                .accounts()
                .try_borrow(index_in_tx)
                .map(|acc| *acc.owner())
                .unwrap_or_default();
            accounts.push(TracedAccount {
                pubkey: *pubkey,
                is_signer,
                is_writable,
                owner,
            });
        }

        frames.push(TracedInstruction {
            program_id: *program_id,
            stack_height: ictx.get_stack_height(),
            accounts,
            data: ictx.get_instruction_data().to_vec(),
        });
    }
    InstructionTrace(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acct(pubkey: Pubkey, is_signer: bool) -> TracedAccount {
        TracedAccount {
            pubkey,
            is_signer,
            is_writable: true,
            owner: Pubkey::default(),
        }
    }

    #[test]
    fn program_signed_accounts_finds_invoke_signed_pdas() {
        // A PDA that signs inside a CPI but is not a transaction-level signer
        // was signed for by the program via `invoke_signed`. The payer is a tx
        // signer (extended into the CPI), so it is not reported.
        let payer = Pubkey::new_unique();
        let pda = Pubkey::new_unique();
        let passive = Pubkey::new_unique();

        let trace = InstructionTrace(vec![
            TracedInstruction {
                program_id: Pubkey::new_unique(),
                stack_height: 1,
                accounts: vec![acct(payer, true), acct(passive, false)],
                data: vec![],
            },
            TracedInstruction {
                program_id: Pubkey::new_unique(),
                stack_height: 2,
                // The PDA signs here (invoke_signed); the payer's tx signature
                // is extended in too.
                accounts: vec![acct(pda, true), acct(payer, true)],
                data: vec![],
            },
        ]);

        assert_eq!(
            trace.program_signed_accounts(&[payer]),
            vec![pda],
            "only the program-signed PDA, not the extended tx signer",
        );
    }

    #[test]
    fn program_signed_accounts_dedupes_and_skips_passive_accounts() {
        // The same PDA signing in two frames is reported once; an account that
        // never signs is never reported.
        let pda = Pubkey::new_unique();
        let passive = Pubkey::new_unique();
        let trace = InstructionTrace(vec![
            TracedInstruction {
                program_id: Pubkey::new_unique(),
                stack_height: 2,
                accounts: vec![acct(pda, true), acct(passive, false)],
                data: vec![],
            },
            TracedInstruction {
                program_id: Pubkey::new_unique(),
                stack_height: 2,
                accounts: vec![acct(pda, true)],
                data: vec![],
            },
        ]);

        assert_eq!(
            trace.program_signed_accounts(&[]),
            vec![pda],
            "a PDA signing across frames is reported once; passives are skipped",
        );
    }

    #[test]
    fn program_signed_accounts_empty_when_only_tx_signers_sign() {
        // A transaction whose only signers are tx-level (a plain human-signed
        // transfer) has no program-signed authority to report.
        let payer = Pubkey::new_unique();
        let recipient = Pubkey::new_unique();
        let trace = InstructionTrace(vec![TracedInstruction {
            program_id: Pubkey::new_unique(),
            stack_height: 1,
            accounts: vec![acct(payer, true), acct(recipient, false)],
            data: vec![],
        }]);
        assert!(trace.program_signed_accounts(&[payer]).is_empty());
    }
}
