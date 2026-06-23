//! `#[derive(AliasMirror)]`: emit `Self::alias_all(&self, ctx: &mut AnchorContext)`
//! that registers every `Pubkey` field in the alias table under a
//! label derived from the field name (or an explicit override).
//!
//! Motivating pattern: a `Pool` fixture carries 5+ PDA pubkeys; tests
//! manually call `world.alias(pool.config, "Pool")` etc. for each one
//! at setup time, so structured-log frames show friendly names. The
//! derive moves that ceremony to one line:
//!
//! ```ignore
//! pool.alias_all(&mut world.ctx);
//! ```
//!
//! ## Rules
//!
//! - Fields whose textual type is `Pubkey` are aliased; everything
//!   else is silently skipped. The check is textual on the last path
//!   segment, same approach as `parse::classify_field_type`.
//! - Default label = PascalCase of the field name (`vault_x` → `VaultX`,
//!   `lp_vault` → `LpVault`).
//! - `#[alias("CustomName")]` overrides the label.
//! - `#[alias(skip)]` omits a Pubkey field that the user doesn't want
//!   labelled.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{spanned::Spanned, DeriveInput, Error, Result};

pub fn derive(input: DeriveInput) -> Result<TokenStream> {
    let spec = parse_spec(&input)?;
    Ok(emit(&spec))
}

#[derive(Debug)]
struct Spec {
    struct_ident: syn::Ident,
    /// Only the fields that should be aliased; non-Pubkey and skipped
    /// fields are filtered out at parse time.
    entries: Vec<Entry>,
}

#[derive(Debug)]
struct Entry {
    field: syn::Ident,
    label: String,
}

fn parse_spec(input: &DeriveInput) -> Result<Spec> {
    let struct_ident = input.ident.clone();
    let syn::Data::Struct(data) = &input.data else {
        return Err(Error::new(
            input.span(),
            "#[derive(AliasMirror)] only supports structs",
        ));
    };
    let syn::Fields::Named(named) = &data.fields else {
        return Err(Error::new(
            data.fields.span(),
            "#[derive(AliasMirror)] requires named fields",
        ));
    };
    let mut entries = Vec::new();
    for field in &named.named {
        let name = field
            .ident
            .clone()
            .ok_or_else(|| Error::new(field.span(), "field must be named"))?;
        match extract_alias_attr(field)? {
            AliasDecision::Skip => continue,
            AliasDecision::Label(label) => {
                entries.push(Entry { field: name, label });
            }
            AliasDecision::Default => {
                if !is_pubkey_type(&field.ty) {
                    // Non-Pubkey + no explicit alias = silent skip.
                    continue;
                }
                entries.push(Entry {
                    label: pascal_case(&name.to_string()),
                    field: name,
                });
            }
        }
    }
    Ok(Spec {
        struct_ident,
        entries,
    })
}

enum AliasDecision {
    /// No `#[alias(...)]` attr; use the default (PascalCase if Pubkey,
    /// else skip).
    Default,
    /// `#[alias(skip)]` — omit regardless of type.
    Skip,
    /// `#[alias("Custom")]` — use the given label regardless of type.
    Label(String),
}

fn extract_alias_attr(field: &syn::Field) -> Result<AliasDecision> {
    let mut found: Option<AliasDecision> = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("alias") {
            continue;
        }
        let decision = attr.parse_args_with(parse_alias_args)?;
        if found.is_some() {
            return Err(Error::new(attr.span(), "duplicate #[alias(...)] on field"));
        }
        found = Some(decision);
    }
    Ok(found.unwrap_or(AliasDecision::Default))
}

fn parse_alias_args(input: syn::parse::ParseStream) -> Result<AliasDecision> {
    // `#[alias(skip)]` — bare ident.
    if input.peek(syn::Ident) {
        let ident: syn::Ident = input.parse()?;
        if ident == "skip" {
            return Ok(AliasDecision::Skip);
        }
        return Err(Error::new(
            ident.span(),
            "expected `skip` or a string literal, e.g. `#[alias(\"MyLabel\")]`",
        ));
    }
    // `#[alias("Label")]` — string literal.
    let lit: syn::LitStr = input.parse()?;
    Ok(AliasDecision::Label(lit.value()))
}

/// Textual match: the field type's last path segment is exactly `Pubkey`.
/// `anchor_lang::prelude::Pubkey`, `solana_program::pubkey::Pubkey`,
/// and bare `Pubkey` all resolve to the same answer.
fn is_pubkey_type(ty: &syn::Type) -> bool {
    let syn::Type::Path(tp) = ty else {
        return false;
    };
    tp.path
        .segments
        .last()
        .is_some_and(|s| s.ident == "Pubkey" && matches!(s.arguments, syn::PathArguments::None))
}

/// `vault_x` → `VaultX`, `lp_vault` → `LpVault`, `mint` → `Mint`.
fn pascal_case(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut capitalize_next = true;
    for ch in snake.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            out.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn emit(spec: &Spec) -> TokenStream {
    let name = &spec.struct_ident;
    let aliasing = spec.entries.iter().map(|e| {
        let field = &e.field;
        let label = &e.label;
        quote! { .alias(self.#field, #label) }
    });
    quote! {
        impl #name {
            /// Register every `Pubkey` field with a friendly label in
            /// the context's alias table. Returns the context for
            /// chaining. Generated by `#[derive(AliasMirror)]`.
            pub fn alias_all<'__a>(
                &self,
                ctx: &'__a mut ::anchor_litesvm::AnchorContext,
            ) -> &'__a mut ::anchor_litesvm::AnchorContext {
                ctx #(#aliasing)*
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn pascal_case_simple() {
        assert_eq!(pascal_case("mint"), "Mint");
        assert_eq!(pascal_case("vault_x"), "VaultX");
        assert_eq!(pascal_case("lp_vault"), "LpVault");
        assert_eq!(pascal_case("a_b_c"), "ABC");
    }

    #[test]
    fn pubkey_type_match() {
        let t: syn::Type = parse_quote!(Pubkey);
        assert!(is_pubkey_type(&t));
        let t: syn::Type = parse_quote!(anchor_lang::prelude::Pubkey);
        assert!(is_pubkey_type(&t));
        let t: syn::Type = parse_quote!(u64);
        assert!(!is_pubkey_type(&t));
        // Generic Pubkey<T> doesn't match (no such type, but guard the
        // path-args check).
        let t: syn::Type = parse_quote!(Option<Pubkey>);
        assert!(!is_pubkey_type(&t));
    }

    #[test]
    fn skips_non_pubkey_fields() {
        let input: DeriveInput = parse_quote! {
            struct Pool {
                pub seed: u64,
                pub mint_x: Pubkey,
            }
        };
        let spec = parse_spec(&input).expect("parse");
        assert_eq!(spec.entries.len(), 1);
        assert_eq!(spec.entries[0].field, "mint_x");
        assert_eq!(spec.entries[0].label, "MintX");
    }

    #[test]
    fn label_override_wins() {
        let input: DeriveInput = parse_quote! {
            struct Pool {
                #[alias("LP Vault")]
                pub lp_vault: Pubkey,
            }
        };
        let spec = parse_spec(&input).expect("parse");
        assert_eq!(spec.entries[0].label, "LP Vault");
    }

    #[test]
    fn skip_attr_omits_pubkey_field() {
        let input: DeriveInput = parse_quote! {
            struct Pool {
                #[alias(skip)]
                pub debug_only: Pubkey,
                pub config: Pubkey,
            }
        };
        let spec = parse_spec(&input).expect("parse");
        assert_eq!(spec.entries.len(), 1);
        assert_eq!(spec.entries[0].field, "config");
    }

    #[test]
    fn label_attr_overrides_non_pubkey_skip() {
        // Explicit label forces aliasing even if the type isn't
        // textually Pubkey (lets the user opt non-Pubkey-but-pubkey-like
        // types in if they have to; rustc will reject a bad type at
        // the emitted impl).
        let input: DeriveInput = parse_quote! {
            struct Pool {
                #[alias("Strange")]
                pub weird: u64,
            }
        };
        let spec = parse_spec(&input).expect("parse");
        assert_eq!(spec.entries.len(), 1);
        assert_eq!(spec.entries[0].label, "Strange");
    }

    #[test]
    fn errors_on_duplicate_alias_attr() {
        let input: DeriveInput = parse_quote! {
            struct Pool {
                #[alias("A")]
                #[alias("B")]
                pub config: Pubkey,
            }
        };
        let err = parse_spec(&input).expect_err("must error");
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn errors_on_unknown_ident() {
        let input: DeriveInput = parse_quote! {
            struct Pool {
                #[alias(maybe)]
                pub config: Pubkey,
            }
        };
        let err = parse_spec(&input).expect_err("must error");
        assert!(err.to_string().contains("expected"));
    }
}
