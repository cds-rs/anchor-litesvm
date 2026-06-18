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
