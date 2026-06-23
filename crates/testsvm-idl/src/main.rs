//! `testsvm-idl <idl.json> <out.rs>`: parse a Quasar IDL and emit its client.
//!
//! Quasar is the only format wired into the binary today; adding Shank/Codama
//! is a new [`IdlSource`](testsvm_idl::IdlSource) impl plus a dispatch arm here.

use {
    std::{fs, process::exit},
    testsvm_idl::{emit_client, quasar::QuasarIdl},
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, idl_path, out_path] = args.as_slice() else {
        eprintln!("usage: testsvm-idl <idl.json> <out.rs>");
        exit(2);
    };

    let json = fs::read_to_string(idl_path).unwrap_or_else(|e| {
        eprintln!("read {idl_path}: {e}");
        exit(1);
    });
    let idl = QuasarIdl::from_json(&json).unwrap_or_else(|e| {
        eprintln!("parse {idl_path}: {e}");
        exit(1);
    });
    fs::write(out_path, emit_client(&idl)).unwrap_or_else(|e| {
        eprintln!("write {out_path}: {e}");
        exit(1);
    });
    eprintln!("wrote {out_path}");
}
