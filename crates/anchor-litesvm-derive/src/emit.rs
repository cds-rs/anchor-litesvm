//! Emit code from a parsed `Spec`.

use crate::parse::{FieldSource, Spec};
use proc_macro2::TokenStream;
use quote::quote;

pub fn emit(spec: &Spec) -> TokenStream {
    let from_impl = emit_from_impl(spec);
    let buildable_impl = emit_buildable_impl(spec);
    quote! {
        #from_impl
        #buildable_impl
    }
}

/// Token stream for the `accounts::*` type the derive emits impls against.
/// Uses the `#[bundled_with(..., accounts = path)]` override when set,
/// otherwise falls back to `crate::accounts::#accounts_ident`. The fallback
/// path inherits its span from `#accounts_ident`, so resolution failures
/// attribute back to the struct definition site (not the derive call).
fn accounts_target(spec: &Spec) -> TokenStream {
    match &spec.accounts_path {
        Some(path) => quote!(#path),
        None => {
            let id = &spec.accounts_ident;
            quote!(crate::accounts::#id)
        }
    }
}

/// Token stream for the `instruction::*` type the derive emits impls against.
/// Uses the `#[bundled_with(..., instruction = path)]` override when set,
/// otherwise falls back to `crate::instruction::#accounts_ident`. Same span
/// inheritance as [`accounts_target`].
fn instruction_target(spec: &Spec) -> TokenStream {
    match &spec.instruction_path {
        Some(path) => quote!(#path),
        None => {
            let id = &spec.accounts_ident;
            quote!(crate::instruction::#id)
        }
    }
}

fn emit_from_impl(spec: &Spec) -> TokenStream {
    let bundle = &spec.bundle_path;
    let target = accounts_target(spec);
    let accounts_name = spec.accounts_ident.to_string();
    let assignments = spec.fields.iter().map(|f| {
        let name = &f.name;
        match &f.source {
            FieldSource::Const(expr) => quote!(#name: #expr),
            FieldSource::Project => quote!(#name: ::core::convert::Into::into(b.#name)),
            FieldSource::ProjectUnwrap => {
                let field_str = name.to_string();
                let msg = format!(
                    "bundle field `{field_str}` is None, but accounts::{accounts_name}.{field_str} requires Some(_); set the bundle field before building this instruction"
                );
                quote!(#name: ::core::option::Option::expect(b.#name, #msg))
            }
            FieldSource::ProjectWrapSome => {
                quote!(#name: ::core::option::Option::Some(b.#name))
            }
        }
    });
    quote! {
        impl ::core::convert::From<#bundle> for #target {
            fn from(b: #bundle) -> Self {
                Self {
                    #(#assignments,)*
                }
            }
        }
    }
}

fn emit_buildable_impl(spec: &Spec) -> TokenStream {
    let bundle = &spec.bundle_path;
    let instruction = instruction_target(spec);
    let accounts = accounts_target(spec);
    quote! {
        impl ::anchor_litesvm::BuildableIx<#bundle> for #instruction {
            type Accounts = #accounts;
        }
    }
}

/// Emit `impl Default for Bundle { fn default() -> Self { ... Pubkey::new_unique() ... } }`.
///
/// Every field is filled with `Pubkey::new_unique()`. We do not inspect
/// the field's type; if the user has a non-Pubkey field, `new_unique()`
/// won't compile and they can hand-write Default instead. That trade-off
/// keeps this derive tiny and the failure mode obvious.
pub fn emit_bundle_default(input: &syn::DeriveInput) -> TokenStream {
    use syn::spanned::Spanned;
    let name = &input.ident;
    let syn::Data::Struct(data) = &input.data else {
        return syn::Error::new(input.span(), "#[derive(Bundle)] only supports structs")
            .to_compile_error();
    };
    let syn::Fields::Named(named) = &data.fields else {
        return syn::Error::new(
            data.fields.span(),
            "#[derive(Bundle)] requires named fields",
        )
        .to_compile_error();
    };
    let assignments = named.named.iter().map(|f| {
        let name = f.ident.as_ref().expect("named");
        quote! { #name: ::anchor_litesvm::BundleDefault::bundle_default() }
    });
    quote! {
        impl ::core::default::Default for #name {
            fn default() -> Self {
                Self {
                    #(#assignments,)*
                }
            }
        }
    }
}

/// Emit `impl Resolvable for Bundle`: call `resolve_field` on every field, so
/// `Lazy` fields resolve against the SVM and `Pubkey` fields are a no-op. Build
/// runs this before projecting the bundle onto account metas.
pub fn emit_bundle_resolvable(input: &syn::DeriveInput) -> TokenStream {
    use syn::spanned::Spanned;
    let name = &input.ident;
    let syn::Data::Struct(data) = &input.data else {
        return syn::Error::new(input.span(), "#[derive(Bundle)] only supports structs")
            .to_compile_error();
    };
    let syn::Fields::Named(named) = &data.fields else {
        return syn::Error::new(data.fields.span(), "#[derive(Bundle)] requires named fields")
            .to_compile_error();
    };
    let calls = named.named.iter().map(|f| {
        let fname = f.ident.as_ref().expect("named");
        quote! { ::anchor_litesvm::ResolveField::resolve_field(&mut self.#fname, ctx); }
    });
    quote! {
        impl ::anchor_litesvm::Resolvable for #name {
            fn resolve_all(&mut self, ctx: &::anchor_litesvm::AnchorContext) {
                #(#calls)*
            }
        }
    }
}
