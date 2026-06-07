//! Parse a `#[derive(Accounts)]` struct decorated with `#[bundled_with(...)]`
//! into our intermediate representation.

use syn::{
    parse::{Parse, ParseStream},
    spanned::Spanned,
    DeriveInput, Error, Result, Token,
};

/// Everything the emit step needs.
#[derive(Debug)]
pub struct Spec {
    /// The accounts struct's ident, e.g. `Make`. Used to construct the
    /// default `crate::accounts::Make` / `crate::instruction::Make`
    /// paths when no override is given.
    pub accounts_ident: syn::Ident,
    /// Path to the bundle struct named by `#[bundled_with(...)]`.
    /// Accepts a bare ident (`EscrowBundle`) or a qualified path
    /// (`crate::test_helpers::EscrowBundle`).
    pub bundle_path: syn::Path,
    /// Optional explicit override for the `instruction::*` type. Set via
    /// `#[bundled_with(Bundle, instruction = path)]`. When `None`, the
    /// emitter falls back to `crate::instruction::#accounts_ident`.
    ///
    /// Needed when the Accounts struct name doesn't match
    /// `PascalCase(fn_name)` of the handler: e.g. `fn initialize_poll`
    /// paired with `struct InitPoll`, where Anchor names
    /// `instruction::InitializePoll` from the handler (not the struct).
    pub instruction_path: Option<syn::Path>,
    /// Optional explicit override for the `accounts::*` type. Set via
    /// `#[bundled_with(Bundle, accounts = path)]`. When `None`, the
    /// emitter falls back to `crate::accounts::#accounts_ident`.
    ///
    /// Rarely needed in practice (Anchor pulls the `accounts::*` name
    /// from the `Context<Foo>` type argument, which usually matches the
    /// struct name by construction), but provided for symmetry with
    /// `instruction_path`.
    pub accounts_path: Option<syn::Path>,
    /// All fields, in declaration order.
    pub fields: Vec<Field>,
}

#[derive(Debug)]
pub struct Field {
    pub name: syn::Ident,
    /// Source-of-value for this field in the emitted `From` impl.
    pub source: FieldSource,
}

#[derive(Debug)]
pub enum FieldSource {
    /// `field: anchor_lang::system_program::ID`
    Const(proc_macro2::TokenStream),
    /// `field: b.field` (named with the same ident as the field on the bundle)
    Project,
    /// `field: b.field.expect("...")` — used when the bundle field is
    /// `Option<T>` but this particular accounts struct needs a bare `T`.
    /// Triggered by `#[bundle(unwrap)]` on the field.
    ProjectUnwrap,
    /// `field: ::core::option::Option::Some(b.field)` — used when the
    /// bundle field is a bare `T` but this accounts struct needs
    /// `Option<T>`. Triggered by `#[bundle(wrap_some)]` on the field.
    ProjectWrapSome,
}

pub fn parse(input: DeriveInput) -> Result<Spec> {
    let accounts_ident = input.ident.clone();
    let BundledWith {
        bundle_path,
        instruction_path,
        accounts_path,
    } = extract_bundled_with(&input)?;
    let fields = extract_fields(&input)?;
    Ok(Spec {
        accounts_ident,
        bundle_path,
        instruction_path,
        accounts_path,
        fields,
    })
}

/// Parsed contents of `#[bundled_with(BundlePath, instruction = ..., accounts = ...)]`.
/// The first positional arg is the bundle path; the keyword args are
/// optional and order-independent.
struct BundledWith {
    bundle_path: syn::Path,
    instruction_path: Option<syn::Path>,
    accounts_path: Option<syn::Path>,
}

impl Parse for BundledWith {
    fn parse(input: ParseStream) -> Result<Self> {
        let bundle_path: syn::Path = input.parse()?;
        let mut instruction_path: Option<syn::Path> = None;
        let mut accounts_path: Option<syn::Path> = None;
        while !input.is_empty() {
            let _: Token![,] = input.parse()?;
            // Tolerate a trailing comma after the bundle path or any kv arg.
            if input.is_empty() {
                break;
            }
            let key: syn::Ident = input.parse()?;
            let _: Token![=] = input.parse()?;
            let value: syn::Path = input.parse()?;
            match key.to_string().as_str() {
                "instruction" => {
                    if instruction_path.is_some() {
                        return Err(Error::new(
                            key.span(),
                            "duplicate `instruction = ...` in #[bundled_with]",
                        ));
                    }
                    instruction_path = Some(value);
                }
                "accounts" => {
                    if accounts_path.is_some() {
                        return Err(Error::new(
                            key.span(),
                            "duplicate `accounts = ...` in #[bundled_with]",
                        ));
                    }
                    accounts_path = Some(value);
                }
                other => {
                    return Err(Error::new(
                        key.span(),
                        format!(
                            "unknown key `{other}` in #[bundled_with]; expected `instruction` or `accounts`"
                        ),
                    ));
                }
            }
        }
        Ok(Self {
            bundle_path,
            instruction_path,
            accounts_path,
        })
    }
}

fn extract_bundled_with(input: &DeriveInput) -> Result<BundledWith> {
    let mut found: Option<BundledWith> = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("bundled_with") {
            continue;
        }
        let parsed: BundledWith = attr.parse_args()?;
        if found.is_some() {
            return Err(Error::new(attr.span(), "duplicate #[bundled_with]"));
        }
        found = Some(parsed);
    }
    found.ok_or_else(|| {
        Error::new(
            input.ident.span(),
            "missing #[bundled_with(BundleType)] - required by #[derive(BundledPubkeys)]",
        )
    })
}

fn extract_fields(input: &DeriveInput) -> Result<Vec<Field>> {
    let syn::Data::Struct(data) = &input.data else {
        return Err(Error::new(
            input.span(),
            "#[derive(BundledPubkeys)] only supports structs",
        ));
    };
    let syn::Fields::Named(named) = &data.fields else {
        return Err(Error::new(
            data.fields.span(),
            "#[derive(BundledPubkeys)] requires named fields",
        ));
    };
    let mut fields = Vec::with_capacity(named.named.len());
    for field in &named.named {
        let name = field
            .ident
            .clone()
            .ok_or_else(|| Error::new(field.span(), "field must be named"))?;
        // `#[bundle(...)]` overrides the default classification (which
        // would normally be `Project` or `Const`). The attribute is the
        // user's explicit request to coerce shapes between the bundle
        // and the target accounts field; if they wrote it, honour it.
        let source = match extract_bundle_attr(field)? {
            Some(s) => s,
            None => classify_field_type(&field.ty),
        };
        fields.push(Field { name, source });
    }
    Ok(fields)
}

/// Parse `#[bundle(unwrap)]` / `#[bundle(wrap_some)]` on a field, if
/// present. Returns `Ok(None)` when the attribute is absent. Duplicate
/// `#[bundle(...)]` attributes on the same field are an error.
fn extract_bundle_attr(field: &syn::Field) -> Result<Option<FieldSource>> {
    let mut found: Option<FieldSource> = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("bundle") {
            continue;
        }
        let arg: syn::Ident = attr.parse_args().map_err(|e| {
            Error::new(
                attr.span(),
                format!(
                    "#[bundle(...)] expects a bare keyword like `unwrap` or `wrap_some`: {e}"
                ),
            )
        })?;
        let mode = match arg.to_string().as_str() {
            "unwrap" => FieldSource::ProjectUnwrap,
            "wrap_some" => FieldSource::ProjectWrapSome,
            other => {
                return Err(Error::new(
                    arg.span(),
                    format!(
                        "unknown `#[bundle({other})]`; expected `unwrap` (Option<T> bundle field -> T target) or `wrap_some` (T bundle field -> Option<T> target)"
                    ),
                ))
            }
        };
        if found.is_some() {
            return Err(Error::new(
                attr.span(),
                "duplicate #[bundle(...)] on the same field",
            ));
        }
        found = Some(mode);
    }
    Ok(found)
}

/// Map well-known Anchor account types to canonical program IDs.
/// Anything we don't recognise falls through to `FieldSource::Project`,
/// which projects from the bundle struct.
fn classify_field_type(ty: &syn::Type) -> FieldSource {
    use quote::quote;
    let Some((head, inner)) = generic_inner(ty) else {
        return FieldSource::Project;
    };
    match (head.as_str(), inner.as_str()) {
        ("Program", "System") => {
            FieldSource::Const(quote!(anchor_lang::solana_program::system_program::ID))
        }
        ("Program", "AssociatedToken") => {
            FieldSource::Const(quote!(anchor_spl::associated_token::ID))
        }
        ("Interface", "TokenInterface") => FieldSource::Const(quote!(anchor_spl::token::ID)),
        _ => FieldSource::Project,
    }
}

/// Pull `(Head, Inner)` out of `Head<'_, Inner>` or `Head<Inner>`. The
/// match is purely textual on the last path segment, which is what we want:
/// `anchor_lang::prelude::Program<'info, System>` and `Program<'info, System>`
/// both resolve to `("Program", "System")`.
fn generic_inner(ty: &syn::Type) -> Option<(String, String)> {
    let syn::Type::Path(tp) = ty else { return None };
    let seg = tp.path.segments.last()?;
    let head = seg.ident.to_string();
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    // Find the first type argument (skip lifetime args).
    for arg in &args.args {
        if let syn::GenericArgument::Type(syn::Type::Path(p)) = arg {
            let last = p.path.segments.last()?;
            return Some((head, last.ident.to_string()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn render(ts: &proc_macro2::TokenStream) -> String {
        ts.to_string()
    }

    fn render_path(p: &syn::Path) -> String {
        use quote::ToTokens;
        p.to_token_stream().to_string()
    }

    #[test]
    fn parses_bundled_with() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(EscrowBundle)]
            struct Make<'info> {}
        };
        let spec = parse(input).expect("parse ok");
        assert_eq!(spec.accounts_ident, "Make");
        assert_eq!(render_path(&spec.bundle_path), "EscrowBundle");
    }

    #[test]
    fn parses_bundled_with_qualified_path() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(crate::test_helpers::EscrowBundle)]
            struct Make<'info> {}
        };
        let spec = parse(input).expect("parse ok");
        assert_eq!(
            render_path(&spec.bundle_path),
            "crate :: test_helpers :: EscrowBundle"
        );
    }

    #[test]
    fn errors_when_missing_bundled_with() {
        let input: DeriveInput = parse_quote! {
            struct Make<'info> {}
        };
        let err = parse(input).expect_err("must error");
        assert!(err.to_string().contains("missing #[bundled_with"));
    }

    #[test]
    fn errors_on_duplicate_bundled_with() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(A)]
            #[bundled_with(B)]
            struct Make<'info> {}
        };
        let err = parse(input).expect_err("must error");
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn classifies_program_system_as_const() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B)]
            struct Make<'info> {
                pub maker: Signer<'info>,
                pub system_program: Program<'info, System>,
            }
        };
        let spec = parse(input).expect("parse ok");
        assert_eq!(spec.fields.len(), 2);
        assert_eq!(spec.fields[0].name, "maker");
        assert!(matches!(spec.fields[0].source, FieldSource::Project));
        assert_eq!(spec.fields[1].name, "system_program");
        match &spec.fields[1].source {
            FieldSource::Const(ts) => {
                assert!(render(ts).contains("system_program"));
                assert!(render(ts).contains("ID"));
            }
            _ => panic!("expected Const for Program<System>"),
        }
    }

    #[test]
    fn classifies_program_associated_token_as_const() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B)]
            struct Make<'info> {
                pub associated_token_program: Program<'info, AssociatedToken>,
            }
        };
        let spec = parse(input).expect("parse ok");
        match &spec.fields[0].source {
            FieldSource::Const(ts) => {
                assert!(render(ts).contains("associated_token"));
            }
            _ => panic!("expected Const"),
        }
    }

    #[test]
    fn classifies_interface_token_interface_as_const() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B)]
            struct Make<'info> {
                pub token_program: Interface<'info, TokenInterface>,
            }
        };
        let spec = parse(input).expect("parse ok");
        match &spec.fields[0].source {
            FieldSource::Const(ts) => {
                assert!(render(ts).contains("anchor_spl"));
                assert!(render(ts).contains("token"));
            }
            _ => panic!("expected Const"),
        }
    }

    #[test]
    fn non_const_field_projects() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B)]
            struct Make<'info> {
                pub mint_a: InterfaceAccount<'info, Mint>,
                pub escrow: Account<'info, Escrow>,
            }
        };
        let spec = parse(input).expect("parse ok");
        assert!(spec
            .fields
            .iter()
            .all(|f| matches!(f.source, FieldSource::Project)));
    }

    #[test]
    fn errors_on_tuple_struct() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B)]
            struct Make(Signer<'info>);
        };
        let err = parse(input).expect_err("must error");
        assert!(err.to_string().contains("named fields"));
    }

    #[test]
    fn parses_bundled_with_instruction_override() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B, instruction = crate::instruction::InitializePoll)]
            struct InitPoll<'info> {}
        };
        let spec = parse(input).expect("parse ok");
        assert_eq!(render_path(&spec.bundle_path), "B");
        let instruction = spec.instruction_path.expect("override present");
        assert_eq!(
            render_path(&instruction),
            "crate :: instruction :: InitializePoll"
        );
        assert!(spec.accounts_path.is_none());
    }

    #[test]
    fn parses_bundled_with_accounts_override() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B, accounts = crate::accounts::InitPoll)]
            struct InitPoll<'info> {}
        };
        let spec = parse(input).expect("parse ok");
        let accounts = spec.accounts_path.expect("override present");
        assert_eq!(render_path(&accounts), "crate :: accounts :: InitPoll");
        assert!(spec.instruction_path.is_none());
    }

    #[test]
    fn parses_bundled_with_both_overrides() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(
                B,
                accounts = crate::accounts::InitPoll,
                instruction = crate::instruction::InitializePoll,
            )]
            struct InitPoll<'info> {}
        };
        let spec = parse(input).expect("parse ok");
        assert_eq!(
            render_path(&spec.accounts_path.unwrap()),
            "crate :: accounts :: InitPoll"
        );
        assert_eq!(
            render_path(&spec.instruction_path.unwrap()),
            "crate :: instruction :: InitializePoll"
        );
    }

    #[test]
    fn overrides_are_order_independent() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(
                B,
                instruction = crate::instruction::InitializePoll,
                accounts = crate::accounts::InitPoll
            )]
            struct InitPoll<'info> {}
        };
        let spec = parse(input).expect("parse ok");
        assert_eq!(
            render_path(&spec.accounts_path.unwrap()),
            "crate :: accounts :: InitPoll"
        );
        assert_eq!(
            render_path(&spec.instruction_path.unwrap()),
            "crate :: instruction :: InitializePoll"
        );
    }

    #[test]
    fn errors_on_unknown_key() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B, signer_seed = something)]
            struct Make<'info> {}
        };
        let err = parse(input).expect_err("must error");
        let msg = err.to_string();
        assert!(msg.contains("unknown key"), "got: {msg}");
        assert!(msg.contains("signer_seed"), "got: {msg}");
    }

    #[test]
    fn errors_on_duplicate_instruction_key() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B, instruction = a::B, instruction = c::D)]
            struct Make<'info> {}
        };
        let err = parse(input).expect_err("must error");
        let msg = err.to_string();
        assert!(msg.contains("duplicate"), "got: {msg}");
        assert!(msg.contains("instruction"), "got: {msg}");
    }

    #[test]
    fn errors_on_duplicate_accounts_key() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B, accounts = a::B, accounts = c::D)]
            struct Make<'info> {}
        };
        let err = parse(input).expect_err("must error");
        let msg = err.to_string();
        assert!(msg.contains("duplicate"), "got: {msg}");
        assert!(msg.contains("accounts"), "got: {msg}");
    }

    #[test]
    fn bundle_unwrap_attr_overrides_project() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B)]
            struct BuyWithToken<'info> {
                #[bundle(unwrap)]
                pub payment_mint: InterfaceAccount<'info, Mint>,
            }
        };
        let spec = parse(input).expect("parse ok");
        assert!(matches!(spec.fields[0].source, FieldSource::ProjectUnwrap));
    }

    #[test]
    fn bundle_wrap_some_attr_overrides_project() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B)]
            struct List<'info> {
                #[bundle(wrap_some)]
                pub payment_mint: Option<InterfaceAccount<'info, Mint>>,
            }
        };
        let spec = parse(input).expect("parse ok");
        assert!(matches!(
            spec.fields[0].source,
            FieldSource::ProjectWrapSome
        ));
    }

    #[test]
    fn bundle_attr_takes_precedence_over_well_known_program_classification() {
        // Pathological but worth pinning: if a user puts `#[bundle(unwrap)]`
        // on a Program<System> field, the attribute wins over the auto-Const
        // classification. We'd emit garbage, but that's the user's choice;
        // it's better to honour the explicit attribute than to silently
        // ignore it.
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B)]
            struct Make<'info> {
                #[bundle(unwrap)]
                pub system_program: Program<'info, System>,
            }
        };
        let spec = parse(input).expect("parse ok");
        assert!(matches!(spec.fields[0].source, FieldSource::ProjectUnwrap));
    }

    #[test]
    fn errors_on_unknown_bundle_keyword() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B)]
            struct Make<'info> {
                #[bundle(do_something_weird)]
                pub maker: Pubkey,
            }
        };
        let err = parse(input).expect_err("must error");
        let msg = err.to_string();
        assert!(msg.contains("unknown"), "got: {msg}");
        assert!(msg.contains("do_something_weird"), "got: {msg}");
    }

    #[test]
    fn errors_on_duplicate_bundle_attr_on_same_field() {
        let input: DeriveInput = parse_quote! {
            #[bundled_with(B)]
            struct Make<'info> {
                #[bundle(unwrap)]
                #[bundle(wrap_some)]
                pub maker: Pubkey,
            }
        };
        let err = parse(input).expect_err("must error");
        let msg = err.to_string();
        assert!(msg.contains("duplicate"), "got: {msg}");
    }
}
