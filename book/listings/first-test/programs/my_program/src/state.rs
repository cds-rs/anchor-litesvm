use anchor_lang::prelude::*;

// ANCHOR: state
// src/state.rs
#[account]
#[derive(InitSpace)]
pub struct Data {
    pub value: u64,
}
// ANCHOR_END: state
