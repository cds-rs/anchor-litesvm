//! Emit code from a parsed `Spec`.

use crate::parse::{FieldSource, Spec};
use proc_macro2::TokenStream;
use quote::quote;

pub fn emit(spec: &Spec) -> TokenStream {
    let from_impl = emit_from_impl(spec);
    let buildable_impl = emit_buildable_impl(spec);
    let injected = emit_injected_programs(spec);
    quote! {
        #from_impl
        #buildable_impl
        #injected
    }
}

/// Emit a host-only `injected_programs()` table on the accounts struct: one
/// `(id, name)` pair per field the structural rule classified, so injected
/// programs name themselves (the table feeds the alias layer the way the
/// Discriminator tables feed `register_program_instructions`). Nothing is
/// emitted when no field was rule-classified.
fn emit_injected_programs(spec: &Spec) -> TokenStream {
    let entries: Vec<TokenStream> = spec
        .fields
        .iter()
        .filter_map(|f| {
            let name = f.injected_name.as_deref()?;
            let FieldSource::Const(expr) = &f.source else {
                return None;
            };
            Some(quote! { (#expr, #name) })
        })
        .collect();
    if entries.is_empty() {
        return quote!();
    }
    let ident = &spec.accounts_ident;
    let (impl_generics, ty_generics, where_clause) = spec.generics.split_for_impl();
    quote! {
        impl #impl_generics #ident #ty_generics #where_clause {
            /// The programs the `BundledPubkeys` structural rule injects into
            /// this accounts struct, as `(id, name)` pairs. Host-only; feed it
            /// to the alias layer (e.g. `ctx.alias_programs(...)`) so injected
            /// programs render named with zero registration.
            #[cfg(not(target_os = "solana"))]
            pub fn injected_programs() -> ::std::vec::Vec<(anchor_lang::prelude::Pubkey, &'static str)> {
                ::std::vec![ #(#entries),* ]
            }
        }
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
/// Every field is filled with `Pubkey::new_unique()` unless it carries
/// `#[bundle(default = <expr>)]`, in which case the expression is used. The
/// per-field override is what deletes hand-rolled `Default` impls downstream
/// (a known mint, a real program id) while unannotated fields keep the
/// fail-loudly placeholder semantics. We do not inspect the field's type; if
/// the user has a non-Pubkey field, `new_unique()` won't compile and they
/// can hand-write Default instead. That trade-off keeps this derive tiny and
/// the failure mode obvious.
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
    let mut assignments = Vec::new();
    for f in &named.named {
        let fname = f.ident.as_ref().expect("named");
        match bundle_default_override(f) {
            Ok(Some(expr)) => assignments.push(quote! { #fname: #expr }),
            Ok(None) => assignments
                .push(quote! { #fname: ::anchor_litesvm::BundleDefault::bundle_default() }),
            Err(e) => return e.to_compile_error(),
        }
    }
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

/// Parse `#[bundle(default = <expr>)]` on a bundle field, if present. The
/// `bundle` helper-attribute namespace is shared with `BundledPubkeys`'
/// field attributes, but the two derives sit on different structs (accounts
/// vs bundle), so each parser owns its keys; unknown keys are compile errors.
fn bundle_default_override(f: &syn::Field) -> syn::Result<Option<TokenStream>> {
    use syn::spanned::Spanned;
    let mut found = None;
    for attr in &f.attrs {
        if !attr.path().is_ident("bundle") {
            continue;
        }
        let meta: syn::Meta = attr.parse_args().map_err(|e| {
            syn::Error::new(attr.span(), format!("#[bundle(...)] on a Bundle field expects `default = <expr>`: {e}"))
        })?;
        match &meta {
            syn::Meta::NameValue(nv) if nv.path.is_ident("default") => {
                if found.is_some() {
                    return Err(syn::Error::new(
                        attr.span(),
                        "duplicate #[bundle(default = ...)] on the same field",
                    ));
                }
                let expr = &nv.value;
                found = Some(quote! { #expr });
            }
            other => {
                return Err(syn::Error::new(
                    attr.span(),
                    format!(
                        "unknown `#[bundle({})]` on a Bundle field; expected `default = <expr>`",
                        quote!(#other)
                    ),
                ));
            }
        }
    }
    Ok(found)
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
