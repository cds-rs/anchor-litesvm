//! Reproduces the dogfood bug: `fn initialize_poll` paired with
//! `struct InitPoll`. Anchor names `instruction::InitializePoll` from
//! the handler, but the derive (without an `instruction = ...` override)
//! reaches for `crate::instruction::InitPoll`, which doesn't exist.
//!
//! The path that the derive emits (`crate::instruction::#accounts_ident`)
//! inherits its span from `#accounts_ident`, so rustc's E0425 attributes
//! to the struct definition site and surfaces a "consider importing"
//! suggestion. The `instruction = path` override on `bundled_with` is the
//! actual fix for the user; this fixture just locks in the diagnostic.

use anchor_lang::prelude::Pubkey;
use anchor_litesvm_derive::BundledPubkeys;

pub mod accounts {
    use anchor_lang::prelude::Pubkey;
    pub struct InitPoll {
        pub auth: Pubkey,
    }
    impl anchor_lang::ToAccountMetas for InitPoll {
        fn to_account_metas(
            &self,
            _signers: Option<bool>,
        ) -> Vec<anchor_lang::prelude::AccountMeta> {
            vec![anchor_lang::prelude::AccountMeta::new(self.auth, true)]
        }
    }
}

pub mod instruction {
    // The mismatch: Anchor's handler is `fn initialize_poll`, so the
    // generated type is `InitializePoll`. The accounts struct below is
    // `InitPoll`, so the derive infers `crate::instruction::InitPoll`,
    // which doesn't exist here (only `InitializePoll` does).
    #[derive(anchor_lang::AnchorSerialize, anchor_lang::AnchorDeserialize)]
    pub struct InitializePoll {}
    impl anchor_lang::Discriminator for InitializePoll {
        const DISCRIMINATOR: &'static [u8] = &[];
    }
    impl anchor_lang::InstructionData for InitializePoll {}
}

pub struct Signer<'info, T = ()>(::core::marker::PhantomData<&'info T>);

#[derive(Copy, Clone, Debug)]
pub struct InitPollBundle {
    pub auth: Pubkey,
}

#[derive(BundledPubkeys)]
#[bundled_with(InitPollBundle)]
#[allow(dead_code)]
pub struct InitPoll<'info> {
    pub auth: Signer<'info>,
}

fn main() {}
