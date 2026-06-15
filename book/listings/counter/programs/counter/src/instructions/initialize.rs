use crate::state::Counter;
use anchor_lang::prelude::*;

// ANCHOR: accounts
// src/instructions/initialize.rs
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(crate::test_helpers::InitializeBundle)
)]
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + Counter::INIT_SPACE,
        seeds = [b"counter", payer.key().as_ref()],
        bump,
    )]
    pub counter: Account<'info, Counter>,
    pub system_program: Program<'info, System>,
}
// ANCHOR_END: accounts

// ANCHOR: handler
// src/instructions/initialize.rs
impl Initialize<'_> {
    pub fn initialize(&mut self, start: u64) -> Result<()> {
        self.counter.count = start;
        Ok(())
    }
}
// ANCHOR_END: handler
