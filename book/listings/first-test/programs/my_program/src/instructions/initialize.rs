use crate::state::Data;
use anchor_lang::prelude::*;

// ANCHOR: accounts
// src/instructions/initialize.rs
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(crate::test_helpers::InitAccs)
)]
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// Signs the transaction and pays the new account's rent.
    #[account(mut)]
    pub user_account: Signer<'info>,
    #[account(
        init,
        payer = user_account,
        space = 8 + Data::INIT_SPACE,
        seeds = [b"data", user_account.key().as_ref()],
        bump,
    )]
    pub data: Account<'info, Data>,
    pub system_program: Program<'info, System>,
}
// ANCHOR_END: accounts

// ANCHOR: handler
// src/instructions/initialize.rs
impl Initialize<'_> {
    pub fn initialize(&mut self, value: u64) -> Result<()> {
        self.data.value = value;
        Ok(())
    }
}
// ANCHOR_END: handler
