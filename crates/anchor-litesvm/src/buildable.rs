//! Type-level pairing of an instruction's args struct with its accounts struct.
//!
//! [`BuildableIx`] lets [`Program::build_ix`] accept just an args struct plus a
//! caller-supplied bundle of pubkeys, then construct the corresponding
//! accounts struct via [`From`]. Implementing it for each instruction's args
//! struct (one line per instruction) gives the program a one-method ix
//! constructor with compile-time-checked accounts/args pairing.
//!
//! # Example
//!
//! In your program crate:
//!
//! ```ignore
//! use anchor_litesvm::{Bundle, BundledPubkeys};
//! use anchor_lang::prelude::*;
//!
//! #[derive(Bundle, Copy, Clone)]
//! pub struct EscrowBundle {
//!     pub user: Pubkey,
//!     pub vault_state: Pubkey,
//!     pub vault: Pubkey,
//! }
//!
//! #[derive(Accounts, BundledPubkeys)]
//! #[bundled_with(EscrowBundle)]
//! pub struct Deposit<'info> {
//!     pub user: Signer<'info>,
//!     pub vault_state: Account<'info, VaultState>,
//!     pub vault: Account<'info, Vault>,
//!     pub system_program: Program<'info, System>,
//! }
//! ```
//!
//! The derive generates `From<EscrowBundle> for accounts::Deposit` (with
//! `system_program` auto-injected from the `Program<System>` type) and
//! `BuildableIx<EscrowBundle> for instruction::Deposit`. `#[derive(Bundle)]`
//! emits a `Default` impl that fills every field with `Pubkey::new_unique()`,
//! which is what tests want (std `Pubkey::default()` is all-zeros and gets
//! rejected by virtually every Solana program).
//!
//! In your test:
//!
//! ```ignore
//! let ix = ctx.program().build_ix(
//!     BundledPubkeys { user, state, vault },
//!     instruction::Deposit { amount: 1_000_000 },
//! );
//! ```
//!
//! The bundle type parameter is on the trait (not an associated type) so the
//! same args struct can implement `BuildableIx<TestBundle>` and
//! `BuildableIx<ProdBundle>` if a program wants more than one bundle shape.
//!
//! For negative-path tests that need a deliberately-wrong account, see
//! [`Program::build_ix_with`], which takes a closure with `&mut Self::Accounts`.
//!
//! [`Program::build_ix`]: crate::program::Program::build_ix
//! [`Program::build_ix_with`]: crate::program::Program::build_ix_with

use anchor_lang::{InstructionData, ToAccountMetas};

/// Pairs an instruction's args struct with its accounts struct, given a
/// caller-supplied bundle that knows how to construct the accounts.
///
/// The bundle type `B` is whatever pubkey container the program author finds
/// convenient (a single struct, a tuple, etc.). The projection itself
/// (bundle → accounts) is expressed via `B: Into<Self::Accounts>` on the
/// call sites that need it ([`Program::build_ix`],
/// [`Program::build_ix_with`], and the [`Tx`] fluent builder), rather
/// than as a bound on the associated type. That keeps this trait open
/// to any derive that emits either `From<B> for Self::Accounts` (in
/// which case `Into` is implied for free, via the blanket impl) or
/// `Into<Self::Accounts> for B` directly. In-house `BundledPubkeys`
/// emits the former; multi-source projections via `BundleFrom` go
/// through `From<(&T1, &T2, ...)>` and reach the call site as
/// `(&t1, &t2).into()`. Both satisfy the call-site bound.
///
/// See the [module-level docs](crate::buildable) for an end-to-end example.
///
/// [`Program::build_ix`]: crate::program::Program::build_ix
/// [`Program::build_ix_with`]: crate::program::Program::build_ix_with
/// [`Tx`]: crate::tx::Tx
#[diagnostic::on_unimplemented(
    message = "`{Self}` can't be built with bundle `{B}`: no `BuildableIx<{B}>` impl",
    label = "no `BuildableIx<{B}>` for this args type",
    note = "Add `#[derive(BundledPubkeys)] #[bundled_with({B})]` to the \
            matching `#[derive(Accounts)]` struct, or hand-write \
            `impl BuildableIx<{B}> for {Self}`.",
    note = "If the args type *does* derive `BundledPubkeys` for a different \
            bundle, double-check that the bundle you're passing to \
            `build_ix` matches the one in `#[bundled_with(...)]`."
)]
pub trait BuildableIx<B>: InstructionData {
    /// The accounts struct paired with this instruction's args struct.
    type Accounts: ToAccountMetas;
}
