use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

// ANCHOR: accounts
// src/instructions/donate.rs
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(crate::test_helpers::DonateBundle)
)]
#[derive(Accounts)]
pub struct Donate<'info> {
    pub donor: Signer<'info>,
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(mut)]
    pub donor_ata: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub recipient_ata: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
}
// ANCHOR_END: accounts

// ANCHOR: handler
// src/instructions/donate.rs
impl Donate<'_> {
    pub fn donate(&self, amount: u64) -> Result<()> {
        let cpi_accounts = TransferChecked {
            from: self.donor_ata.to_account_info(),
            mint: self.mint.to_account_info(),
            to: self.recipient_ata.to_account_info(),
            authority: self.donor.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(self.token_program.key(), cpi_accounts);
        transfer_checked(cpi_ctx, amount, self.mint.decimals)
    }
}
// ANCHOR_END: handler
