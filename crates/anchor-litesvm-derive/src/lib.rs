//! Proc-macro derives for anchor-litesvm, all re-exported from
//! `anchor_litesvm`:
//!
//! - [`BundledPubkeys`] on a `#[derive(Accounts)]` struct emits
//!   `From<Bundle> for accounts::*` and
//!   `BuildableIx<Bundle> for instruction::*`. Lets tests collapse the
//!   per-ix `.accounts(...).args(...).instruction()` chain into a single
//!   `ctx.program().build_ix(bundle, args)` call. Design:
//!   `docs/design/bundled-pubkeys.md`.
//! - [`Bundle`] on a `Pubkey`-only struct emits a `Default` impl that
//!   fills every field with `Pubkey::new_unique()` so the bundle is
//!   ready to populate from test setup without spelling out each
//!   placeholder.
//! - [`BundleFrom`] projects a bundle from multiple source structs.
//! - [`AliasMirror`] wires a struct's fields into the `Aliases` table so
//!   rendered output reads in the test's own vocabulary.

mod alias_mirror;
mod bundle_from;
mod emit;
mod parse;

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

/// Wire an Anchor `#[derive(Accounts)]` struct to a caller-supplied
/// pubkey bundle, so tests can build instructions with one call instead
/// of hand-filling `accounts::* { ... }` + `instruction::* { ... }`.
///
/// The derive emits two impls per Accounts struct it's attached to:
///
/// - `impl From<Bundle> for crate::accounts::<StructName>` projects
///   each field from the bundle, auto-injecting canonical program IDs
///   for `Program<'_, System>`, `Program<'_, AssociatedToken>`, and
///   `Interface<'_, TokenInterface>` so the bundle doesn't have to
///   carry them.
/// - `impl BuildableIx<Bundle> for crate::instruction::<StructName>`
///   pairs the args struct with its accounts struct at the type level.
///   `Program::build_ix(bundle, args)` consumes both and emits an
///   `Instruction`, with compile-time pairing: passing `Deposit` args
///   with `Withdraw` accounts is a type error, not a runtime failure.
///
/// # Attributes
///
/// **`#[bundled_with(BundlePath, instruction = path, accounts = path)]`** is
/// required. The first positional argument is the bundle type; the
/// optional `instruction =` / `accounts =` keyword arguments override
/// the inferred `crate::instruction::<StructName>` / `crate::accounts::<StructName>`
/// paths, which is needed when Anchor's naming diverges from the
/// Accounts struct's name (e.g. `fn initialize_poll` paired with
/// `struct InitPoll`, where Anchor names `instruction::InitializePoll`
/// from the handler, not the struct).
///
/// **`#[bundle(unwrap)]` / `#[bundle(wrap_some)]`** are optional per-field
/// attributes for shape fixups, used when one bundle is shared across
/// accounts structs that disagree on a field's optionality. `unwrap`
/// projects an `Option<T>` bundle field into a bare `T` account field
/// (`b.field.expect(...)`, panicking with a pointed message if `None`);
/// `wrap_some` does the reverse (`Some(b.field)`). Without them, a
/// type mismatch between bundle and accounts field is a compile error.
///
/// # Example
///
/// You write:
///
/// ```ignore
/// use anchor_lang::prelude::*;
/// use anchor_litesvm::{Bundle, BundledPubkeys};
///
/// // 1. A host-only bundle struct holding the pubkeys your tests will
/// //    populate. Omit Program<System>, Program<AssociatedToken>, and
/// //    Interface<TokenInterface>; the derive auto-injects canonical
/// //    IDs for those.
/// #[cfg(not(target_os = "solana"))]
/// pub mod test_helpers {
///     use super::*;
///
///     #[derive(Bundle, Copy, Clone, Debug)]
///     pub struct EscrowBundle {
///         pub maker: Pubkey,
///         pub mint_a: Pubkey,
///         pub vault: Pubkey,
///         pub escrow: Pubkey,
///     }
/// }
///
/// // 2. The derive on each #[derive(Accounts)] struct, gated for
/// //    non-Solana so it doesn't pull into the on-chain BPF build.
/// #[cfg_attr(
///     not(target_os = "solana"),
///     derive(anchor_litesvm::BundledPubkeys),
///     bundled_with(crate::test_helpers::EscrowBundle),
/// )]
/// #[derive(Accounts)]
/// pub struct Make<'info> {
///     #[account(mut)] pub maker: Signer<'info>,
///     pub mint_a: InterfaceAccount<'info, Mint>,
///     #[account(mut)] pub vault: InterfaceAccount<'info, TokenAccount>,
///     pub escrow: Account<'info, Escrow>,
///     pub token_program: Interface<'info, TokenInterface>,
///     pub system_program: Program<'info, System>,
/// }
/// ```
///
/// The derive emits (roughly, as if you'd hand-written it):
///
/// ```ignore
/// impl From<EscrowBundle> for crate::accounts::Make {
///     fn from(b: EscrowBundle) -> Self {
///         Self {
///             maker: b.maker,
///             mint_a: b.mint_a,
///             vault: b.vault,
///             escrow: b.escrow,
///             // Auto-injected from field types in the Accounts struct:
///             token_program: anchor_spl::token::ID,
///             system_program: anchor_lang::solana_program::system_program::ID,
///         }
///     }
/// }
///
/// impl ::anchor_litesvm::BuildableIx<EscrowBundle> for crate::instruction::Make {
///     type Accounts = crate::accounts::Make;
/// }
/// ```
///
/// So in a test you write:
///
/// ```ignore
/// let bundle = EscrowBundle { maker: maker.pubkey(), /* ... */ };
/// let ix = ctx.program().build_ix(bundle, instruction::Make { amount: 1_000 });
/// ```
///
/// # See also
///
/// - `crates/anchor-litesvm/src/buildable.rs`: the `BuildableIx` trait
///   the derive plugs into.
#[proc_macro_derive(BundledPubkeys, attributes(bundled_with, bundle))]
pub fn derive_bundled_pubkeys(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match parse::parse(input) {
        Ok(spec) => emit::emit(&spec).into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Emit `impl Default` for a struct of `Pubkey` fields, filling every
/// field with `Pubkey::new_unique()`.
///
/// Pubkey bundles in tests want valid-looking placeholders for fields
/// the test hasn't bound yet (so `struct update` syntax can fill in the
/// ones that matter). The standard `#[derive(Default)]` gives
/// `Pubkey::default()`, which is the all-zeros address; that gets
/// rejected by virtually every Solana program. `Pubkey::new_unique()`
/// produces a fresh address per field, which the runtime will treat as
/// a non-existent account (the expected failure mode for a placeholder).
///
/// # Caveats
///
/// - Don't also `#[derive(Default)]` on the same struct; that's a
///   duplicate impl error.
/// - The derive doesn't inspect field types. Non-`Pubkey` fields will
///   trip a "the trait `Default` is not implemented for ..." error on
///   `Pubkey::new_unique()`; hand-write `Default` instead.
///
/// # Example
///
/// You write:
///
/// ```ignore
/// use anchor_lang::prelude::Pubkey;
/// use anchor_litesvm::Bundle;
///
/// #[derive(Bundle, Copy, Clone, Debug)]
/// pub struct EscrowBundle {
///     pub maker: Pubkey,
///     pub vault: Pubkey,
///     pub escrow: Pubkey,
/// }
/// ```
///
/// The derive emits (roughly, as if you'd hand-written it):
///
/// ```ignore
/// impl Default for EscrowBundle {
///     fn default() -> Self {
///         Self {
///             maker: ::anchor_lang::prelude::Pubkey::new_unique(),
///             vault: ::anchor_lang::prelude::Pubkey::new_unique(),
///             escrow: ::anchor_lang::prelude::Pubkey::new_unique(),
///         }
///     }
/// }
/// ```
///
/// So in a test, bind only what matters; the rest get fresh placeholders:
///
/// ```ignore
/// let bundle = EscrowBundle {
///     maker: maker.pubkey(),
///     ..EscrowBundle::default()
/// };
/// ```
#[proc_macro_derive(Bundle)]
pub fn derive_bundle(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let default = emit::emit_bundle_default(&input);
    let resolvable = emit::emit_bundle_resolvable(&input);
    quote::quote! { #default #resolvable }.into()
}

/// Emit `From<(&T1, &T2, ...)> for Self` for a `Bundle` struct whose
/// fields come from multiple upstream fixtures.
///
/// Motivating pattern: a test scenario has a shared fixture (e.g.
/// `Pool` carrying PDAs) plus a per-actor fixture (e.g. `UserAccounts`
/// carrying signers/ATAs), and every per-ix `Bundle` is a hand-rolled
/// projection that merges fields from both. With this derive, the
/// projection is declared once on the bundle:
///
/// ```ignore
/// use anchor_lang::prelude::Pubkey;
/// use anchor_litesvm::{Bundle, BundleFrom};
///
/// pub struct Pool { pub mint_x: Pubkey, pub config: Pubkey, pub vault_x: Pubkey }
/// pub struct UserAccounts { pub signer: solana_sdk::signature::Keypair, pub ata_x: Pubkey }
/// impl UserAccounts { pub fn pubkey(&self) -> Pubkey { /* ... */ Pubkey::default() } }
///
/// #[derive(Copy, Clone, Bundle, BundleFrom)]
/// #[from_fixtures(p: Pool, u: UserAccounts)]
/// pub struct SwapBundle {
///     #[from(u.pubkey())]
///     pub user: Pubkey,
///     pub mint_x: Pubkey,    // auto: p.mint_x
///     pub config: Pubkey,    // auto: p.config
///     pub vault_x: Pubkey,   // auto: p.vault_x
///     #[from(u.ata_x)]
///     pub user_x: Pubkey,
/// }
/// ```
///
/// Then in a test: `let bundle = SwapBundle::from((&pool, &user));`.
///
/// # Attributes
///
/// **`#[from_fixtures(name1: Type1, name2: Type2, ...)]`** is required.
/// Declare 2..N bindings; the names are used inside `#[from(...)]`
/// expressions. Single-source bundles should use a hand-written `From`
/// or the upstream `BundledPubkeys` projection instead.
///
/// **`#[from(expr)]`** on a field overrides the auto-projection with
/// any Rust expression that can reference the bound names. Use this
/// when the field name on the bundle doesn't match the source field,
/// when the value needs a method call, or when it needs a computation.
///
/// # Auto-projection rule
///
/// Bare fields (no `#[from(...)]`) project as
/// `<first_binding>.<field_name>`. Put your primary fixture first;
/// annotate the rest. Proc-macros can't introspect source structs, so
/// if a bare field doesn't exist on the first fixture, you'll get a
/// rustc field-not-found error at the generated impl — that's the
/// signal to add an override.
#[proc_macro_derive(BundleFrom, attributes(from_fixtures, from))]
pub fn derive_bundle_from(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match bundle_from::derive(input) {
        Ok(ts) => ts.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Emit `Self::alias_all(&self, ctx: &mut AnchorContext)` that registers
/// every `Pubkey` field in the context's alias table under a label
/// derived from the field name.
///
/// Motivating pattern: a `Pool` fixture carries 5+ PDA pubkeys; tests
/// manually call `world.alias(pool.config, "Pool")` etc. for each one
/// at setup time. The derive collapses that to one call.
///
/// ```ignore
/// use anchor_lang::prelude::Pubkey;
/// use anchor_litesvm::AliasMirror;
///
/// #[derive(Copy, Clone, AliasMirror)]
/// pub struct Pool {
///     pub seed: u64,            // skipped (not Pubkey)
///     pub mint_x: Pubkey,       // → "MintX"
///     pub mint_y: Pubkey,       // → "MintY"
///     pub config: Pubkey,       // → "Config"
///     #[alias("LpVault")]
///     pub lp_vault: Pubkey,     // explicit label
///     #[alias(skip)]
///     pub debug_only: Pubkey,   // not aliased
/// }
///
/// // In a test:
/// // pool.alias_all(&mut ctx);
/// ```
///
/// # Rules
///
/// - Default label: PascalCase of the field name (`vault_x` →
///   `"VaultX"`, `lp_vault` → `"LpVault"`).
/// - Non-`Pubkey` fields are silently skipped (textual type check on
///   the last path segment).
/// - `#[alias("CustomName")]` overrides the label.
/// - `#[alias(skip)]` omits a `Pubkey` field entirely.
/// - The generated method returns `&mut AnchorContext` for chaining.
#[proc_macro_derive(AliasMirror, attributes(alias))]
pub fn derive_alias_mirror(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match alias_mirror::derive(input) {
        Ok(ts) => ts.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
