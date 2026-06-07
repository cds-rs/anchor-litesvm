//! Integration: `#[bundled_with(..., instruction = path, accounts = path)]`
//! lets the derive target paths that don't follow the default
//! `crate::accounts::<StructName>` / `crate::instruction::<StructName>`
//! convention. This is the workaround for the dogfood mismatch
//! (`fn initialize_poll` paired with `struct InitPoll`).

use anchor_lang::prelude::Pubkey;
use anchor_litesvm::BuildableIx;
use anchor_litesvm_derive::BundledPubkeys;

pub mod accounts {
    use super::*;
    // Anchor names this from `Context<InitPoll>` (struct name).
    pub struct InitPoll {
        pub auth: Pubkey,
        pub system_program: Pubkey,
    }
    impl anchor_lang::ToAccountMetas for InitPoll {
        fn to_account_metas(
            &self,
            _signers: Option<bool>,
        ) -> Vec<anchor_lang::prelude::AccountMeta> {
            vec![
                anchor_lang::prelude::AccountMeta::new(self.auth, true),
                anchor_lang::prelude::AccountMeta::new_readonly(self.system_program, false),
            ]
        }
    }
}

pub mod instruction {
    use anchor_lang::AnchorSerialize;
    // Anchor names this from `PascalCase(fn initialize_poll)`, which
    // differs from the accounts struct name. AnchorSerialize is
    // hand-rolled; see derive_buildable.rs for why.
    pub struct InitializePoll {
        pub seed: u64,
    }
    impl AnchorSerialize for InitializePoll {
        fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
            writer.write_all(&self.seed.to_le_bytes())
        }
    }
    impl anchor_lang::Discriminator for InitializePoll {
        const DISCRIMINATOR: &'static [u8] = &[9, 8, 7, 6, 5, 4, 3, 2];
    }
    impl anchor_lang::InstructionData for InitializePoll {}
}

pub struct Signer<'info, T = ()>(::core::marker::PhantomData<&'info T>);
pub struct Program<'info, T>(::core::marker::PhantomData<&'info T>);
pub struct System;

#[derive(Copy, Clone, Debug)]
pub struct InitPollBundle {
    pub auth: Pubkey,
}

// Without the overrides, the derive would emit impls for
// `crate::instruction::InitPoll` (doesn't exist) and
// `crate::accounts::InitPoll` (exists). The `instruction = ...` override
// is the unblocker; `accounts = ...` is set explicitly here for symmetry,
// even though the default would have resolved.
#[derive(BundledPubkeys)]
#[bundled_with(
    InitPollBundle,
    instruction = crate::instruction::InitializePoll,
    accounts = crate::accounts::InitPoll,
)]
#[allow(dead_code)]
pub struct InitPoll<'info> {
    pub auth: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[test]
fn override_wires_buildable_to_renamed_instruction() {
    fn _assert_buildable<T: BuildableIx<InitPollBundle>>() {}
    _assert_buildable::<instruction::InitializePoll>();

    let program_id = Pubkey::new_unique();
    let auth = Pubkey::new_unique();
    let bundle = InitPollBundle { auth };
    let ix = anchor_litesvm::Program::new(program_id)
        .build_ix(bundle, instruction::InitializePoll { seed: 42 });

    assert_eq!(ix.program_id, program_id);
    assert_eq!(ix.accounts.len(), 2);
    assert_eq!(ix.accounts[0].pubkey, auth);
    assert_eq!(
        ix.accounts[1].pubkey,
        anchor_lang::solana_program::system_program::ID
    );
    assert_eq!(
        &ix.data[0..8],
        <instruction::InitializePoll as anchor_lang::Discriminator>::DISCRIMINATOR
    );
    assert_eq!(&ix.data[8..16], &42u64.to_le_bytes());
}
