//! Integration: the BuildableIx impl emitted by the derive plugs into
//! `anchor_litesvm::Program::build_ix` without any further glue.

use anchor_lang::prelude::Pubkey;
use anchor_litesvm::BuildableIx;
use anchor_litesvm_derive::BundledPubkeys;

pub mod accounts {
    use super::*;
    pub struct Make {
        pub maker: Pubkey,
        pub system_program: Pubkey,
    }
    impl anchor_lang::ToAccountMetas for Make {
        fn to_account_metas(
            &self,
            _signers: Option<bool>,
        ) -> Vec<anchor_lang::prelude::AccountMeta> {
            vec![
                anchor_lang::prelude::AccountMeta::new(self.maker, true),
                anchor_lang::prelude::AccountMeta::new_readonly(self.system_program, false),
            ]
        }
    }
}
pub mod instruction {
    #[derive(anchor_lang::AnchorSerialize, anchor_lang::AnchorDeserialize)]
    pub struct Make {
        pub amount: u64,
    }
    impl anchor_lang::Discriminator for Make {
        const DISCRIMINATOR: &'static [u8] = &[1, 2, 3, 4, 5, 6, 7, 8];
    }
    impl anchor_lang::InstructionData for Make {}
}

pub struct Signer<'info, T = ()>(::core::marker::PhantomData<&'info T>);
pub struct Program<'info, T>(::core::marker::PhantomData<&'info T>);
pub struct System;
impl anchor_lang::Id for System {
    fn id() -> Pubkey {
        anchor_lang::solana_program::system_program::ID
    }
}

#[derive(Copy, Clone, Debug)]
pub struct EscrowBundle {
    pub maker: Pubkey,
}

#[derive(BundledPubkeys)]
#[bundled_with(EscrowBundle)]
#[allow(dead_code)]
pub struct Make<'info> {
    pub maker: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[test]
fn build_ix_uses_derived_accounts() {
    fn _assert_buildable<T: BuildableIx<EscrowBundle>>() {}
    _assert_buildable::<instruction::Make>();

    let program_id = Pubkey::new_unique();
    let maker = Pubkey::new_unique();
    let bundle = EscrowBundle { maker };
    let ix =
        anchor_litesvm::Program::new(program_id).build_ix(bundle, instruction::Make { amount: 7 });

    assert_eq!(ix.program_id, program_id);
    assert_eq!(ix.accounts.len(), 2);
    assert_eq!(ix.accounts[0].pubkey, maker);
    assert_eq!(
        ix.accounts[1].pubkey,
        anchor_lang::solana_program::system_program::ID
    );
    // First 8 bytes are the discriminator; next 8 are the u64 amount LE.
    assert_eq!(
        &ix.data[0..8],
        <instruction::Make as anchor_lang::Discriminator>::DISCRIMINATOR
    );
    assert_eq!(&ix.data[8..16], &7u64.to_le_bytes());
}
