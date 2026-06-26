use anchor_lang::prelude::*;

// ANCHOR: state
// src/state.rs
#[account]
#[derive(InitSpace)]
pub struct Counter {
    pub count: u64,
}
// ANCHOR_END: state
