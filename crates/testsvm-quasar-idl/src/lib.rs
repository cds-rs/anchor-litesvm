//! Source-extractor for quasar-lang programs.
//!
//! Read the declaration site (the program's own source) instead of compiling and
//! scraping it: parse the crate with `syn` for the `#[program]` instructions and
//! the `#[derive(Accounts)]` structs they name, build [`testsvm_idl`]'s [`IxDef`]
//! model directly, and emit the client through the shared [`emit_client`]. This
//! is the deterministic stage-1 replacement for `quasar idl-build`, which emits
//! the instruction list in hash-map order; reading source is order-stable.
//!
//! Design: NOTES/2026-06-22-quasar-source-extractor.md (to be promoted to
//! docs/design/quasar-source-extractor.md; repoint this pointer when it moves).

use {
    std::path::Path,
    testsvm_idl::{AccountDef, ArgDef, ArgType, IdlSource, IxDef, LenWidth},
};

pub use testsvm_idl::emit_client;

/// A quasar-lang program parsed from source: its address and instructions.
pub struct QuasarSource {
    address: String,
    instructions: Vec<IxDef>,
}

impl IdlSource for QuasarSource {
    fn program_address(&self) -> &str {
        &self.address
    }
    fn instructions(&self) -> Vec<IxDef> {
        self.instructions.clone()
    }
}

impl QuasarSource {
    /// Parse every `.rs` file under `src_dir` and extract the program.
    pub fn from_crate(src_dir: &Path) -> Result<Self, Error> {
        let mut sources = Vec::new();
        collect_rs(src_dir, &mut sources)?;
        Self::from_sources(&sources)
    }

    /// Extract the program from a set of already-read source strings (one per
    /// file). The split across files does not matter: instructions and the
    /// account structs they name are matched by name across the whole set.
    pub fn from_sources(sources: &[String]) -> Result<Self, Error> {
        let files = sources
            .iter()
            .map(|s| syn::parse_file(s).map_err(Error::Parse))
            .collect::<Result<Vec<_>, _>>()?;
        let items = || files.iter().flat_map(|f| &f.items);

        let address = items().find_map(program_id).ok_or(Error::MissingProgramId)?;

        // Every `#[derive(Accounts)]` struct, by name, so an instruction's
        // `Ctx<T>` resolves to its accounts wherever in the crate it is declared.
        let mut accounts = std::collections::HashMap::new();
        for item in items() {
            if let syn::Item::Struct(s) = item {
                if has_derive(&s.attrs, "Accounts") {
                    accounts.insert(s.ident.to_string(), account_defs(s));
                }
            }
        }

        let fns = items().find_map(program_fns).ok_or(Error::MissingProgram)?;
        let mut instructions = Vec::new();
        for f in fns {
            let Some(disc) = instruction_discriminator(&f.attrs) else {
                continue;
            };
            let (ctx_ty, args) = split_inputs(&f.sig);
            let accts = accounts.get(&ctx_ty).ok_or_else(|| Error::UnknownAccounts {
                instruction: f.sig.ident.to_string(),
                accounts: ctx_ty.clone(),
            })?;
            instructions.push(IxDef {
                name: f.sig.ident.to_string(),
                discriminator: vec![disc],
                accounts: accts.clone(),
                args,
                has_remaining: false,
            });
        }
        // Match the JSON path's canonical order (it sorts on ingest): the
        // discriminator is unique, so this is a total, source-order-independent
        // order, and the emitted client is byte-identical to the idl-build path.
        instructions.sort_by(|a, b| a.discriminator.cmp(&b.discriminator));

        Ok(QuasarSource {
            address,
            instructions,
        })
    }
}

/// The `declare_id!("…")` address, if this item is that macro call.
fn program_id(item: &syn::Item) -> Option<String> {
    let syn::Item::Macro(m) = item else { return None };
    if !m.mac.path.is_ident("declare_id") {
        return None;
    }
    m.mac.parse_body::<syn::LitStr>().ok().map(|l| l.value())
}

/// The functions inside the `#[program]` module, if this item is that module.
fn program_fns(item: &syn::Item) -> Option<Vec<&syn::ItemFn>> {
    let syn::Item::Mod(m) = item else { return None };
    if !m.attrs.iter().any(|a| a.path().is_ident("program")) {
        return None;
    }
    let (_, items) = m.content.as_ref()?;
    Some(
        items
            .iter()
            .filter_map(|it| match it {
                syn::Item::Fn(f) => Some(f),
                _ => None,
            })
            .collect(),
    )
}

/// Whether `attrs` carries `#[derive(.., name, ..)]`.
fn has_derive(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("derive") {
            return false;
        }
        let mut found = false;
        let _ = a.parse_nested_meta(|m| {
            found |= m.path.is_ident(name);
            Ok(())
        });
        found
    })
}

/// The `N` from a fn's `#[instruction(discriminator = N)]`. (The struct-level
/// `#[instruction(arg: T)]` uses a different syntax and is never read here.)
fn instruction_discriminator(attrs: &[syn::Attribute]) -> Option<u8> {
    for a in attrs.iter().filter(|a| a.path().is_ident("instruction")) {
        let mut disc = None;
        let _ = a.parse_nested_meta(|m| {
            if m.path.is_ident("discriminator") {
                disc = m.value()?.parse::<syn::LitInt>()?.base10_parse::<u8>().ok();
            }
            Ok(())
        });
        if disc.is_some() {
            return disc;
        }
    }
    None
}

/// Split a handler signature into the `Ctx<T>` accounts-struct name and the
/// trailing scalar args.
fn split_inputs(sig: &syn::Signature) -> (String, Vec<ArgDef>) {
    let mut inputs = sig.inputs.iter();
    let ctx_ty = inputs.next().and_then(ctx_inner).unwrap_or_default();
    let args = inputs.filter_map(arg_def).collect();
    (ctx_ty, args)
}

/// The `T` in a first parameter typed `Ctx<T>` (or any `Wrapper<T>`).
fn ctx_inner(arg: &syn::FnArg) -> Option<String> {
    let syn::FnArg::Typed(pt) = arg else {
        return None;
    };
    let syn::Type::Path(tp) = &*pt.ty else {
        return None;
    };
    let syn::PathArguments::AngleBracketed(ab) = &tp.path.segments.last()?.arguments else {
        return None;
    };
    ab.args.iter().find_map(|ga| match ga {
        syn::GenericArgument::Type(syn::Type::Path(inner)) => {
            Some(inner.path.segments.last()?.ident.to_string())
        }
        _ => None,
    })
}

/// A scalar arg (`name: ty`) as an [`ArgDef`].
fn arg_def(arg: &syn::FnArg) -> Option<ArgDef> {
    let syn::FnArg::Typed(pt) = arg else {
        return None;
    };
    let syn::Pat::Ident(pi) = &*pt.pat else {
        return None;
    };
    Some(ArgDef {
        name: pi.ident.to_string(),
        ty: arg_type(&pt.ty),
    })
}

/// Map a `syn` type to the flat-args [`ArgType`] (the JSON path's `arg_type`,
/// over tokens instead of a JSON string). Quasar is wincode, so a `String` is a
/// `u8`-prefixed byte vector.
fn arg_type(ty: &syn::Type) -> ArgType {
    if let syn::Type::Array(arr) = ty {
        let n = array_len(&arr.len).unwrap_or(0);
        return ArgType::Array(Box::new(arg_type(&arr.elem)), n);
    }
    let syn::Type::Path(tp) = ty else {
        return unsupported(ty);
    };
    let seg = match tp.path.segments.last() {
        Some(s) => s,
        None => return unsupported(ty),
    };
    match seg.ident.to_string().as_str() {
        "u8" => ArgType::U8,
        "u16" => ArgType::U16,
        "u32" => ArgType::U32,
        "u64" => ArgType::U64,
        "i8" => ArgType::I8,
        "i16" => ArgType::I16,
        "i32" => ArgType::I32,
        "i64" => ArgType::I64,
        "bool" => ArgType::Bool,
        // Quasar's `Address` is the pubkey type (the IDL emits it as `pubkey`):
        // 32 bytes, same wire encoding as a solana `Pubkey`.
        "Pubkey" | "Address" => ArgType::Pubkey,
        "String" => ArgType::Bytes { len: LenWidth::U8 },
        "Option" => match &seg.arguments {
            syn::PathArguments::AngleBracketed(ab) => match ab.args.first() {
                Some(syn::GenericArgument::Type(inner)) => {
                    ArgType::Option(Box::new(arg_type(inner)))
                }
                _ => unsupported(ty),
            },
            _ => unsupported(ty),
        },
        _ => unsupported(ty),
    }
}

fn unsupported(ty: &syn::Type) -> ArgType {
    ArgType::Unsupported(quote::quote!(#ty).to_string())
}

/// A `[T; N]` length literal as `usize`.
fn array_len(expr: &syn::Expr) -> Option<usize> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(n),
            ..
        }) => n.base10_parse().ok(),
        _ => None,
    }
}

/// The declared accounts of a `#[derive(Accounts)]` struct, in field order.
fn account_defs(s: &syn::ItemStruct) -> Vec<AccountDef> {
    let syn::Fields::Named(named) = &s.fields else {
        return Vec::new();
    };
    named
        .named
        .iter()
        .filter_map(|f| {
            Some(AccountDef {
                name: f.ident.as_ref()?.to_string(),
                signer: type_last_ident(&f.ty).as_deref() == Some("Signer"),
                writable: account_is_writable(&f.attrs),
            })
        })
        .collect()
}

/// The last path segment of a type, e.g. `Signer` from `Signer`, `Program` from
/// `Program<SystemProgram>`.
fn type_last_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(tp) => Some(tp.path.segments.last()?.ident.to_string()),
        _ => None,
    }
}

/// Whether a field's `#[account(..)]` marks it writable. `mut` is the explicit
/// flag; `init`/`init_if_needed`/`realloc` create or resize the account and so
/// imply it (matching Anchor/Quasar semantics, e.g. an `init` PDA with no `mut`).
fn account_is_writable(attrs: &[syn::Attribute]) -> bool {
    const FLAGS: [&str; 4] = ["mut", "init", "init_if_needed", "realloc"];
    attrs.iter().any(|a| {
        if !a.path().is_ident("account") {
            return false;
        }
        let syn::Meta::List(list) = &a.meta else {
            return false;
        };
        list.tokens
            .to_string()
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| FLAGS.contains(&w))
    })
}

/// Recursively read every `.rs` file under `dir` into `out`.
fn collect_rs(dir: &Path, out: &mut Vec<String>) -> Result<(), Error> {
    for entry in std::fs::read_dir(dir).map_err(|e| Error::Io(dir.display().to_string(), e))? {
        let path = entry
            .map_err(|e| Error::Io(dir.display().to_string(), e))?
            .path();
        if path.is_dir() {
            collect_rs(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(std::fs::read_to_string(&path).map_err(|e| Error::Io(path.display().to_string(), e))?);
        }
    }
    Ok(())
}

/// What can go wrong extracting a program from source.
#[derive(Debug)]
pub enum Error {
    Io(String, std::io::Error),
    Parse(syn::Error),
    /// No `declare_id!("…")` found.
    MissingProgramId,
    /// No `#[program]` module found.
    MissingProgram,
    /// An instruction names an accounts struct that no `#[derive(Accounts)]` in
    /// the parsed sources defines.
    UnknownAccounts {
        instruction: String,
        accounts: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(p, e) => write!(f, "read {p}: {e}"),
            Error::Parse(e) => write!(f, "parse: {e}"),
            Error::MissingProgramId => write!(f, "no declare_id!(\"…\") in sources"),
            Error::MissingProgram => write!(f, "no #[program] module in sources"),
            Error::UnknownAccounts {
                instruction,
                accounts,
            } => write!(f, "instruction `{instruction}` names unknown accounts `{accounts}`"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    const PROG: &str = r#"
        declare_id!("8hjbFtnfY87ZzpEpx26u5tx4KjxkrqiWGUEcWFbDNn7h");

        #[program]
        mod p {
            #[instruction(discriminator = 0)]
            pub fn initialize(ctx: Ctx<Initialize>, amount: u64) -> Result<(), ProgramError> { Ok(()) }

            #[instruction(discriminator = 2)]
            pub fn reveal(ctx: Ctx<Reveal>, preimage: [u8; 32]) -> Result<(), ProgramError> { Ok(()) }
        }

        #[derive(Accounts)]
        pub struct Initialize {
            #[account(mut)]
            pub house: Signer,
            #[account(init, payer = house)]
            pub vault: UncheckedAccount,
            pub system_program: Program<SystemProgram>,
        }

        #[derive(Accounts)]
        pub struct Reveal {
            pub house: Signer,
        }
    "#;

    fn parse() -> QuasarSource {
        QuasarSource::from_sources(&[PROG.to_string()]).unwrap()
    }

    #[test]
    fn extracts_program_address() {
        assert_eq!(
            parse().program_address(),
            "8hjbFtnfY87ZzpEpx26u5tx4KjxkrqiWGUEcWFbDNn7h"
        );
    }

    #[test]
    fn extracts_instruction_name_discriminator_and_args() {
        let ixs = parse().instructions();
        let init = ixs.iter().find(|i| i.name == "initialize").expect("initialize");
        assert_eq!(init.discriminator, vec![0]);
        assert_eq!(init.args.len(), 1);
        assert_eq!(init.args[0].name, "amount");
        assert_eq!(init.args[0].ty, ArgType::U64);
    }

    #[test]
    fn maps_a_fixed_size_array_arg() {
        let ixs = parse().instructions();
        let reveal = ixs.iter().find(|i| i.name == "reveal").expect("reveal");
        assert_eq!(reveal.discriminator, vec![2]);
        assert_eq!(
            reveal.args[0].ty,
            ArgType::Array(Box::new(ArgType::U8), 32)
        );
    }

    #[test]
    fn resolves_accounts_with_signer_and_writable_flags() {
        let ixs = parse().instructions();
        let init = ixs.iter().find(|i| i.name == "initialize").expect("initialize");
        let names: Vec<&str> = init.accounts.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["house", "vault", "system_program"]);
        // `#[account(mut)] pub house: Signer` — signs and is written.
        assert!(init.accounts[0].signer && init.accounts[0].writable);
        // `#[account(init, ..)]` implies writable even without `mut`.
        assert!(!init.accounts[1].signer && init.accounts[1].writable);
        // a bare `Program<SystemProgram>` — neither.
        assert!(!init.accounts[2].signer && !init.accounts[2].writable);
    }

    #[test]
    fn sorts_instructions_by_discriminator() {
        // Declared initialize(0) then reveal(2); a wider program could declare
        // out of order, and the JSON path sorts, so match it for a canonical,
        // path-independent client.
        let ixs = parse().instructions();
        let discs: Vec<u8> = ixs.iter().map(|i| i.discriminator[0]).collect();
        let mut sorted = discs.clone();
        sorted.sort();
        assert_eq!(discs, sorted, "instructions come out in discriminator order");
    }

    #[test]
    fn maps_quasar_address_args_as_pubkey() {
        // Quasar's `Address` is the pubkey type (the IDL emits it as `pubkey`), so
        // an `Address` arg and an `Option<Address>` arg map to Pubkey rather than
        // falling past the flat-args boundary.
        let src = r#"
            declare_id!("11111111111111111111111111111111");
            #[program]
            mod p {
                #[instruction(discriminator = 0)]
                pub fn cfg(ctx: Ctx<Cfg>, admin: Address, backup: Option<Address>) -> Result<(), E> { Ok(()) }
            }
            #[derive(Accounts)]
            pub struct Cfg { #[account(mut)] pub signer: Signer }
        "#;
        let src = QuasarSource::from_sources(&[src.to_string()]).unwrap();
        let args = &src.instructions()[0].args;
        assert_eq!(args[0].ty, ArgType::Pubkey);
        assert_eq!(args[1].ty, ArgType::Option(Box::new(ArgType::Pubkey)));
    }
}
