//! Integration: the structural rule (any `Program<'info, T>` injects
//! `<T as Id>::id()`), the `#[bundle(inject = expr)]` hatch, the
//! `#[bundle(default = expr)]` override, and the generated
//! `injected_programs()` naming table. This is the well-known-programs
//! proposal end to end: the mpl-core shape that motivated it, exercised with
//! a custom `Id` type the old table never heard of.

use anchor_lang::prelude::Pubkey;
use anchor_litesvm_derive::{Bundle, BundledPubkeys};

/// The five-line `Id` wrapper the proposal asks of program code whose
/// external crate ships no Anchor type.
pub struct MplCore;
impl anchor_lang::Id for MplCore {
    fn id() -> Pubkey {
        Pubkey::new_from_array([7u8; 32])
    }
}

pub mod accounts {
    use super::*;
    pub struct Create {
        pub payer: Pubkey,
        pub asset: Pubkey,
        pub mpl_core_program: Pubkey,
        pub log_wrapper: Pubkey,
    }
    impl anchor_lang::ToAccountMetas for Create {
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
    // Hand-rolled like compat's other test mocks: anchor 0.31's serialize
    // derive expands referencing `borsh` at the call site.
    pub struct Create {}
    impl AnchorSerialize for Create {
        fn serialize<W: std::io::Write>(&self, _writer: &mut W) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl anchor_lang::Discriminator for Create {
        const DISCRIMINATOR: &'static [u8] = &[1, 2, 3, 4, 5, 6, 7, 8];
    }
    impl anchor_lang::InstructionData for Create {}
}

const LOG_WRAPPER_ID: Pubkey = Pubkey::new_from_array([9u8; 32]);

// Anchor-shaped stand-ins so the derive sees the real field types.
pub struct Signer<'info>(std::marker::PhantomData<&'info ()>);
pub struct UncheckedAccount<'info>(std::marker::PhantomData<&'info ()>);
pub struct Program<'info, T>(std::marker::PhantomData<(&'info (), T)>);

#[derive(BundledPubkeys)]
#[bundled_with(CreateBundle)]
#[allow(dead_code)]
pub struct Create<'info> {
    pub payer: Signer<'info>,
    pub asset: Signer<'info>,
    /// The motivating case: a non-SPL program the old table had no row for.
    pub mpl_core_program: Program<'info, MplCore>,
    /// The hatch: an account that genuinely cannot be a typed Program<T>.
    #[bundle(inject = LOG_WRAPPER_ID)]
    pub log_wrapper: UncheckedAccount<'info>,
}

#[derive(Bundle)]
pub struct CreateBundle {
    pub payer: Pubkey,
    pub asset: Pubkey,
    /// The override that deletes hand-rolled Default impls downstream.
    #[bundle(default = Pubkey::new_from_array([3u8; 32]))]
    pub known_mint: Pubkey,
}

#[test]
fn the_rule_injects_any_id_type_and_the_hatch_injects_expressions() {
    let bundle = CreateBundle {
        payer: Pubkey::new_unique(),
        asset: Pubkey::new_unique(),
        known_mint: Pubkey::new_unique(),
    };
    let payer = bundle.payer;
    let asset = bundle.asset;
    let acc: accounts::Create = bundle.into();
    assert_eq!(acc.payer, payer);
    assert_eq!(acc.asset, asset);
    // The structural rule: <MplCore as Id>::id(), no table row anywhere.
    assert_eq!(acc.mpl_core_program, Pubkey::new_from_array([7u8; 32]));
    // The hatch: the given expression verbatim.
    assert_eq!(acc.log_wrapper, LOG_WRAPPER_ID);
}

#[test]
fn injected_programs_names_the_rule_classified_fields_only() {
    let table = Create::injected_programs();
    assert_eq!(table.len(), 1, "the hatch field has no type to name");
    assert_eq!(
        table[0],
        (Pubkey::new_from_array([7u8; 32]), "MplCore"),
        "injected programs name themselves"
    );
}

#[test]
fn bundle_default_override_pins_the_field() {
    let b = CreateBundle::default();
    assert_eq!(b.known_mint, Pubkey::new_from_array([3u8; 32]));
    assert_ne!(b.payer, b.asset, "unannotated fields stay unique placeholders");
}
