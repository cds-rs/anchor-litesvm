//! Bundle struct used by tests to drive the vault program through
//! [`anchor_litesvm::Program::build_ix`]. Each `#[derive(Accounts)]` struct
//! in this crate derives `BundledPubkeys` against `VaultAccs`, so the
//! `From<VaultAccs> for accounts::*` and `BuildableIx<VaultAccs> for
//! instruction::*` impls are generated next to the account definitions.
//!
//! Host-only (the module is `#[cfg(not(target_os = "solana"))]` in lib.rs)
//! so this never ships in the BPF binary.

use anchor_lang::prelude::Pubkey;
use anchor_litesvm::Bundle;

// `Bundle` emits a `Default` impl that seeds every field with a fresh
// `Pubkey::new_unique()`. That lets a test bind only the keys it cares about
// and let the rest fall to throwaway placeholders:
//
//     let accs = VaultAccs { user: user.pubkey(), ..VaultAccs::default() };
//
// It pairs with the `BundledPubkeys` projections on the `#[derive(Accounts)]`
// structs (see the instruction modules): `Default` builds the bundle, those
// `From<VaultAccs>` impls project it into each `accounts::*` struct.
// ANCHOR: bundle
// src/test_helpers.rs
#[derive(Copy, Clone, Debug, Bundle)]
pub struct VaultAccs {
    pub user: Pubkey,
    pub vault: Pubkey,
    pub vault_state: Pubkey,
}
// ANCHOR_END: bundle
