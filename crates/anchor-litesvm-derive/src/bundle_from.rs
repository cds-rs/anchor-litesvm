//! `#[derive(BundleFrom)]`: emit `From<(&T1, &T2, ...)> for Bundle` for
//! bundle structs whose fields come from a tuple of upstream fixture
//! types.
//!
//! Motivating pattern (from `~/sol/02-amm`): a `Pool` fixture holds
//! shared PDAs, a `UserAccounts` fixture holds per-actor signers/ATAs,
//! and every per-ix `Bundle` is a hand-projection of fields from both.
//! With this derive, the projection is declared once on the bundle:
//!
//! ```ignore
//! #[derive(BundleFrom)]
//! #[from_fixtures(p: Pool, u: UserAccounts)]
//! pub struct SwapBundle {
//!     #[from(u.pubkey())]
//!     pub user: Pubkey,
//!     pub mint_x: Pubkey,   // auto: p.mint_x
//!     #[from(u.ata_x)]
//!     pub user_x: Pubkey,
//! }
//! ```
//!
//! The default projection rule is **first declared fixture wins**:
//! a bare field (no `#[from(...)]`) projects from `<first_binding>.<field_name>`.
//! Put your primary fixture first; use `#[from(other.expr)]` for the
//! rest. Proc-macros can't read the source structs at expansion time
//! (only the type names are visible), so we can't auto-pick "the
//! source that has this field" — if a bare field doesn't exist on the
//! first fixture, you'll get a rustc field-not-found error at the
//! emitted `From` impl, pointing at the field by name. That's the
//! signal to add a `#[from(...)]` override.
//!
//! Overrides accept any Rust expression that can reference the bound
//! names (`#[from(u.pubkey())]`, `#[from(u.ata_lp(&p.mint_lp))]`,
//! `#[from(Pubkey::find_program_address(&[...], &id).0)]`, etc.).

use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
    DeriveInput, Error, Expr, Result, Token,
};

pub fn derive(input: DeriveInput) -> Result<TokenStream> {
    let spec = parse_spec(&input)?;
    Ok(emit(&spec))
}

#[derive(Debug)]
struct Spec {
    bundle_ident: syn::Ident,
    /// `(name, type)` for each fixture binding, in declaration order.
    fixtures: Vec<(syn::Ident, syn::Type)>,
    fields: Vec<Field>,
}

#[derive(Debug)]
struct Field {
    name: syn::Ident,
    source: FieldSource,
}

#[derive(Debug)]
enum FieldSource {
    /// Auto-projected: use `<binding>.<field_name>` where `binding` is
    /// the unique fixture whose type has this field.
    Project(syn::Ident),
    /// User-supplied expression (`u.pubkey()`, `u.ata_lp(&p.mint_lp)`,
    /// etc.).
    Expr(Expr),
}

fn parse_spec(input: &DeriveInput) -> Result<Spec> {
    let bundle_ident = input.ident.clone();
    let fixtures = extract_fixtures(input)?;
    let fields = extract_fields(input, &fixtures)?;
    Ok(Spec {
        bundle_ident,
        fixtures,
        fields,
    })
}

struct FixtureBindings(Punctuated<FixtureBinding, Token![,]>);

struct FixtureBinding {
    name: syn::Ident,
    ty: syn::Type,
}

impl Parse for FixtureBindings {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(Self(Punctuated::parse_terminated(input)?))
    }
}

impl Parse for FixtureBinding {
    fn parse(input: ParseStream) -> Result<Self> {
        let name: syn::Ident = input.parse()?;
        let _: Token![:] = input.parse()?;
        let ty: syn::Type = input.parse()?;
        Ok(Self { name, ty })
    }
}

fn extract_fixtures(input: &DeriveInput) -> Result<Vec<(syn::Ident, syn::Type)>> {
    let mut found: Option<Vec<(syn::Ident, syn::Type)>> = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("from_fixtures") {
            continue;
        }
        let bindings: FixtureBindings = attr.parse_args()?;
        let parsed: Vec<_> = bindings.0.into_iter().map(|b| (b.name, b.ty)).collect();
        if parsed.len() < 2 {
            return Err(Error::new(
                attr.span(),
                "#[from_fixtures(...)] requires at least 2 bindings; \
                 use #[derive(BundledPubkeys)] or a hand-written From for the single-source case",
            ));
        }
        if found.is_some() {
            return Err(Error::new(attr.span(), "duplicate #[from_fixtures]"));
        }
        found = Some(parsed);
    }
    found.ok_or_else(|| {
        Error::new(
            input.ident.span(),
            "missing #[from_fixtures(name: Type, ...)] - required by #[derive(BundleFrom)]",
        )
    })
}

fn extract_fields(
    input: &DeriveInput,
    fixtures: &[(syn::Ident, syn::Type)],
) -> Result<Vec<Field>> {
    let syn::Data::Struct(data) = &input.data else {
        return Err(Error::new(
            input.span(),
            "#[derive(BundleFrom)] only supports structs",
        ));
    };
    let syn::Fields::Named(named) = &data.fields else {
        return Err(Error::new(
            data.fields.span(),
            "#[derive(BundleFrom)] requires named fields",
        ));
    };
    let mut out = Vec::with_capacity(named.named.len());
    for field in &named.named {
        let name = field
            .ident
            .clone()
            .ok_or_else(|| Error::new(field.span(), "field must be named"))?;
        let source = if let Some(expr) = extract_from_attr(field)? {
            FieldSource::Expr(expr)
        } else {
            FieldSource::Project(infer_source(&name, fixtures)?)
        };
        out.push(Field { name, source });
    }
    Ok(out)
}

fn extract_from_attr(field: &syn::Field) -> Result<Option<Expr>> {
    let mut found: Option<Expr> = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("from") {
            continue;
        }
        let expr: Expr = attr.parse_args()?;
        if found.is_some() {
            return Err(Error::new(attr.span(), "duplicate #[from(...)] on field"));
        }
        found = Some(expr);
    }
    Ok(found)
}

/// Auto-pick the source whose type carries a field of `field_name`.
///
/// We don't have access to the full source struct definition at
/// macro-expansion time (proc-macros only see their own input). So we
/// can't *actually* verify the source has the named field. The
/// strategy: emit `<binding>.<field>` referencing each fixture binding;
/// if no source has the field, rustc surfaces the error at the call
/// site of the generated `From` impl, with field-name precision.
///
/// For now, the auto-projection rule is "use the first declared
/// binding." This requires the user to put their primary fixture
/// first; per-field `#[from(...)]` overrides handle the rest. A future
/// refinement could read the source structs via the type info that the
/// nightly compiler exposes, but stable proc-macros don't have that.
fn infer_source(_field: &syn::Ident, fixtures: &[(syn::Ident, syn::Type)]) -> Result<syn::Ident> {
    // First-declared binding wins for auto-projection. Users who need
    // a different default rearrange `#[from_fixtures(...)]` or annotate
    // the field with `#[from(other_binding.field)]`.
    Ok(fixtures[0].0.clone())
}

fn emit(spec: &Spec) -> TokenStream {
    let bundle = &spec.bundle_ident;
    let fixture_pat = fixture_tuple_pat(&spec.fixtures);
    let fixture_ty = fixture_tuple_ty(&spec.fixtures);
    let assignments = spec.fields.iter().map(|f| {
        let name = &f.name;
        match &f.source {
            FieldSource::Project(binding) => quote!(#name: #binding.#name),
            FieldSource::Expr(expr) => quote!(#name: #expr),
        }
    });
    quote! {
        impl<'__fixt> ::core::convert::From<#fixture_ty> for #bundle {
            fn from(#fixture_pat: #fixture_ty) -> Self {
                Self {
                    #(#assignments,)*
                }
            }
        }
    }
}

/// Render the pattern `(p, u)` for `#[from_fixtures(p: Pool, u: User)]`.
fn fixture_tuple_pat(fixtures: &[(syn::Ident, syn::Type)]) -> TokenStream {
    let names = fixtures.iter().map(|(n, _)| n);
    quote! { (#(#names,)*) }
}

/// Render the type `(&'__fixt Pool, &'__fixt User)` for the same.
fn fixture_tuple_ty(fixtures: &[(syn::Ident, syn::Type)]) -> TokenStream {
    let tys = fixtures.iter().map(|(_, t)| {
        let t = t.to_token_stream();
        quote! { &'__fixt #t }
    });
    quote! { (#(#tys,)*) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn parses_two_fixtures() {
        let input: DeriveInput = parse_quote! {
            #[from_fixtures(p: Pool, u: User)]
            struct B { a: Pubkey }
        };
        let spec = parse_spec(&input).expect("parse");
        assert_eq!(spec.fixtures.len(), 2);
        assert_eq!(spec.fixtures[0].0, "p");
        assert_eq!(spec.fixtures[1].0, "u");
    }

    #[test]
    fn errors_when_missing_fixtures() {
        let input: DeriveInput = parse_quote! { struct B { a: Pubkey } };
        let err = parse_spec(&input).expect_err("must error");
        assert!(err.to_string().contains("missing #[from_fixtures"));
    }

    #[test]
    fn errors_on_single_fixture() {
        let input: DeriveInput = parse_quote! {
            #[from_fixtures(p: Pool)]
            struct B { a: Pubkey }
        };
        let err = parse_spec(&input).expect_err("must error");
        assert!(err.to_string().contains("at least 2"));
    }

    #[test]
    fn errors_on_duplicate_fixtures_attr() {
        let input: DeriveInput = parse_quote! {
            #[from_fixtures(p: Pool, u: User)]
            #[from_fixtures(x: X, y: Y)]
            struct B { a: Pubkey }
        };
        let err = parse_spec(&input).expect_err("must error");
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn picks_first_fixture_for_auto_project() {
        let input: DeriveInput = parse_quote! {
            #[from_fixtures(p: Pool, u: User)]
            struct B { a: Pubkey }
        };
        let spec = parse_spec(&input).expect("parse");
        match &spec.fields[0].source {
            FieldSource::Project(b) => assert_eq!(b, "p"),
            FieldSource::Expr(_) => panic!("expected Project"),
        }
    }

    #[test]
    fn from_attr_takes_precedence() {
        let input: DeriveInput = parse_quote! {
            #[from_fixtures(p: Pool, u: User)]
            struct B {
                #[from(u.pubkey())]
                user: Pubkey,
            }
        };
        let spec = parse_spec(&input).expect("parse");
        match &spec.fields[0].source {
            FieldSource::Expr(_) => {}
            FieldSource::Project(_) => panic!("expected Expr"),
        }
    }

    #[test]
    fn errors_on_duplicate_from_attr() {
        let input: DeriveInput = parse_quote! {
            #[from_fixtures(p: Pool, u: User)]
            struct B {
                #[from(u.x)]
                #[from(u.y)]
                a: Pubkey,
            }
        };
        let err = parse_spec(&input).expect_err("must error");
        assert!(err.to_string().contains("duplicate #[from"));
    }
}
