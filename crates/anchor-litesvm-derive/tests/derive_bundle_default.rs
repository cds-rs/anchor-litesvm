//! Integration: `#[derive(Bundle)]` emits a `Default` impl that uses
//! `Pubkey::new_unique()` per field (not std `Pubkey::default()`, which
//! is all-zeros and gets rejected by virtually every Solana program).

use anchor_lang::prelude::Pubkey;
use anchor_litesvm_derive::Bundle;

#[derive(Bundle, Copy, Clone, Debug)]
pub struct EscrowBundle {
    pub maker: Pubkey,
    pub taker: Pubkey,
    pub mint_a: Pubkey,
}

#[test]
fn default_uses_new_unique() {
    let a = EscrowBundle::default();
    let b = EscrowBundle::default();
    // new_unique() returns a distinct pubkey each call, so two defaults
    // should disagree on every field.
    assert_ne!(a.maker, b.maker);
    assert_ne!(a.taker, b.taker);
    assert_ne!(a.mint_a, b.mint_a);
    // None of them are the all-zeros Pubkey::default().
    assert_ne!(a.maker, Pubkey::default());
    assert_ne!(a.taker, Pubkey::default());
    assert_ne!(a.mint_a, Pubkey::default());
}
