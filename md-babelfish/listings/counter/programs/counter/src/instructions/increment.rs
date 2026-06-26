use crate::state::Counter;
use anchor_lang::prelude::*;

// ANCHOR: increment
// src/instructions/increment.rs
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(crate::test_helpers::IncrementBundle)
)]
#[derive(Accounts)]
pub struct Increment<'info> {
    #[account(
        mut,
        seeds = [b"counter", payer.key().as_ref()],
        bump,
    )]
    pub counter: Account<'info, Counter>,
    pub payer: Signer<'info>,
}

impl Increment<'_> {
    pub fn increment(&mut self) -> Result<()> {
        self.counter.count += 1;
        Ok(())
    }
}
// ANCHOR_END: increment
