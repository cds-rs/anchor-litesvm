//! Integration: `#[derive(BundleFrom)]` projects a multi-source tuple
//! into a per-ix bundle. Mirrors the 02-amm shape (`Pool` + `User`
//! fixtures → SwapBundle / AddLiquidityBundle).

#![allow(dead_code)]

use anchor_lang::prelude::Pubkey;
use anchor_litesvm_derive::BundleFrom;

// Shared fixture: pool-level PDAs.
#[derive(Copy, Clone)]
pub struct Pool {
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub mint_lp: Pubkey,
    pub config: Pubkey,
    pub vault_x: Pubkey,
    pub vault_y: Pubkey,
    pub lp_vault: Pubkey,
}

// Per-actor fixture: signer + token ATAs.
#[derive(Clone)]
pub struct UserAccounts {
    pub key: Pubkey,
    pub ata_x: Pubkey,
    pub ata_y: Pubkey,
}

impl UserAccounts {
    pub fn pubkey(&self) -> Pubkey {
        self.key
    }
    pub fn ata_lp(&self, _mint_lp: &Pubkey) -> Pubkey {
        // For the test, fake a deterministic derivation.
        Pubkey::new_from_array([42; 32])
    }
}

// Bundle 1: every projection rule exercised.
#[derive(Debug, BundleFrom)]
#[from_fixtures(p: Pool, u: UserAccounts)]
pub struct SwapBundle {
    // Method-call override — no field name match.
    #[from(u.pubkey())]
    pub user: Pubkey,
    // Auto from first fixture (Pool).
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub config: Pubkey,
    pub vault_x: Pubkey,
    pub vault_y: Pubkey,
    // Override: different name (`user_x` ← `u.ata_x`).
    #[from(u.ata_x)]
    pub user_x: Pubkey,
    #[from(u.ata_y)]
    pub user_y: Pubkey,
}

// Bundle 2: one override pulls a value computed from BOTH fixtures
// (cross-fixture expression), proving the bound names compose.
#[derive(Debug, BundleFrom)]
#[from_fixtures(p: Pool, u: UserAccounts)]
pub struct AddLiquidityBundle {
    #[from(u.pubkey())]
    pub user: Pubkey,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub config: Pubkey,
    pub mint_lp: Pubkey,
    pub vault_x: Pubkey,
    pub vault_y: Pubkey,
    pub lp_vault: Pubkey,
    #[from(u.ata_x)]
    pub user_x: Pubkey,
    #[from(u.ata_y)]
    pub user_y: Pubkey,
    // Cross-fixture: user method call that needs a pool field.
    #[from(u.ata_lp(&p.mint_lp))]
    pub user_lp: Pubkey,
}

#[test]
fn projects_from_two_fixtures_with_auto_and_overrides() {
    let pool = Pool {
        mint_x: Pubkey::new_unique(),
        mint_y: Pubkey::new_unique(),
        mint_lp: Pubkey::new_unique(),
        config: Pubkey::new_unique(),
        vault_x: Pubkey::new_unique(),
        vault_y: Pubkey::new_unique(),
        lp_vault: Pubkey::new_unique(),
    };
    let user = UserAccounts {
        key: Pubkey::new_unique(),
        ata_x: Pubkey::new_unique(),
        ata_y: Pubkey::new_unique(),
    };

    let b = SwapBundle::from((&pool, &user));
    assert_eq!(b.user, user.key);
    assert_eq!(b.mint_x, pool.mint_x);
    assert_eq!(b.mint_y, pool.mint_y);
    assert_eq!(b.config, pool.config);
    assert_eq!(b.vault_x, pool.vault_x);
    assert_eq!(b.vault_y, pool.vault_y);
    assert_eq!(b.user_x, user.ata_x);
    assert_eq!(b.user_y, user.ata_y);
}

#[test]
fn cross_fixture_expression_in_override() {
    let pool = Pool {
        mint_x: Pubkey::new_unique(),
        mint_y: Pubkey::new_unique(),
        mint_lp: Pubkey::new_unique(),
        config: Pubkey::new_unique(),
        vault_x: Pubkey::new_unique(),
        vault_y: Pubkey::new_unique(),
        lp_vault: Pubkey::new_unique(),
    };
    let user = UserAccounts {
        key: Pubkey::new_unique(),
        ata_x: Pubkey::new_unique(),
        ata_y: Pubkey::new_unique(),
    };

    let b = AddLiquidityBundle::from((&pool, &user));
    assert_eq!(b.user_lp, Pubkey::new_from_array([42; 32]));
    assert_eq!(b.mint_lp, pool.mint_lp);
}
