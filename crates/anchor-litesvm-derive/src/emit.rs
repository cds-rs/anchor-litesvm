//! Emit code from a parsed `Spec`.

use proc_macro2::TokenStream;
use quote::quote;

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

/// Parse `#[bundle(default = <expr>)]` on a bundle field, if present.
fn bundle_default_override(f: &syn::Field) -> syn::Result<Option<TokenStream>> {
    use syn::spanned::Spanned;
    let mut found = None;
    for attr in &f.attrs {
        if !attr.path().is_ident("bundle") {
            continue;
        }
        let meta: syn::Meta = attr.parse_args().map_err(|e| {
            syn::Error::new(
                attr.span(),
                format!("#[bundle(...)] on a Bundle field expects `default = <expr>`: {e}"),
            )
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
        return syn::Error::new(
            data.fields.span(),
            "#[derive(Bundle)] requires named fields",
        )
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
