//! Run the counter against a live surfnet instead of the in-memory engine.
//!
//! The body below is the counter test with one line changed: `.with_backend(...)`.
//! Sends route over JSON-RPC to surfpool; the bundle, the build, the send are
//! identical. Drop the `with_backend` call and the same calls run in-memory.
//!
//! ```sh
//! surfpool start --no-tui          # in book/listings/counter; deploys the program
//! cargo run --example surfpool --features rpc
//! ```

#[cfg(not(feature = "rpc"))]
fn main() {
    eprintln!("rebuild with `--features rpc` (and start a surfnet first)");
}

// ANCHOR: example
// programs/counter/examples/surfpool.rs
#[cfg(feature = "rpc")]
fn main() {
    use anchor_litesvm::{AnchorContext, Keypair, LiteSVM, RpcBackend, Signer, TestHelpers, TestSVM};
    use counter::{
        instruction as vix,
        test_helpers::{IncrementBundle, InitializeBundle},
    };

    let url = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".to_string());
    println!("counter over surfnet at {url}");

    // A fresh payer, airdropped on the surfnet, so each run gets a fresh PDA.
    let payer = Keypair::new();
    let mut backend = RpcBackend::new(url);
    backend.fund_sol(&payer.pubkey(), 1_000_000_000);
    for _ in 0..50 {
        if backend.client().get_balance(&payer.pubkey()).unwrap_or(0) > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // The one line that moves the counter off the in-memory engine: a backend.
    // Everything below is identical to the in-memory test.
    let mut ctx = AnchorContext::new(LiteSVM::new(), counter::ID).with_backend(Box::new(backend));

    let counter = ctx.svm.get_pda(&[b"counter", payer.pubkey().as_ref()], &counter::ID);
    ctx.alias(payer.pubkey(), "Alice").alias(counter, "Counter");

    let accs = InitializeBundle { payer: payer.pubkey(), counter };
    ctx.tx(&[&payer]).build(accs, vix::Initialize { start: 0 }).send_ok();

    let accs = IncrementBundle { counter, payer: payer.pubkey() };
    ctx.tx(&[&payer]).build(accs, vix::Increment {}).send_ok();

    println!("initialized and incremented the counter on the surfnet");
}
// ANCHOR_END: example
