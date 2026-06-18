//! Extract per-transaction signer info from a `Message`.
//!
//! `SignerInfo` is the small intermediate struct the structured-logs
//! printer consumes when rendering signer annotations. It preserves the
//! message's signer ordering throughout: the first entry is always the
//! fee payer.

use solana_message::Message;
use solana_pubkey::Pubkey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SignerInfo {
    /// The tx's required signers in `account_keys` order.
    pub(super) tx_signers: Vec<Pubkey>,
    /// One entry per top-level instruction in the original tx, matching
    /// the order of `message.instructions`. Each entry lists the
    /// tx-required signers whose pubkey index appears in that ix's
    /// `accounts` slice. Empty if no required signer is referenced.
    pub(super) per_root: Vec<Vec<Pubkey>>,
}

pub(super) fn extract(message: &Message) -> SignerInfo {
    let n_signers = message.header.num_required_signatures as usize;
    let tx_signers: Vec<Pubkey> = message.account_keys[..n_signers].to_vec();

    let per_root: Vec<Vec<Pubkey>> = message
        .instructions
        .iter()
        .map(|ix| {
            ix.accounts
                .iter()
                .filter_map(|&idx| {
                    let i = idx as usize;
                    if i < n_signers {
                        Some(message.account_keys[i])
                    } else {
                        None
                    }
                })
                .collect()
        })
        .collect();

    SignerInfo {
        tx_signers,
        per_root,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_message::compiled_instruction::CompiledInstruction;
    use solana_message::MessageHeader;

    fn make_message(
        n_signers: u8,
        account_keys: Vec<Pubkey>,
        instructions: Vec<CompiledInstruction>,
    ) -> Message {
        Message {
            header: MessageHeader {
                num_required_signatures: n_signers,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 0,
            },
            account_keys,
            recent_blockhash: Default::default(),
            instructions,
        }
    }

    #[test]
    fn extract_single_signer_single_ix() {
        let payer = Pubkey::new_unique();
        let program_id = Pubkey::new_unique();
        let msg = make_message(
            1,
            vec![payer, program_id],
            vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0],
                data: vec![],
            }],
        );
        let info = extract(&msg);
        assert_eq!(info.tx_signers, vec![payer]);
        assert_eq!(info.per_root, vec![vec![payer]]);
    }

    #[test]
    fn extract_multi_signer_multi_ix() {
        let alice = Pubkey::new_unique();
        let bob = Pubkey::new_unique();
        let program_id = Pubkey::new_unique();
        let msg = make_message(
            2,
            vec![alice, bob, program_id],
            vec![
                CompiledInstruction {
                    program_id_index: 2,
                    accounts: vec![0],
                    data: vec![],
                },
                CompiledInstruction {
                    program_id_index: 2,
                    accounts: vec![1],
                    data: vec![],
                },
            ],
        );
        let info = extract(&msg);
        assert_eq!(info.tx_signers, vec![alice, bob]);
        assert_eq!(info.per_root, vec![vec![alice], vec![bob]]);
    }

    #[test]
    fn extract_per_root_empty_when_no_required_signer_referenced() {
        let payer = Pubkey::new_unique();
        let other = Pubkey::new_unique();
        let program_id = Pubkey::new_unique();
        let msg = make_message(
            1,
            vec![payer, other, program_id],
            vec![CompiledInstruction {
                program_id_index: 2,
                accounts: vec![1],
                data: vec![],
            }],
        );
        let info = extract(&msg);
        assert_eq!(info.tx_signers, vec![payer]);
        assert_eq!(info.per_root, vec![vec![]]);
    }

    #[test]
    fn extract_preserves_message_signer_order() {
        let first = Pubkey::new_unique();
        let second = Pubkey::new_unique();
        let third = Pubkey::new_unique();
        let program_id = Pubkey::new_unique();
        let msg = make_message(
            3,
            vec![first, second, third, program_id],
            vec![CompiledInstruction {
                program_id_index: 3,
                accounts: vec![0, 1, 2],
                data: vec![],
            }],
        );
        let info = extract(&msg);
        assert_eq!(info.tx_signers, vec![first, second, third]);
        assert_eq!(info.per_root, vec![vec![first, second, third]]);
    }

    #[test]
    fn extract_fee_payer_appears_in_each_ix_that_references_it() {
        let payer = Pubkey::new_unique();
        let alice = Pubkey::new_unique();
        let program_id = Pubkey::new_unique();
        let msg = make_message(
            2,
            vec![payer, alice, program_id],
            vec![
                CompiledInstruction {
                    program_id_index: 2,
                    accounts: vec![0],
                    data: vec![],
                },
                CompiledInstruction {
                    program_id_index: 2,
                    accounts: vec![0, 1],
                    data: vec![],
                },
            ],
        );
        let info = extract(&msg);
        assert_eq!(info.tx_signers, vec![payer, alice]);
        assert_eq!(info.per_root, vec![vec![payer], vec![payer, alice]]);
    }
}
