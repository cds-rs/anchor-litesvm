//! The probe spec against a live surfnet (surfpool) over JSON-RPC. Same spec,
//! different engine. The only seam: surfpool deploys the counter, so this calls
//! `run_counter_spec` (no in-process deploy) instead of `run_counter_probe`.
//!
//! ```sh
//! cd md-babelfish/listings/counter && surfpool start --no-tui   # deploys counter.so
//! # then, from this crate:
//! cargo run --example probe --features rpc
//! ```
//!
//! Live-only: it needs a running surfnet, so it is an example, not a CI test.

#[cfg(not(feature = "rpc"))]
fn main() {
    eprintln!("rebuild with `--features rpc`, and start a surfnet first (see the module docs)");
}

// ANCHOR: example
#[cfg(feature = "rpc")]
fn main() {
    use litesvm_utils::RpcBackend;

    let url = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".to_string());
    println!("counter spec over surfnet at {url}");

    // surfpool has already deployed the counter at COUNTER_ID; run the same spec
    // every in-memory runner ran, now over JSON-RPC.
    let mut engine = RpcBackend::new(url);
    let run = probe_spec::run_counter_spec(&mut engine);

    assert!(run.initialize.error.is_none(), "initialize: {:?}", run.initialize.error);
    assert!(run.increment.error.is_none(), "increment: {:?}", run.increment.error);
    assert_eq!(run.final_count, Some(1), "the counter spec reads 1 on every engine");
    println!("counter spec passed on the surfnet");
}
// ANCHOR_END: example
