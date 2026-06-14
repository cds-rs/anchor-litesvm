//! # anchor-litesvm
//!
//! Testing framework for Anchor programs using LiteSVM.
//!
//! This crate provides a **simplified syntax similar to anchor-client** but without RPC overhead,
//! achieving **78% code reduction** compared to raw LiteSVM.
//!
//! ## Why anchor-litesvm?
//!
//! | Feature | anchor-client + LiteSVM | anchor-litesvm |
//! |---------|-------------------------|----------------|
//! | **Code Lines** | 279 | **106 (78% less)** |
//! | **Compilation** | Slow (network deps) | **40% faster** |
//! | **Setup** | Mock RPC needed | **One line** |
//! | **Syntax** | anchor-client | **Similar to anchor-client** |
//! | **Helpers** | Manual | **Built-in** |
//!
//! ## Key Features
//!
//! - **Simplified Syntax**: Similar to anchor-client
//! - **No Mock RPC Setup**: One-line initialization
//! - **Integrated Test Helpers**: Token operations, assertions, event parsing
//! - **Familiar API**: If you know anchor-client, you know this
//! - **Transferable Knowledge**: Test skills apply to production
//! - **Type Safety**: Compile-time validation with Anchor types
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use anchor_litesvm::{AnchorLiteSVM, TestHelpers, AssertionHelpers, Signer};
//!
//! // 1. Generate client types from your program
//! anchor_lang::declare_program!(my_program);
//!
//! #[test]
//! fn test_my_program() {
//!     // 2. One-line setup (no mock RPC needed). The name registers as
//!     //    a pubkey alias so structured logs read `my_program::Transfer`
//!     //    instead of the raw program ID.
//!     let mut ctx = AnchorLiteSVM::build_with_program(
//!         my_program::ID,
//!         "my_program",
//!         include_bytes!("../target/deploy/my_program.so"),
//!     );
//!
//!     // 3. Create test accounts with helpers
//!     let user = ctx.svm.create_funded_account(10_000_000_000).unwrap();
//!     let mint = ctx.svm.create_token_mint(&user, 9).unwrap();
//!
//!     // 4. Build instruction (simplified syntax - similar to anchor client)
//!     let ix = ctx.program()
//!         .accounts(my_program::client::accounts::Transfer {
//!             from: sender_account,
//!             to: recipient_account,
//!             authority: user.pubkey(),
//!             token_program: spl_token::id(),
//!         })
//!         .args(my_program::client::args::Transfer { amount: 100 })
//!         .instruction()?;
//!
//!     // 5. Execute and verify
//!     ctx.execute_instruction(ix, &[&user])?.assert_success();
//!     ctx.svm.assert_token_balance(&recipient_account, 100);
//! }
//! ```
//!
//! ## Common Patterns
//!
//! ### Token Operations
//!
//! ```rust,ignore
//! use litesvm_utils::TestHelpers;
//!
//! let mint = ctx.svm.create_token_mint(&authority, 9)?;
//! let token_account = ctx.svm.create_associated_token_account(&mint.pubkey(), &owner)?;
//! ctx.svm.mint_to(&mint.pubkey(), &token_account, &authority, 1_000_000)?;
//!
//! // Read SPL Token balance. `None` means the account doesn't exist
//! // (so closed-vault assertions read tightly).
//! assert_eq!(ctx.svm.token_balance(&token_account), Some(1_000_000));
//! assert!(ctx.svm.token_balance(&closed_vault).is_none());
//! ```
//!
//! ### PDA Derivation
//!
//! ```rust,ignore
//! // Just the address
//! let pda = ctx.svm.get_pda(&[b"vault", user.pubkey().as_ref()], &program_id);
//!
//! // With bump seed
//! let (pda, bump) = ctx.svm.get_pda_with_bump(&[b"vault"], &program_id);
//! ```
//!
//! ### Error Testing
//!
//! ```rust,ignore
//! let result = ctx.execute_instruction(ix, &[&user])?;
//! // Pick the assertion that matches your scenario; each consumes `result`.
//! result.assert_failure();                          // generic
//! // or: result.assert_error("EscrowExpired");     // substring in logs or error field
//! // or: result.assert_error_code(6000);           // Anchor custom error code
//! ```
//!
//! ### Event Parsing
//!
//! ```rust,ignore
//! use anchor_litesvm::EventHelpers;
//!
//! let events: Vec<TransferEvent> = result.parse_events()?;
//! result.assert_event_emitted::<TransferEvent>();
//! ```
//!
//! ### Account Deserialization
//!
//! ```rust,ignore
//! let account: MyAccountType = ctx.get_account(&pda)?;
//! assert_eq!(account.authority, user.pubkey());
//! ```
//!
//! ### Bundled Instruction Construction
//!
//! Instead of hand-filling `accounts::Foo { .. }.to_account_metas(None)`
//! plus `instruction::Foo { .. }.data()` per instruction, define a
//! `Bundle` once for your program and let the derive emit the wiring.
//!
//! Step 1: a host-only bundle struct holding every account pubkey your
//! instructions reference (omit `Program<System>`, `Program<AssociatedToken>`,
//! and `Interface<TokenInterface>` — those are auto-injected):
//!
//! ```rust,ignore
//! #[cfg(not(target_os = "solana"))]
//! pub mod test_helpers {
//!     use anchor_lang::prelude::Pubkey;
//!     use anchor_litesvm::Bundle;
//!
//!     #[derive(Bundle, Copy, Clone, Debug)]
//!     pub struct MyBundle {
//!         pub maker: Pubkey,
//!         pub mint_a: Pubkey,
//!         pub vault: Pubkey,
//!         // ... etc
//!     }
//! }
//! ```
//!
//! Step 2: attach `BundledPubkeys` to each `#[derive(Accounts)]` struct,
//! gated for non-Solana so it doesn't pull into the BPF build:
//!
//! ```rust,ignore
//! #[cfg_attr(
//!     not(target_os = "solana"),
//!     derive(anchor_litesvm::BundledPubkeys),
//!     bundled_with(crate::test_helpers::MyBundle)
//! )]
//! #[derive(Accounts)]
//! pub struct Make<'info> { /* ... */ }
//! ```
//!
//! Step 3: populate the bundle once in setup, then build any ix:
//!
//! ```rust,ignore
//! let bundle = MyBundle { maker: maker.pubkey(), /* ... */ ..MyBundle::default() };
//! let ix = ctx.program().build_ix(bundle, instruction::Make { amount, deposit, .. });
//! ```
//!
//! The same `bundle` works for every instruction whose accounts derive
//! `BundledPubkeys` against `MyBundle` — `make`, `take`, `refund`, etc.
//! Adding an account to any `#[derive(Accounts)]` struct only requires
//! adding the field to `MyBundle`; no per-instruction builders to update.
//!
//! ### Send + Assert Shortcuts
//!
//! Most tests end in one of two shapes; the shortcuts collapse the
//! send + unwrap + assert chain into a single call. On failure, both
//! print the structured CPI tree to stderr before the underlying
//! assertion panics, so the test author sees which program frame
//! raised the error in addition to the flat-log dump.
//!
//! Use [`AnchorContext`]'s send helpers when the context owns the
//! alias table (`ctx.alias(pk, "name")` builds it up); use the bare
//! [`TransactionHelpers`] variants on `ctx.svm` when threading an
//! external alias table directly.
//!
//! ```rust,ignore
//! // Context-owned aliases: no per-call `&Aliases`. The returned
//! // TransactionResult carries the alias table, so chained
//! // `.print_logs_structured()` reads it implicitly.
//! ctx.alias(maker.pubkey(), "maker");
//! ctx.send_ok(ix, &[&maker]).print_logs_structured();
//!
//! // Expected failure: substring match against logs + the error field,
//! // same semantics as TransactionResult::assert_error. Aliases
//! // are applied to the structured tree printed when the assertion is
//! // about to fail (wrong error name, or tx unexpectedly succeeded).
//! ctx.send_err_named(ix, &[&taker], "EscrowExpired");
//!
//! // Bare LiteSVM variant: external alias table threaded per call.
//! ctx.svm.send_ok(ix, &[&maker], &aliases).print_logs_structured();
//! ```
//!
//! ## Documentation
//!
//! - [Quick Start Guide](https://github.com/cds-rs/anchor-litesvm/blob/compat/anchor-0.31/docs/QUICK_START.md)
//! - [API Reference](https://github.com/cds-rs/anchor-litesvm/blob/compat/anchor-0.31/docs/API_REFERENCE.md)
//! - [Migration Guide](https://github.com/cds-rs/anchor-litesvm/blob/compat/anchor-0.31/docs/MIGRATION.md)
//! - [Examples](https://github.com/cds-rs/anchor-litesvm/tree/compat/anchor-0.31/examples)
//!
//! ## Modules
//!
//! - [`account`] - Account deserialization utilities
//! - [`builder`] - Test environment builders
//! - [`context`] - Main test context (`AnchorContext`)
//! - [`events`] - Event parsing helpers
//! - [`instruction`] - Instruction building utilities
//! - [`program`] - Simplified Program API

pub mod account;
pub mod buildable;
pub mod builder;
pub mod context;
pub mod events;
pub mod instruction;
pub mod lazy;
pub mod program;
pub mod tx;

// Re-export main types for convenience
pub use account::{get_anchor_account, get_anchor_account_unchecked, AccountError};
pub use anchor_litesvm_derive::{AliasMirror, Bundle, BundleFrom, BundledPubkeys};
pub use buildable::BuildableIx;
pub use builder::{AnchorLiteSVM, ProgramTestExt};
pub use context::AnchorContext;
pub use events::{parse_event_data, EventError, EventHelpers};
pub use instruction::{build_anchor_instruction, calculate_anchor_discriminator};
pub use lazy::{BundleDefault, Lazy, Resolvable, Resolve, ResolveField};
pub use program::{InstructionBuilder, Program};
pub use tx::Tx;

// Re-export litesvm-utils functionality for convenience
pub use litesvm_utils::{
    actors::{deterministic_keypair, seed_bytes, ActorRegistry},
    md_kv, md_table,
    report::{MarkdownBlock, Report, ToMarkdown},
    Aliases, AssertionHelpers, LiteSVMBuilder, TestHelpers, TransactionError, TransactionHelpers,
    TransactionResult,
};

// Re-export commonly used external types
pub use anchor_lang::{AccountDeserialize, AnchorSerialize};
pub use litesvm::LiteSVM;
pub use solana_sdk::signer::keypair::Keypair;
pub use solana_program::instruction::{AccountMeta, Instruction};
pub use solana_program::pubkey::Pubkey;
pub use solana_sdk::signer::Signer;

#[cfg(test)]
mod integration_tests {
    use super::*;
    use anchor_lang::AnchorSerialize;

    /// Tiny payload type. The derive form (`#[derive(AnchorSerialize)]`)
    /// expands to `::borsh::maybestd::*` under anchor 0.31; that path
    /// only exists in borsh 0.10 and the workspace deps surface a newer
    /// borsh, so the derive doesn't resolve here. Hand-rolling
    /// `AnchorSerialize` sidesteps it.
    struct TestArgs {
        value: u64,
    }

    impl AnchorSerialize for TestArgs {
        fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
            writer.write_all(&self.value.to_le_bytes())
        }
    }

    #[test]
    fn test_full_workflow() {
        let svm = LiteSVM::new();
        let program_id = Pubkey::new_unique();
        let _ctx = AnchorContext::new(svm, program_id);

        let accounts = vec![
            AccountMeta::new(Pubkey::new_unique(), true),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
        ];

        let instruction =
            build_anchor_instruction(&program_id, "test", accounts, TestArgs { value: 42 })
                .unwrap();

        assert_eq!(instruction.program_id, program_id);
        assert!(!instruction.data.is_empty());
    }

    #[test]
    fn cast_vocabulary_funds_aliases_and_guards_uniqueness() {
        use litesvm_utils::TestHelpers;

        let program_id = Pubkey::new_unique();
        let mut ctx = AnchorContext::new(LiteSVM::new(), program_id);

        // cast_actor: deterministic, funded, aliased under its name.
        let issuer = ctx.cast_actor("issuer");
        assert_eq!(ctx.label(&issuer.pubkey()), "issuer");

        // cast_mint: a real SPL mint a holder can be funded from.
        let usdc = ctx.cast_mint("USDC", &issuer, 6);
        assert_eq!(ctx.label(&usdc), "USDC");

        // fund_ata: a holder with a balance in its ATA, aliased "<owner>/<mint>".
        let alice = ctx.cast_actor("Alice");
        let alice_usdc = ctx.fund_ata(&alice, &usdc, &issuer, 1_000_000);
        assert_eq!(ctx.label(&alice_usdc), "Alice/USDC");
        assert_eq!(ctx.svm.token_balance(&alice_usdc), Some(1_000_000));

        // cast_actor_with_sol: an exact stake, aliased like any cast.
        let whale = ctx.cast_actor_with_sol("Whale", 5_000_000_000);
        assert_eq!(ctx.label(&whale.pubkey()), "Whale");
    }

    #[test]
    #[should_panic(expected = "already used in this scenario")]
    fn cast_vocabulary_rejects_a_duplicate_name() {
        let mut ctx = AnchorContext::new(LiteSVM::new(), Pubkey::new_unique());
        ctx.cast_actor("dup");
        ctx.cast_actor("dup");
    }
}
