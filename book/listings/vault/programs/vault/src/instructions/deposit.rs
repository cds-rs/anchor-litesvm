use crate::state::VaultState;
use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

// `BundledPubkeys`, host-only (see close.rs for the full mechanism). Emits
// `From<VaultAccs> for accounts::Deposit` and `BuildableIx<VaultAccs> for
// instruction::Deposit`, so `build_ix(bundle, instruction::Deposit { amount })`
// type-checks. The bundle covers `user`/`vault`/`vault_state`; `system_program`
// is auto-injected from its field type.
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(crate::test_helpers::VaultAccs)
)]
#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"vault", vault_state.key().as_ref()],
        bump = vault_state.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    #[account(
        seeds = [b"state", user.key().as_ref()],
        bump = vault_state.state_bump,
    )]
    pub vault_state: Account<'info, VaultState>,

    pub system_program: Program<'info, System>,
}

impl<'info> Deposit<'info> {
    pub fn deposit(&mut self, amount: u64) -> Result<()> {
        let cpi_accounts = Transfer {
            from: self.user.to_account_info(),
            to: self.vault.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(System::id(), cpi_accounts);
        transfer(cpi_ctx, amount)
    }
}
