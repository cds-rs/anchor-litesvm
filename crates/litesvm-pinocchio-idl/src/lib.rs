//! Host-side IDL extractor for Pinocchio programs.
//!
//! The companion `#[derive(Discriminator)]` keeps a program's instruction enum
//! plain and syn-visible; this crate is the "build step" that reads it. It walks
//! a crate's `src/`, finds the enum carrying `#[derive(Discriminator)]`, and
//! emits a **modern Anchor IDL** (spec 0.1.0) from three syntactic facts: the
//! variant order (the leading-byte discriminator), the `#[account(..)]` helper
//! attributes (the account list, with `writable`/`signer`/`optional`/`desc`),
//! and the variant payload + referenced structs (the arg types). No compilation,
//! no `shank`, no dependency in the program graph; just source in, IDL out.
//!
//! The Anchor IDL shape is what `@codama/nodes-from-anchor` ingests, so the
//! output feeds codama's Rust/JS/Go/Python client generators, the same pipeline
//! an Anchor program enjoys, for a `no_std` Pinocchio program shank cannot touch.
//! It is also what `testsvm-idl`'s `anchor::AnchorIdl` consumes, so a consumer's
//! `build.rs` can extract the IDL and emit a client without a JSON file:
//!
//! ```no_run
//! let idl = litesvm_pinocchio_idl::idl_from_crate(
//!     std::path::Path::new("."), "11111111111111111111111111111111", None,
//! ).unwrap();
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use syn::{Expr, Fields, GenericArgument, Item, Lit, PathArguments, Type};

/// Walk `crate_root/src`, read the `#[derive(Discriminator)]` instruction enum
/// and the types it references, and return a modern Anchor IDL (spec 0.1.0).
/// `program_id` is the address embedded in the IDL; `name` overrides the program
/// name (defaults to the crate name).
pub fn idl_from_crate(
    crate_root: &Path,
    program_id: &str,
    name: Option<String>,
) -> Result<Value, String> {
    let src = crate_root.join("src");
    let mut items = Vec::new();
    for file in rs_files(&src) {
        let content =
            std::fs::read_to_string(&file).map_err(|e| format!("read {}: {e}", file.display()))?;
        let parsed =
            syn::parse_file(&content).map_err(|e| format!("parse {}: {e}", file.display()))?;
        collect_items(parsed.items, &mut items);
    }

    // Index every struct/enum definition by name, so an instruction's arg type
    // (and types it transitively references) can be resolved and emitted.
    let mut defs: BTreeMap<String, Item> = BTreeMap::new();
    for item in &items {
        match item {
            Item::Struct(s) => {
                defs.insert(s.ident.to_string(), item.clone());
            }
            Item::Enum(e) => {
                defs.insert(e.ident.to_string(), item.clone());
            }
            _ => {}
        }
    }

    // The instruction set is the enum carrying `#[derive(Discriminator)]`.
    let ix_enum = items
        .iter()
        .filter_map(|item| match item {
            Item::Enum(e) if has_derive(&e.attrs, "Discriminator") => Some(e),
            _ => None,
        })
        .next()
        .ok_or("no enum with #[derive(Discriminator)] found in src/")?;

    let mut instructions = Vec::new();
    let mut referenced: Vec<String> = Vec::new();

    for (idx, variant) in ix_enum.variants.iter().enumerate() {
        // The invariant the whole pipeline rests on: the discriminator is the
        // single leading byte of the instruction data (`data[0]`), equal to the
        // variant's declaration index. The `#[derive(Discriminator)]` half emits
        // `idx as u8`; we emit the same index here. If a program ever grew past
        // 256 instructions the two would silently disagree (the derive wraps mod
        // 256, this array would not), so refuse to emit rather than ship an IDL
        // whose generated client dispatches to the wrong handler.
        if idx > u8::MAX as usize {
            return Err(format!(
                "instruction #{idx} ({}) exceeds the single-byte discriminator space; \
                 the leading-byte dispatch invariant only holds for up to 256 variants",
                variant.ident
            ));
        }

        let accounts: Vec<Value> = variant
            .attrs
            .iter()
            .filter(|a| a.path().is_ident("account"))
            .map(parse_account)
            .collect::<Result<_, _>>()?;

        let args = match &variant.fields {
            Fields::Unit => Vec::new(),
            Fields::Unnamed(fields) => fields
                .unnamed
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let ty = type_to_idl(&f.ty, &mut referenced);
                    // A single payload names the arg after the type
                    // (`Make(MakeArgs)` -> `make_args`); multiple positional
                    // fields fall back to arg0, arg1, ...
                    let name = if fields.unnamed.len() == 1 {
                        to_snake(&type_ident(&f.ty).unwrap_or_else(|| format!("arg{i}")))
                    } else {
                        format!("arg{i}")
                    };
                    json!({ "name": name, "type": ty })
                })
                .collect(),
            Fields::Named(fields) => fields
                .named
                .iter()
                .map(|f| {
                    let name = f.ident.as_ref().unwrap().to_string();
                    json!({ "name": name, "type": type_to_idl(&f.ty, &mut referenced) })
                })
                .collect(),
        };

        instructions.push(json!({
            "name": to_snake(&variant.ident.to_string()),
            "discriminator": [idx as u64],
            "accounts": accounts,
            "args": args,
        }));
    }

    // Emit every referenced type (and types they reference, transitively).
    let mut types = Vec::new();
    let mut emitted: Vec<String> = Vec::new();
    while let Some(name) = referenced.pop() {
        if emitted.contains(&name) {
            continue;
        }
        emitted.push(name.clone());
        if let Some(item) = defs.get(&name) {
            if let Some(ty_json) = def_to_idl(item, &mut referenced) {
                types.push(json!({ "name": name, "type": ty_json }));
            }
        }
    }
    types.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));

    let program_name = name.unwrap_or_else(|| crate_name(crate_root));

    Ok(json!({
        "address": program_id,
        "metadata": { "name": program_name, "version": "0.1.0", "spec": "0.1.0" },
        "instructions": instructions,
        "accounts": [],
        "types": types,
        "errors": [],
    }))
}

/// Recursively gather items, descending into inline `mod { .. }` blocks.
fn collect_items(items: Vec<Item>, out: &mut Vec<Item>) {
    for item in items {
        if let Item::Mod(m) = &item {
            if let Some((_, inner)) = &m.content {
                collect_items(inner.clone(), out);
            }
        }
        out.push(item);
    }
}

fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(rs_files(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out
}

fn has_derive(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("derive"))
        .any(|a| {
            let mut found = false;
            let _ = a.parse_nested_meta(|m| {
                if m.path.is_ident(name) {
                    found = true;
                }
                Ok(())
            });
            found
        })
}

/// `#[account(0, signer, writable, optional, name = "x", desc = "..")]` ->
/// the Anchor account node `{name, writable?, signer?, optional?, docs?}`,
/// omitting the flags that are false (Anchor's convention).
fn parse_account(attr: &syn::Attribute) -> Result<Value, String> {
    let mut name = String::new();
    let mut writable = false;
    let mut signer = false;
    let mut optional = false;
    let mut docs: Option<String> = None;

    let exprs = attr
        .parse_args_with(syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated)
        .map_err(|e| format!("parse #[account]: {e}"))?;

    for expr in exprs {
        match expr {
            // leading index literal: ordering only, ignored
            Expr::Lit(lit) if matches!(lit.lit, Lit::Int(_)) => {}
            Expr::Path(p) => {
                if p.path.is_ident("signer") {
                    signer = true;
                } else if p.path.is_ident("writable") || p.path.is_ident("mut") {
                    writable = true;
                } else if p.path.is_ident("optional") {
                    optional = true;
                }
            }
            Expr::Assign(assign) => {
                if let (Expr::Path(p), Expr::Lit(lit)) =
                    (assign.left.as_ref(), assign.right.as_ref())
                {
                    if let Lit::Str(s) = &lit.lit {
                        if p.path.is_ident("name") {
                            name = s.value();
                        } else if p.path.is_ident("desc") {
                            docs = Some(s.value());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let mut account = Map::new();
    account.insert("name".into(), json!(name));
    if writable {
        account.insert("writable".into(), json!(true));
    }
    if signer {
        account.insert("signer".into(), json!(true));
    }
    if optional {
        account.insert("optional".into(), json!(true));
    }
    if let Some(d) = docs {
        account.insert("docs".into(), json!([d]));
    }
    Ok(Value::Object(account))
}

fn def_to_idl(item: &Item, referenced: &mut Vec<String>) -> Option<Value> {
    match item {
        Item::Struct(s) => {
            let fields: Vec<Value> = s
                .fields
                .iter()
                .map(|f| {
                    json!({
                        "name": f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default(),
                        "type": type_to_idl(&f.ty, referenced),
                    })
                })
                .collect();
            Some(json!({ "kind": "struct", "fields": fields }))
        }
        Item::Enum(e) => {
            let variants: Vec<Value> = e
                .variants
                .iter()
                .map(|v| json!({ "name": v.ident.to_string() }))
                .collect();
            Some(json!({ "kind": "enum", "variants": variants }))
        }
        _ => None,
    }
}

/// Map a `syn::Type` to a modern Anchor IDL type node, recording `defined` types.
fn type_to_idl(ty: &Type, referenced: &mut Vec<String>) -> Value {
    match ty {
        Type::Path(tp) => {
            let seg = match tp.path.segments.last() {
                Some(s) => s,
                None => return json!("unknown"),
            };
            let ident = seg.ident.to_string();
            let inner = |referenced: &mut Vec<String>| -> Option<Value> {
                if let PathArguments::AngleBracketed(a) = &seg.arguments {
                    for arg in &a.args {
                        if let GenericArgument::Type(t) = arg {
                            return Some(type_to_idl(t, referenced));
                        }
                    }
                }
                None
            };
            match ident.as_str() {
                "Vec" => json!({ "vec": inner(referenced).unwrap_or(json!("unknown")) }),
                "Option" => json!({ "option": inner(referenced).unwrap_or(json!("unknown")) }),
                "Box" => inner(referenced).unwrap_or(json!("unknown")),
                "String" | "str" => json!("string"),
                "Pubkey" | "Address" => json!("pubkey"),
                "u8" | "u16" | "u32" | "u64" | "u128" | "i8" | "i16" | "i32" | "i64" | "i128"
                | "bool" | "f32" | "f64" => json!(ident),
                _ => {
                    referenced.push(ident.clone());
                    json!({ "defined": { "name": ident } })
                }
            }
        }
        Type::Array(arr) => {
            let elem = type_to_idl(&arr.elem, referenced);
            let len = match &arr.len {
                Expr::Lit(lit) => match &lit.lit {
                    Lit::Int(i) => i.base10_parse::<u64>().unwrap_or(0),
                    _ => 0,
                },
                _ => 0,
            };
            json!({ "array": [elem, len] })
        }
        _ => json!("unknown"),
    }
}

fn type_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(tp) => tp.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// PascalCase / camelCase -> snake_case, matching Anchor's IDL naming for
/// instructions, args, and fields. `MakeArgs` -> `make_args`, `Make` -> `make`.
fn to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn crate_name(crate_root: &Path) -> String {
    let manifest = crate_root.join("Cargo.toml");
    if let Ok(text) = std::fs::read_to_string(manifest) {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("name") {
                if let Some(eq) = rest.trim_start().strip_prefix('=') {
                    return eq.trim().trim_matches('"').replace('-', "_");
                }
            }
        }
    }
    "program".to_string()
}
