//! Host-only pubkey bundles that drive the program's instructions in tests.

use anchor_lang::prelude::Pubkey;
use anchor_litesvm::Bundle;

// ANCHOR: bundle
// src/test_helpers.rs
#[derive(Copy, Clone, Debug, Bundle)]
pub struct InitAccs {
    pub user_account: Pubkey,
    pub data: Pubkey,
}
// ANCHOR_END: bundle
