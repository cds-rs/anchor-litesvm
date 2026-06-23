use crate::state::VaultState;
use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

// What `#[derive(BundledPubkeys)]` buys us (host/test builds only):
//
// 1. Host-only gate. `not(target_os = "solana")` keeps every attribute below
//    out of the on-chain SBF binary; this is test scaffolding and never ships.
//
// 2. Generates two impls next to this struct:
//      - `From<VaultAccs> for accounts::Close`: projects the bundle's pubkeys
//        into the generated CPI accounts struct, auto-injecting well-known
//        program IDs (here `system_program`) from the field type, so the
//        bundle never has to carry them.
//      - `BuildableIx<VaultAccs> for instruction::Close`, with
//        `type Accounts = accounts::Close`. This is the compile-time pairing
//        that lets `program.build_ix(bundle, instruction::Close { .. })` find
//        the matching accounts struct; a mismatched arg/accounts pair is a
//        type error, not a runtime surprise.
//
// 3. `bundled_with(..)` names the bundle. `VaultAccs` is a plain struct whose
//    named fields must cover every non-program account here (`user`, `vault`,
//    `vault_state`); the program accounts from (2) are auto-injected and so
//    are omitted from the bundle.
#[cfg_attr(
    not(target_os = "solana"),                   // 1
    derive(anchor_litesvm::BundledPubkeys),      // 2
    bundled_with(crate::test_helpers::VaultAccs) // 3
)]
#[derive(Accounts)]
pub struct Close<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"vault", vault_state.key().as_ref()],
        bump = vault_state.vault_bump,
    )]
    pub vault: SystemAccount<'info>,

    #[account(
        mut,
        seeds = [b"state", user.key().as_ref()],
        bump = vault_state.state_bump,
        close = user,
    )]
    pub vault_state: Account<'info, VaultState>,

    pub system_program: Program<'info, System>,
}

impl<'info> Close<'info> {
    pub fn close(&mut self) -> Result<()> {
        let cpi_accounts = Transfer {
            from: self.vault.to_account_info(),
            to: self.user.to_account_info(),
        };

        let seeds = [
            b"vault",
            self.vault_state.to_account_info().key.as_ref(),
            &[self.vault_state.vault_bump],
        ];

        let signer_seeds: &[&[&[u8]]] = &[&seeds[..]];

        let cpi_ctx = CpiContext::new_with_signer(System::id(), cpi_accounts, signer_seeds);

        let amount = self.vault.lamports();
        transfer(cpi_ctx, amount)
    }
}
