//! Integration: derive emits a working `From<Bundle> for accounts::Make`
//! impl, including type-based const injection.

use anchor_lang::prelude::Pubkey;
use anchor_litesvm_derive::BundledPubkeys;

// Stand-in for the modules Anchor would generate. The derive emits paths
// like `accounts::Make` and `instruction::Make`, so we mock them here.
pub mod accounts {
    use super::*;
    pub struct Make {
        pub maker: Pubkey,
        pub mint_a: Pubkey,
        pub system_program: Pubkey,
        pub associated_token_program: Pubkey,
        pub token_program: Pubkey,
    }
    impl anchor_lang::ToAccountMetas for Make {
        fn to_account_metas(
            &self,
            _signers: Option<bool>,
        ) -> Vec<anchor_lang::prelude::AccountMeta> {
            vec![]
        }
    }
}
pub mod instruction {
    use anchor_lang::AnchorSerialize;
    // See derive_buildable.rs for why this is hand-rolled.
    pub struct Make {
        pub amount: u64,
    }
    impl AnchorSerialize for Make {
        fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
            writer.write_all(&self.amount.to_le_bytes())
        }
    }
    impl anchor_lang::Discriminator for Make {
        const DISCRIMINATOR: &'static [u8] = &[1, 2, 3, 4, 5, 6, 7, 8];
    }
    impl anchor_lang::InstructionData for Make {}
}

// Stand-ins for the Anchor account-wrapper types. The derive only inspects
// the *name* of the head and the inner generic, so these can be empty.
pub struct Signer<'info, T = ()>(::core::marker::PhantomData<&'info T>);
pub struct InterfaceAccount<'info, T>(::core::marker::PhantomData<&'info T>);
pub struct Program<'info, T>(::core::marker::PhantomData<&'info T>);
pub struct Interface<'info, T>(::core::marker::PhantomData<&'info T>);
pub struct System;
impl anchor_lang::Id for System {
    fn id() -> Pubkey {
        anchor_lang::solana_program::system_program::ID
    }
}
pub struct AssociatedToken;
impl anchor_lang::Id for AssociatedToken {
    fn id() -> Pubkey {
        anchor_spl::associated_token::ID
    }
}
pub struct TokenInterface;
pub struct Mint;

#[derive(Copy, Clone, Debug)]
pub struct EscrowBundle {
    pub maker: Pubkey,
    pub mint_a: Pubkey,
}

#[derive(BundledPubkeys)]
#[bundled_with(EscrowBundle)]
#[allow(dead_code)]
pub struct Make<'info> {
    pub maker: Signer<'info>,
    pub mint_a: InterfaceAccount<'info, Mint>,
    pub system_program: Program<'info, System>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
}

#[test]
fn projects_bundle_fields_and_injects_consts() {
    let maker = Pubkey::new_unique();
    let mint_a = Pubkey::new_unique();
    let bundle = EscrowBundle { maker, mint_a };
    let accs: accounts::Make = bundle.into();
    assert_eq!(accs.maker, maker);
    assert_eq!(accs.mint_a, mint_a);
    assert_eq!(
        accs.system_program,
        anchor_lang::solana_program::system_program::ID
    );
    assert_eq!(
        accs.associated_token_program,
        anchor_spl::associated_token::ID
    );
    assert_eq!(accs.token_program, anchor_spl::token::ID);
}
