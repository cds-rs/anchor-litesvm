//! `#[derive(Discriminator)]`: from a plain Pinocchio instruction enum, generate
//! the on-chain dispatch artifacts the program matches on, keyed to declaration
//! order, while leaving the enum in the source for a host-side parser to read.
//!
//! This is the generative half of the IDL story. The old function-like
//! [`define_instruction_set!`](../litesvm_pinocchio/macro.define_instruction_set.html)
//! emitted the same artifacts but *replaced* the enum, so a syn-based parser saw
//! a token blob and got nothing. A derive is *additive*: the plain enum stays in
//! the source verbatim, so the companion `litesvm-pinocchio-idl` extractor reads
//! its variants, `#[account(..)]` lists, and arg types to emit a solita IDL. The
//! same declaration thus drives both the dispatch and the IDL, and their
//! discriminators cannot drift because both are the variant index.
//!
//! `#[account(..)]` is declared as an inert helper attribute: it carries the
//! per-instruction account metadata for the extractor and generates nothing, so
//! it costs zero bytes on-chain and (unlike Shank's `#[account]`, which is `std`)
//! keeps a `no_std` program building for the BPF target.
//!
//! Generated for `enum E { A, B(Args), .. }`:
//!   - `mod discriminators { pub const A: u8 = 0; pub const B: u8 = 1; .. }` (all targets)
//!   - `impl E { pub const fn discriminant(&self) -> u8 }` (all targets)
//!   - `impl E { pub fn instruction_names() -> &'static [(u8, &'static str)] }`
//!     (host only, `#[cfg(not(target_os = "solana"))]`, for the trace registry)

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

#[proc_macro_derive(Discriminator, attributes(account))]
pub fn derive_discriminator(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let variants = match &input.data {
        Data::Enum(data) => &data.variants,
        _ => {
            return syn::Error::new_spanned(
                name,
                "Discriminator can only be derived for enums (one variant per instruction)",
            )
            .to_compile_error()
            .into();
        }
    };

    if variants.len() > u8::MAX as usize + 1 {
        return syn::Error::new_spanned(
            name,
            "more than 256 variants; a u8 discriminator cannot index them",
        )
        .to_compile_error()
        .into();
    }

    let mut consts = Vec::new();
    let mut names = Vec::new();
    let mut arms = Vec::new();

    for (i, variant) in variants.iter().enumerate() {
        let vident = &variant.ident;
        let idx = i as u8;
        let vname = vident.to_string();

        consts.push(quote! {
            #[allow(non_upper_case_globals)]
            pub const #vident: u8 = #idx;
        });
        names.push(quote! { (#idx, #vname) });

        // Match the variant regardless of payload shape; the payload never
        // participates in the discriminator.
        let pattern = match &variant.fields {
            Fields::Unit => quote! { Self::#vident },
            Fields::Unnamed(_) => quote! { Self::#vident(..) },
            Fields::Named(_) => quote! { Self::#vident { .. } },
        };
        arms.push(quote! { #pattern => #idx });
    }

    quote! {
        /// One-byte discriminators, each named after its variant, for matching
        /// the leading instruction-data byte (`discriminators::Make`, etc.).
        #[allow(non_upper_case_globals)]
        pub mod discriminators {
            #(#consts)*
        }

        impl #name {
            /// Leading-byte discriminator (`data[0]`), by declaration order.
            pub const fn discriminant(&self) -> u8 {
                match self {
                    #(#arms),*
                }
            }
        }

        // Host-only: the renderer-facing name table, excluded from the SBF
        // build where it would be dead weight.
        #[cfg(not(target_os = "solana"))]
        impl #name {
            /// `(discriminator, name)` for every instruction, for the test
            /// harness's instruction registry.
            pub fn instruction_names() -> &'static [(u8, &'static str)] {
                &[ #(#names),* ]
            }
        }
    }
    .into()
}
