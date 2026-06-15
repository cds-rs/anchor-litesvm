//! Live smoke test for `RpcBackend` against a running surfnet.
//!
//! Start a surfnet (`surfpool start --no-tui`), then:
//! ```sh
//! cargo run -p litesvm-utils --features rpc --example rpc_smoke
//! ```
//! It airdrops a fresh payer, sends a System transfer through `RpcBackend`, and
//! renders the returned logs as a CPI tree, proving the RPC plumbing
//! (blockhash → simulate → send → logs → tree) works end to end against a real
//! surfnet. `RPC_URL` overrides the default `http://127.0.0.1:8899`.

#[cfg(not(feature = "rpc"))]
fn main() {
    eprintln!("rebuild with `--features rpc`");
}

#[cfg(feature = "rpc")]
fn main() {
    use {
        litesvm::cpi_tree::{cpi_tree, format_cpi_tree},
        litesvm_utils::{RpcBackend, TestSVM},
        solana_keypair::Keypair,
        solana_program::pubkey::Pubkey,
        solana_signer::Signer,
    };

    let url = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".to_string());
    println!("connecting to surfnet at {url}");
    let mut backend = RpcBackend::new(url);

    let payer = Keypair::new();
    backend.fund_sol(&payer.pubkey(), 1_000_000_000);
    // Airdrop is async on the surfnet; wait for it to land before spending.
    for _ in 0..50 {
        if backend.client().get_balance(&payer.pubkey()).unwrap_or(0) > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // A System transfer above the rent floor: one top-level frame the runtime
    // logs, so the tree has a node.
    let dest = Pubkey::new_unique();
    let ix = solana_system_interface::instruction::transfer(&payer.pubkey(), &dest, 2_000_000);
    let record = backend.send(&[ix], &[&payer]);

    println!("error:         {:?}", record.error);
    println!("compute_units: {}", record.compute_units);
    println!("trace present: {}", record.trace.is_some());
    println!("capabilities:  {:?}", backend.capabilities());
    let frames = cpi_tree(&record.logs);
    println!(
        "\n{}",
        format_cpi_tree("smoke: system transfer via RpcBackend", &frames)
    );
}
