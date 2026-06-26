pub mod instructions;
pub mod state;

// ANCHOR: wire
// src/lib.rs
#[cfg(not(target_os = "solana"))]
pub mod test_helpers;
// ANCHOR_END: wire

use anchor_lang::prelude::*;

pub use instructions::*;
pub use state::*;

declare_id!("8E6a1bwRyKjw8YhXYPspSUStESC7mKNkG5hAzz8oERPj");

// ANCHOR: program
// src/lib.rs
#[program]
pub mod counter {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, start: u64) -> Result<()> {
        ctx.accounts.initialize(start)
    }

    pub fn increment(ctx: Context<Increment>) -> Result<()> {
        ctx.accounts.increment()
    }

    pub fn donate(ctx: Context<Donate>, amount: u64) -> Result<()> {
        ctx.accounts.donate(amount)
    }
}
// ANCHOR_END: program
