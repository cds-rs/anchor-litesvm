use crate::state::VaultState;
use anchor_lang::prelude::*;

// `BundledPubkeys`, host-only (see close.rs for the full mechanism). Emits
// `From<VaultAccs> for accounts::Initialize` and `BuildableIx<VaultAccs> for
// instruction::Initialize`. The bundle covers `user`/`vault`/`vault_state`;
// `system_program` is auto-injected from its field type.
// ANCHOR: accounts
// src/instructions/initialize.rs
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(crate::test_helpers::VaultAccs)
)]
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        init,
        payer = user,
        seeds = [b"state", user.key().as_ref()],
        bump,
        space = 8 + VaultState::INIT_SPACE
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        seeds = [b"vault", vault_state.key().as_ref()],
        bump
    )]
    pub vault: SystemAccount<'info>,

    pub system_program: Program<'info, System>,
}
// ANCHOR_END: accounts

impl<'info> Initialize<'info> {
    pub fn initialize(&mut self, bumps: &InitializeBumps) -> Result<()> {
        // Save data to state
        self.vault_state.vault_bump = bumps.vault;
        self.vault_state.state_bump = bumps.vault_state;
        Ok(())
    }
}
