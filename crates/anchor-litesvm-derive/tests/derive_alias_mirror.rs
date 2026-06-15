//! Integration: `#[derive(AliasMirror)]` registers `Pubkey` fields in
//! an `AnchorContext` alias table via a single `alias_all` call.

#![allow(dead_code)]

use anchor_lang::prelude::Pubkey;
use anchor_litesvm::{AliasMirror, AnchorContext, LiteSVM};

#[derive(Copy, Clone, AliasMirror)]
pub struct Pool {
    pub seed: u64,       // skipped (not Pubkey)
    pub mint_x: Pubkey,  // → "MintX"
    pub mint_y: Pubkey,  // → "MintY"
    pub config: Pubkey,  // → "Config"
    pub vault_x: Pubkey, // → "VaultX"
    pub vault_y: Pubkey, // → "VaultY"
    #[alias("LP Vault")]
    pub lp_vault: Pubkey, // explicit label with a space
    #[alias(skip)]
    pub debug_only: Pubkey, // not aliased
}

fn make_ctx() -> AnchorContext {
    AnchorContext::new(LiteSVM::new(), Pubkey::new_unique())
}

fn make_pool() -> Pool {
    Pool {
        seed: 42,
        mint_x: Pubkey::new_unique(),
        mint_y: Pubkey::new_unique(),
        config: Pubkey::new_unique(),
        vault_x: Pubkey::new_unique(),
        vault_y: Pubkey::new_unique(),
        lp_vault: Pubkey::new_unique(),
        debug_only: Pubkey::new_unique(),
    }
}

#[test]
fn alias_all_registers_pubkey_fields_with_pascal_case() {
    let mut ctx = make_ctx();
    let pool = make_pool();
    pool.alias_all(&mut ctx);

    assert_eq!(ctx.aliases.resolve_by_pubkey(&pool.mint_x), Some("MintX"));
    assert_eq!(ctx.aliases.resolve_by_pubkey(&pool.mint_y), Some("MintY"));
    assert_eq!(ctx.aliases.resolve_by_pubkey(&pool.config), Some("Config"));
    assert_eq!(ctx.aliases.resolve_by_pubkey(&pool.vault_x), Some("VaultX"));
    assert_eq!(ctx.aliases.resolve_by_pubkey(&pool.vault_y), Some("VaultY"));
}

#[test]
fn alias_override_uses_explicit_label() {
    let mut ctx = make_ctx();
    let pool = make_pool();
    pool.alias_all(&mut ctx);

    // #[alias("LP Vault")] override carries through verbatim.
    assert_eq!(
        ctx.aliases.resolve_by_pubkey(&pool.lp_vault),
        Some("LP Vault")
    );
}

#[test]
fn alias_skip_omits_pubkey_field() {
    let mut ctx = make_ctx();
    let pool = make_pool();
    pool.alias_all(&mut ctx);

    // #[alias(skip)] keeps the field out of the table.
    assert!(ctx.aliases.resolve_by_pubkey(&pool.debug_only).is_none());
}

#[test]
fn alias_all_returns_ctx_for_chaining() {
    let mut ctx = make_ctx();
    let pool = make_pool();
    let extra = Pubkey::new_unique();
    // Chain alias_all() with another alias() call.
    pool.alias_all(&mut ctx).alias(extra, "Extra");
    assert_eq!(ctx.aliases.resolve_by_pubkey(&extra), Some("Extra"));
}
