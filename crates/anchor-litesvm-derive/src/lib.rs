//! Proc-macro derives for anchor-litesvm, all re-exported from
//! `anchor_litesvm`:
//!
//! - [`Bundle`] on a `Pubkey`-only struct emits a `Default` impl that
//!   fills every field with `Pubkey::new_unique()` so the bundle is
//!   ready to populate from test setup without spelling out each
//!   placeholder.
//! - [`BundleFrom`] projects a bundle from multiple source structs.
//! - [`AliasMirror`] wires a struct's fields into the `Aliases` table so
//!   rendered output reads in the test's own vocabulary.
//! - [`bundles_from_idl!`] generates per-instruction pubkey bundles from a
//!   committed Anchor IDL, paired against the `client::{accounts, args}` types
//!   `declare_program!` emits from the same file.

mod alias_mirror;
mod bundle_from;
pub(crate) mod classify;
mod emit;
pub(crate) mod idl;
mod idl_bundles;

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

/// Emit `impl Default` for a struct of `Pubkey` fields, filling every
/// field with `Pubkey::new_unique()` unless it carries
/// `#[bundle(default = <expr>)]`, which pins that field to the expression
/// (a known mint, a real program id) and deletes the hand-rolled `Default`
/// impls downstream tests used to need.
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
#[proc_macro_derive(Bundle, attributes(bundle))]
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
/// instead.
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

/// Generate per-instruction pubkey bundles from a committed Anchor IDL,
/// against the `client::{accounts, args}` types that
/// `anchor_lang::declare_program!` generates from the same file. Invoke both
/// in the same module: `bundles_from_idl!(vault)` reads `idls/vault.json`
/// (relative to the crate root, the same convention `declare_program!`
/// follows) and targets the adjacent `vault::client` module.
///
/// For each instruction `deposit`, it emits a `DepositBundle` struct (one
/// `Pubkey` field per account the caller must supply), its `Default`, a
/// `From<DepositBundle> for vault::client::accounts::Deposit` that derives the
/// instruction's PDAs and injects its fixed addresses, an
/// `impl BuildableIx<DepositBundle> for vault::client::args::Deposit`, and a
/// `<account>_pda(...)` helper per derivable PDA. One module-level
/// `injected_programs()` lists every fixed address the IDL pins.
///
/// A second argument overrides the IDL path:
/// `bundles_from_idl!(vault, "fixtures/vault.json")`.
///
/// # Foreign-program PDAs
///
/// A PDA usually derives under the IDL's own program, but Anchor emits a
/// `pda.program` for every `associated_token::` account: an ATA derives under
/// the associated-token program, not the program the IDL describes. Both the
/// `From` impl and the `<account>_pda` helper honor that program (the derivation
/// runs under the const program's raw bytes, or under another account when the
/// IDL names one). A `pda.program` the macro can't resolve to a pubkey at build
/// time (an instruction arg, an account-data path, or a const that isn't a
/// 32-byte program id) demotes the account to a caller-supplied bundle field,
/// the same rule an unresolvable seed follows.
///
/// # One program per module
///
/// `PROGRAM_ID` and `injected_programs()` are free items emitted at module
/// scope, so two `bundles_from_idl!` invocations in the same module collide
/// (`E0428: the name `PROGRAM_ID` is defined multiple times`). This mirrors
/// `declare_program!`, which emits its own module-level items per invocation
/// too. Testing two programs in the same file, wrap each pair of
/// `declare_program!` + `bundles_from_idl!` calls in its own `mod`:
///
/// ```ignore
/// mod escrow_ixs {
///     anchor_lang::declare_program!(escrow);
///     anchor_litesvm::bundles_from_idl!(escrow);
/// }
/// mod vault_ixs {
///     anchor_lang::declare_program!(vault);
///     anchor_litesvm::bundles_from_idl!(vault);
/// }
/// ```
#[proc_macro]
pub fn bundles_from_idl(input: TokenStream) -> TokenStream {
    match idl_bundles::expand(input.into()) {
        Ok(ts) => ts.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
