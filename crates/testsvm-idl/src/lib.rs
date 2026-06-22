//! IDL ingestion for testsvm.
//!
//! One [`IdlSource`] impl per IDL format normalizes that format into a common
//! instruction model ([`IxDef`]); [`emit_client`] generates the named bundle +
//! alias client from the model, so the generator is written once and every
//! framework that emits an IDL (Quasar today, Shank/Codama next) is a parser,
//! not a fork of the codegen.
//!
//! The generated client, per flat-args instruction, is a struct whose fields
//! are the declared accounts (well-known programs injected, not exposed) and the
//! scalar args, with:
//!
//!   - `ix()`     : the `Instruction`, accounts in IDL order with the IDL's
//!                  signer/writable flags, the discriminator, args little-endian
//!                  encoded, then `remaining_accounts` appended.
//!   - `alias_all`: registers each account field under its IDL name on any
//!                  [`TestSVM`](testsvm::TestSVM) backend (the engine-agnostic
//!                  AliasMirror).
//!
//! Instructions with a non-scalar arg are skipped at the encoding boundary
//! (wincode `DynVec`/`DynBytes`, borsh `Vec`/`String`): the flat-args floor.

use std::fmt::Write as _;

/// A parsed IDL, normalized: the program address and its instructions. One impl
/// per IDL format (see [`quasar::QuasarIdl`]).
pub trait IdlSource {
    /// The program's on-chain address, base58.
    fn program_address(&self) -> &str;
    /// The instructions, in a deterministic order so the generated client is
    /// byte-stable to commit (the JSON sources sort by discriminator; a
    /// declaration-site source carries its own canonical declaration order).
    fn instructions(&self) -> Vec<IxDef>;
}

/// One instruction, format-independent.
#[derive(Debug, Clone)]
pub struct IxDef {
    pub name: String,
    /// The leading-byte discriminator (any width).
    pub discriminator: Vec<u8>,
    /// The declared accounts, in the order the instruction lists them.
    pub accounts: Vec<AccountDef>,
    pub args: Vec<ArgDef>,
    /// Whether a variable `remaining_accounts` tail is appended.
    pub has_remaining: bool,
}

#[derive(Debug, Clone)]
pub struct AccountDef {
    pub name: String,
    pub signer: bool,
    pub writable: bool,
}

#[derive(Debug, Clone)]
pub struct ArgDef {
    pub name: String,
    pub ty: ArgType,
}

/// The arg types the flat-args floor handles, plus the catch-all that pushes an
/// instruction past the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    Bool,
    Pubkey,
    /// Length-prefixed bytes (a string): a `len`-width little-endian count, then
    /// the bytes. The width is the one format-dependent piece, so the
    /// [`IdlSource`] impl picks it (wincode `DynBytes<u8>` = `U8`, borsh `String`
    /// = `U32`) and the emitter renders it uniformly.
    Bytes {
        len: LenWidth,
    },
    /// An optional value: a 1-byte present/absent tag (`0`/`1`), then the inner
    /// value when present. wincode and borsh agree on this shape, so it's
    /// format-independent (the inner type carries any format-specific piece).
    Option(Box<ArgType>),
    /// A fixed-size array `[T; N]`: `N` elements in order, no length prefix
    /// (the length is in the type). wincode and borsh agree on this shape.
    Array(Box<ArgType>, usize),
    /// Anything we don't encode yet (a vec, a defined type): the boundary.
    /// Carries the source spelling for the skip diagnostic.
    Unsupported(String),
}

/// The width of a length prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LenWidth {
    U8,
    U16,
    U32,
}

impl LenWidth {
    fn rust_type(self) -> &'static str {
        match self {
            LenWidth::U8 => "u8",
            LenWidth::U16 => "u16",
            LenWidth::U32 => "u32",
        }
    }
}

impl ArgType {
    /// The Rust type for a struct field, or `None` past the boundary.
    pub fn rust_type(&self) -> Option<String> {
        Some(match self {
            ArgType::U8 => "u8".into(),
            ArgType::U16 => "u16".into(),
            ArgType::U32 => "u32".into(),
            ArgType::U64 => "u64".into(),
            ArgType::I8 => "i8".into(),
            ArgType::I16 => "i16".into(),
            ArgType::I32 => "i32".into(),
            ArgType::I64 => "i64".into(),
            ArgType::Bool => "bool".into(),
            ArgType::Pubkey => "Pubkey".into(),
            ArgType::Bytes { .. } => "String".into(),
            ArgType::Option(inner) => format!("Option<{}>", inner.rust_type()?),
            ArgType::Array(inner, n) => format!("[{}; {}]", inner.rust_type()?, n),
            ArgType::Unsupported(_) => return None,
        })
    }

    /// The statement that appends this arg's little-endian bytes to `data`,
    /// given the field access `f` (e.g. `self.amount`).
    fn encode(&self, f: &str) -> Option<String> {
        Some(match self {
            ArgType::Bool => format!("data.push({f} as u8);"),
            ArgType::Pubkey => format!("data.extend_from_slice({f}.as_ref());"),
            ArgType::Bytes { len } => {
                // The length prefix in its declared width, then the bytes.
                let lt = len.rust_type();
                format!(
                    "data.extend_from_slice(&({f}.len() as {lt}).to_le_bytes()); \
                     data.extend_from_slice({f}.as_bytes());"
                )
            }
            ArgType::Option(inner) => {
                // A 1-byte tag, then the inner value when present. The inner
                // encodes against the match binding `v`.
                let some = inner.encode("v")?;
                format!(
                    "match &{f} {{ Some(v) => {{ data.push(1); {some} }} None => {{ data.push(0); }} }}"
                )
            }
            ArgType::Array(inner, _n) => {
                // Fixed-size array: elements in order, no length prefix. A
                // `[u8; N]` is a direct byte copy; any other element type
                // encodes per element against the loop binding.
                if matches!(**inner, ArgType::U8) {
                    format!("data.extend_from_slice(&{f});")
                } else {
                    let each = inner.encode("e")?;
                    format!("for e in {f}.iter().copied() {{ {each} }}")
                }
            }
            ArgType::Unsupported(_) => return None,
            // Every integer is little-endian, which is both wincode's and
            // borsh's scalar encoding, so this is format-independent.
            _ => format!("data.extend_from_slice(&{f}.to_le_bytes());"),
        })
    }
}

/// Generate the client module source from any IDL source.
pub fn emit_client(src: &dyn IdlSource) -> String {
    let mut out = String::new();
    // A plain `//` banner and no inner attributes, so the file is `include!`-able
    // into a `mod {}` (inner attributes and doc comments in an included file are
    // rejected outside the crate root). The including site applies
    // `#[allow(dead_code, unused_imports)]` to silence the unused well-known
    // helpers and imports.
    out.push_str(
        "// GENERATED by testsvm-idl. Do not edit by hand; re-run the generator.\n\
         // Each struct's `ix()` lays accounts out in IDL order with the IDL's\n\
         // signer/writable flags, the discriminator, and little-endian args;\n\
         // `alias_all` names the accounts on any TestSVM backend.\n\n\
         use {\n    solana_instruction::{AccountMeta, Instruction},\n    \
         solana_pubkey::Pubkey,\n    std::str::FromStr,\n    testsvm::TestSVM,\n};\n\n",
    );
    let _ = writeln!(
        out,
        "pub fn program_id() -> Pubkey {{ Pubkey::from_str(\"{}\").unwrap() }}",
        src.program_address()
    );
    out.push_str(WELL_KNOWN_FNS);
    out.push('\n');

    for ix in src.instructions() {
        if let Some(bad) = ix.args.iter().find(|a| a.ty.rust_type().is_none()) {
            let spelling = match &bad.ty {
                ArgType::Unsupported(s) => s.as_str(),
                _ => "?",
            };
            let _ = writeln!(
                out,
                "// SKIPPED `{}`: non-scalar arg `{}` ({}) — the flat-args boundary.\n",
                ix.name, bad.name, spelling
            );
            continue;
        }
        emit_instruction(&mut out, &ix);
    }
    out
}

fn emit_instruction(out: &mut String, ix: &IxDef) {
    let name = pascal(&ix.name);
    let fields: Vec<&AccountDef> = ix
        .accounts
        .iter()
        .filter(|a| injector(&a.name).is_none())
        .collect();

    // struct
    let _ = writeln!(out, "pub struct {name} {{");
    for a in &fields {
        let _ = writeln!(out, "    pub {}: Pubkey,", snake(&a.name));
    }
    for arg in &ix.args {
        let _ = writeln!(
            out,
            "    pub {}: {},",
            snake(&arg.name),
            arg.ty.rust_type().unwrap()
        );
    }
    if ix.has_remaining {
        out.push_str("    /// Appended after the declared accounts, in order.\n");
        out.push_str("    pub remaining: Vec<AccountMeta>,\n");
    }
    out.push_str("}\n\n");

    // impl
    let _ = writeln!(out, "impl {name} {{");
    out.push_str("    pub fn ix(&self) -> Instruction {\n");
    let mutability = if ix.has_remaining {
        "let mut accounts"
    } else {
        "let accounts"
    };
    let _ = writeln!(out, "        {mutability} = vec![");
    for a in &ix.accounts {
        let pk = match injector(&a.name) {
            Some(inject) => inject.to_string(),
            None => format!("self.{}", snake(&a.name)),
        };
        let _ = writeln!(out, "            {},", meta(&pk, a.signer, a.writable));
    }
    out.push_str("        ];\n");
    if ix.has_remaining {
        out.push_str("        accounts.extend(self.remaining.iter().cloned());\n");
    }
    let disc = ix
        .discriminator
        .iter()
        .map(|b| format!("{b}u8"))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(out, "        let mut data = vec![{disc}];");
    for arg in &ix.args {
        let _ = writeln!(
            out,
            "        {}",
            arg.ty
                .encode(&format!("self.{}", snake(&arg.name)))
                .unwrap()
        );
    }
    out.push_str("        Instruction { program_id: program_id(), accounts, data }\n");
    out.push_str("    }\n\n");

    out.push_str("    pub fn alias_all(&self, backend: &mut impl TestSVM) {\n");
    for a in &fields {
        let _ = writeln!(
            out,
            "        backend.register_alias(&self.{}, \"{}\");",
            snake(&a.name),
            pascal(&a.name)
        );
    }
    out.push_str("    }\n}\n\n");
}

const WELL_KNOWN_FNS: &str = "\
fn system_program() -> Pubkey { Pubkey::from_str(\"11111111111111111111111111111111\").unwrap() }
fn rent_sysvar() -> Pubkey { Pubkey::from_str(\"SysvarRent111111111111111111111111111111111\").unwrap() }
fn token_program() -> Pubkey { Pubkey::from_str(\"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA\").unwrap() }
fn associated_token_program() -> Pubkey { Pubkey::from_str(\"ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL\").unwrap() }
";

/// A well-known account name -> the injector call emitted for it (filled from a
/// constant rather than exposed as a struct field, the way a bundle injects a
/// `Program`).
fn injector(account: &str) -> Option<&'static str> {
    match account {
        "systemProgram" | "system_program" => Some("system_program()"),
        "rent" => Some("rent_sysvar()"),
        "tokenProgram" | "token_program" => Some("token_program()"),
        "associatedTokenProgram" | "associated_token_program" => Some("associated_token_program()"),
        _ => None,
    }
}

fn meta(pk: &str, signer: bool, writable: bool) -> String {
    if writable {
        format!("AccountMeta::new({pk}, {signer})")
    } else {
        format!("AccountMeta::new_readonly({pk}, {signer})")
    }
}

fn pascal(name: &str) -> String {
    name.split(|c| c == '_' || c == '-')
        .flat_map(split_camel)
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(c) => c.to_ascii_uppercase().to_string() + &cs.as_str().to_ascii_lowercase(),
                None => String::new(),
            }
        })
        .collect()
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if c == '-' {
            out.push('_');
        } else {
            out.push(c);
        }
    }
    out
}

/// Split a camelCase word so `systemProgram` pascal-cases to `SystemProgram`.
fn split_camel(w: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for c in w.chars() {
        if c.is_ascii_uppercase() && !cur.is_empty() {
            parts.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

// --- IDL parsing ------------------------------------------------------------
// Quasar and Anchor IDLs share a JSON shape: `address` plus `instructions`, each
// with a discriminator byte array, `accounts` (name + signer/writable), and
// `args` (name + type, where a type is a scalar string or `{"option": inner}`).
// They differ only in the string length-prefix width (wincode `DynBytes<u8>` =
// u8, borsh `String` = u32), so one parser parameterized by that width serves
// both formats.

#[derive(serde::Deserialize)]
struct RawIdl {
    address: String,
    instructions: Vec<RawIx>,
}

#[derive(serde::Deserialize)]
struct RawIx {
    name: String,
    discriminator: Vec<u8>,
    accounts: Vec<RawAcct>,
    #[serde(default)]
    args: Vec<RawArg>,
    #[serde(rename = "remainingAccounts", default)]
    remaining: Option<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct RawAcct {
    name: String,
    #[serde(default)]
    signer: bool,
    #[serde(default)]
    writable: bool,
}

#[derive(serde::Deserialize)]
struct RawArg {
    name: String,
    #[serde(rename = "type")]
    ty: serde_json::Value,
}

/// Parse an IDL JSON into the normalized model. `string_len` is the one
/// format-dependent piece (wincode `DynBytes<u8>` = `U8`, borsh `String` = `U32`).
fn parse_idl(json: &str, string_len: LenWidth) -> serde_json::Result<(String, Vec<IxDef>)> {
    let raw: RawIdl = serde_json::from_str(json)?;
    let mut instructions: Vec<IxDef> = raw
        .instructions
        .into_iter()
        .map(|ix| IxDef {
            name: ix.name,
            discriminator: ix.discriminator,
            has_remaining: ix.remaining.is_some(),
            accounts: ix
                .accounts
                .into_iter()
                .map(|a| AccountDef {
                    name: a.name,
                    signer: a.signer,
                    writable: a.writable,
                })
                .collect(),
            args: ix
                .args
                .into_iter()
                .map(|a| ArgDef {
                    name: a.name,
                    ty: arg_type(&a.ty, string_len),
                })
                .collect(),
        })
        .collect();
    // quasar-lang's `idl-build` emits the instruction list in hash-map order, so
    // declaration order is not stable run to run; sort by discriminator (unique per
    // instruction, so the order is total) to make the generated client byte-stable
    // to commit. A future declaration-site source carries its own canonical order
    // and need not route through here.
    instructions.sort_by(|a, b| a.discriminator.cmp(&b.discriminator));
    Ok((raw.address, instructions))
}

fn arg_type(v: &serde_json::Value, string_len: LenWidth) -> ArgType {
    match v.as_str() {
        Some("u8") => ArgType::U8,
        Some("u16") => ArgType::U16,
        Some("u32") => ArgType::U32,
        Some("u64") => ArgType::U64,
        Some("i8") => ArgType::I8,
        Some("i16") => ArgType::I16,
        Some("i32") => ArgType::I32,
        Some("i64") => ArgType::I64,
        Some("bool") => ArgType::Bool,
        Some("pubkey") | Some("publicKey") => ArgType::Pubkey,
        Some("string") => ArgType::Bytes { len: string_len },
        // `{"option": <inner>}` — an optional value (1-byte tag, then inner).
        None => match v.get("option") {
            Some(inner) => ArgType::Option(Box::new(arg_type(inner, string_len))),
            // `{"array": [<inner>, <n>]}` — a fixed-size array.
            None => match v.get("array").and_then(|a| a.as_array()) {
                Some(arr) => match (arr.first(), arr.get(1).and_then(|n| n.as_u64())) {
                    (Some(elem), Some(n)) => {
                        ArgType::Array(Box::new(arg_type(elem, string_len)), n as usize)
                    }
                    _ => ArgType::Unsupported(v.to_string()),
                },
                None => ArgType::Unsupported(v.to_string()),
            },
        },
        _ => ArgType::Unsupported(v.to_string()),
    }
}

/// The Quasar IDL format (`quasar idl` emits it): wincode encoding, single-byte
/// discriminators.
pub mod quasar {
    use super::{parse_idl, IdlSource, IxDef, LenWidth};

    /// A parsed Quasar IDL.
    pub struct QuasarIdl {
        address: String,
        instructions: Vec<IxDef>,
    }

    impl QuasarIdl {
        pub fn from_json(json: &str) -> serde_json::Result<Self> {
            let (address, instructions) = parse_idl(json, LenWidth::U8)?;
            Ok(QuasarIdl {
                address,
                instructions,
            })
        }
    }

    impl IdlSource for QuasarIdl {
        fn program_address(&self) -> &str {
            &self.address
        }
        fn instructions(&self) -> Vec<IxDef> {
            self.instructions.clone()
        }
    }
}

/// The Anchor IDL format (Anchor 0.30+ / Codama JSON): borsh encoding, 8-byte
/// discriminators. Structurally identical to the Quasar IDL; only the string
/// length-prefix width differs (borsh `String` is u32-prefixed). The
/// discriminators are embedded in the JSON, so no sighash computation is needed.
pub mod anchor {
    use super::{parse_idl, IdlSource, IxDef, LenWidth};

    /// A parsed Anchor IDL.
    pub struct AnchorIdl {
        address: String,
        instructions: Vec<IxDef>,
    }

    impl AnchorIdl {
        pub fn from_json(json: &str) -> serde_json::Result<Self> {
            let (address, instructions) = parse_idl(json, LenWidth::U32)?;
            Ok(AnchorIdl {
                address,
                instructions,
            })
        }
    }

    impl IdlSource for AnchorIdl {
        fn program_address(&self) -> &str {
            &self.address
        }
        fn instructions(&self) -> Vec<IxDef> {
            self.instructions.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI: &str = r#"{
        "address": "44444444444444444444444444444444444444444444",
        "instructions": [
            { "name": "create", "discriminator": [0],
              "accounts": [
                {"name":"creator","signer":true,"writable":true},
                {"name":"config","writable":true},
                {"name":"systemProgram"}],
              "args": [{"name":"threshold","type":"u8"}],
              "remainingAccounts": {"kind":"append"} },
            { "name": "set_label", "discriminator": [2],
              "accounts": [{"name":"config","writable":true}],
              "args": [{"name":"label","type":"string"}] }
        ]
    }"#;

    #[test]
    fn quasar_flat_args_emit_the_expected_shape() {
        let idl = quasar::QuasarIdl::from_json(MINI).unwrap();
        let out = emit_client(&idl);

        // The flat instruction becomes a named struct: declared accounts as
        // fields, the well-known program injected (not a field), the arg typed,
        // and a remaining tail.
        assert!(out.contains("pub struct Create {"), "{out}");
        assert!(out.contains("pub creator: Pubkey,"));
        assert!(out.contains("pub config: Pubkey,"));
        assert!(
            !out.contains("pub system_program: Pubkey,"),
            "well-known injected, not a field"
        );
        assert!(out.contains("pub threshold: u8,"));
        assert!(out.contains("pub remaining: Vec<AccountMeta>,"));
        // ix() injects the program and discriminator-prefixes the data.
        assert!(out.contains("AccountMeta::new_readonly(system_program(), false)"));
        assert!(out.contains("let mut data = vec![0u8];"));
        assert!(out.contains("data.extend_from_slice(&self.threshold.to_le_bytes());"));
        // alias_all names the declared accounts.
        assert!(out.contains("backend.register_alias(&self.config, \"Config\");"));
        // A string arg comes in as a `String` field with a u8 length prefix
        // (Quasar's wincode DynBytes<u8>), so set_label generates too.
        assert!(out.contains("pub struct SetLabel"), "{out}");
        assert!(out.contains("pub label: String,"));
        assert!(out.contains("data.extend_from_slice(&(self.label.len() as u8).to_le_bytes());"));
    }

    #[test]
    fn an_option_arg_becomes_an_option_field_with_a_tagged_encoding() {
        let idl = r#"{ "address": "11111111111111111111111111111111",
            "instructions": [{ "name": "init", "discriminator": [0],
                "accounts": [{"name":"payer","signer":true,"writable":true}],
                "args": [{"name":"authority","type":{"option":"pubkey"}}] }] }"#;
        let out = emit_client(&quasar::QuasarIdl::from_json(idl).unwrap());
        assert!(out.contains("pub authority: Option<Pubkey>,"), "{out}");
        // 1-byte tag, then the inner pubkey when present.
        assert!(out.contains("match &self.authority"), "{out}");
        assert!(
            out.contains("data.push(1); data.extend_from_slice(v.as_ref());"),
            "{out}"
        );
        assert!(out.contains("data.push(0);"), "{out}");
    }

    #[test]
    fn the_header_carries_no_inner_attrs_so_it_can_be_included_in_a_module() {
        // Inner attributes and inner doc comments in an `include!`-d file are
        // rejected outside the crate root (E0753), so the generated client must
        // carry neither; the including site applies `#[allow(..)]`. A plain `//`
        // banner and no `#![..]` keep the file droppable into a `mod {}`.
        let out = emit_client(&quasar::QuasarIdl::from_json(MINI).unwrap());
        assert!(!out.contains("#!["), "no inner attributes\n{out}");
        assert!(!out.contains("//!"), "no inner doc comments\n{out}");
    }

    #[test]
    fn instructions_emit_in_discriminator_order_regardless_of_declaration_order() {
        // quasar-lang's `idl-build` emits the instruction list in hash-map order,
        // so the same program can produce a differently-ordered JSON run to run.
        // Sorting by discriminator on ingest makes the generated client byte-stable
        // to commit. Here `set_label` (disc 2) is declared before `create` (disc 0);
        // the emitted structs must still come out create-then-set_label.
        let scrambled = r#"{
            "address": "44444444444444444444444444444444444444444444",
            "instructions": [
                { "name": "set_label", "discriminator": [2],
                  "accounts": [{"name":"config","writable":true}],
                  "args": [{"name":"label","type":"string"}] },
                { "name": "create", "discriminator": [0],
                  "accounts": [{"name":"creator","signer":true,"writable":true}],
                  "args": [{"name":"threshold","type":"u8"}] }
            ]
        }"#;
        let out = emit_client(&quasar::QuasarIdl::from_json(scrambled).unwrap());
        let create_at = out.find("pub struct Create").expect("Create emitted");
        let set_label_at = out.find("pub struct SetLabel").expect("SetLabel emitted");
        assert!(
            create_at < set_label_at,
            "disc 0 must precede disc 2 regardless of declaration order\n{out}"
        );
    }

    #[test]
    fn a_vec_arg_stays_past_the_boundary() {
        let idl = r#"{ "address": "11111111111111111111111111111111",
            "instructions": [{ "name": "batch", "discriminator": [9],
                "accounts": [{"name":"payer","signer":true,"writable":true}],
                "args": [{"name":"items","type":{"vec":"u64"}}] }] }"#;
        let out = emit_client(&quasar::QuasarIdl::from_json(idl).unwrap());
        assert!(out.contains("// SKIPPED `batch`"), "{out}");
        assert!(!out.contains("pub struct Batch"));
    }
}
