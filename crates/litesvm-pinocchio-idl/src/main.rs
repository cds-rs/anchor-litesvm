//! CLI over the [`litesvm_pinocchio_idl`] library: extract a Pinocchio program's
//! Anchor IDL from its source and print it (or write it with `-o`).
//!
//! Usage: `litesvm-pinocchio-idl --crate-root <dir> [--program-id <pubkey>] [--name <program>] [-o <file>]`

use {
    litesvm_pinocchio_idl::idl_from_crate,
    std::path::PathBuf,
};

fn main() {
    let mut crate_root = PathBuf::from(".");
    let mut program_id = "11111111111111111111111111111111".to_string();
    let mut name: Option<String> = None;
    let mut out: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-r" | "--crate-root" => {
                crate_root = PathBuf::from(args.next().expect("--crate-root needs a value"))
            }
            "-p" | "--program-id" => program_id = args.next().expect("--program-id needs a value"),
            "-n" | "--name" => name = Some(args.next().expect("--name needs a value")),
            "-o" | "--out" => out = Some(PathBuf::from(args.next().expect("--out needs a value"))),
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    let idl = match idl_from_crate(&crate_root, &program_id, name) {
        Ok(idl) => idl,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let text = serde_json::to_string_pretty(&idl).expect("serialize IDL");
    match out {
        Some(path) => {
            std::fs::write(&path, text).expect("write IDL");
            eprintln!("wrote {}", path.display());
        }
        None => println!("{text}"),
    }
}
