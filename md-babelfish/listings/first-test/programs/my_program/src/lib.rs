pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

// ANCHOR: wire
// src/lib.rs
#[cfg(not(target_os = "solana"))]
pub mod test_helpers;
// ANCHOR_END: wire

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("3JAqyRbH1ripdAE8h7UrK1TKc84yyNw9QtDqHNVMQcz4");

// ANCHOR: program
// src/lib.rs
#[program]
pub mod my_program {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, value: u64) -> Result<()> {
        ctx.accounts.initialize(value)
    }
}
// ANCHOR_END: program
