//! Host-only pubkey bundles that drive the program's instructions in tests.

use anchor_lang::prelude::Pubkey;
use anchor_litesvm::Bundle;

// ANCHOR: bundle
// src/test_helpers.rs
#[derive(Copy, Clone, Debug, Bundle)]
pub struct InitializeBundle {
    pub payer: Pubkey,
    pub counter: Pubkey,
}

#[derive(Copy, Clone, Debug, Bundle)]
pub struct IncrementBundle {
    pub counter: Pubkey,
    pub payer: Pubkey,
}
// ANCHOR_END: bundle

// ANCHOR: donatebundle
// src/test_helpers.rs
#[derive(Copy, Clone, Debug, Bundle)]
pub struct DonateBundle {
    pub donor: Pubkey,
    pub mint: Pubkey,
    pub donor_ata: Pubkey,
    pub recipient_ata: Pubkey,
}
// ANCHOR_END: donatebundle
