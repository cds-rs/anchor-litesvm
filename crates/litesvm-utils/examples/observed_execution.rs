//! The execution-observer adapter, end to end.
//!
//! Configure a `LiteSVM` *once* with the observers you want; then every send
//! carries the typed metadata they produce, no matter which call site issued it.
//! This is the prototype of the configurable execution seam: write to one
//! interface (`send`), get a response shaped by the configuration, not by the
//! call site.
//!
//! Run with: `cargo run -p litesvm-utils --example observed_execution`

use {
    litesvm::LiteSVM,
    litesvm_utils::{
        deterministic_keypair, CpiForest, CpiTree, InstructionTrace, ObservedSvm, SignerAuthority,
    },
    solana_program::{
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
    },
    solana_signer::Signer,
    std::str::FromStr,
};

/// A raw System::Transfer instruction (variant 2, a 4-byte u32 LE tag, then the
/// lamports), so the example needs no program of its own.
fn system_transfer(from: &Pubkey, to: &Pubkey, lamports: u64) -> Instruction {
    let system_program = Pubkey::from_str("11111111111111111111111111111111").unwrap();
    let mut data = vec![2u8, 0, 0, 0];
    data.extend_from_slice(&lamports.to_le_bytes());
    Instruction {
        program_id: system_program,
        accounts: vec![AccountMeta::new(*from, true), AccountMeta::new(*to, false)],
        data,
    }
}

fn main() {
    // Configure the svm ONCE. From here on every send carries these observers'
    // output. Register only what you need; an unregistered observer costs nothing.
    let mut svm = ObservedSvm::new(LiteSVM::new())
        .observe(SignerAuthority) // in-flight: per-frame signer / writable / owner facts
        .observe(CpiTree); // post: the structural CPI tree, parsed from the logs

    // Seeded actors (deterministic keypairs) so the addresses are identical on
    // every run and the output below does not churn. That makes the printed run a
    // committable snapshot: a later diff means the behavior changed, not that the
    // keypairs rolled, so any diff is worth scrutinizing rather than ignoring.
    let payer = deterministic_keypair("observed-execution/v1", "payer");
    svm.airdrop(&payer.pubkey(), 5_000_000_000).unwrap(); // a plain LiteSVM call, via Deref
    let recipient = deterministic_keypair("observed-execution/v1", "recipient").pubkey();

    // One interface: send. The shape of what comes back was decided above.
    let observed = svm.send_instructions(
        &[system_transfer(&payer.pubkey(), &recipient, 1_000_000)],
        &[&payer],
    );

    // `observed` *is* the TransactionResult it carries (via Deref), the wrapper
    // is invisible to the result's own API:
    println!("landed:         {}", observed.error().is_none());
    println!("compute units:  {}", observed.compute_units());

    // ...and the observers' metadata rides alongside, read by output type:
    let trace = observed
        .metadata()
        .get::<InstructionTrace>()
        .expect("SignerAuthority was registered");
    println!("\nSignerAuthority hydrated {} frame(s):", trace.0.len());
    for frame in &trace.0 {
        println!(
            "  {}  ({} account(s), stack height {})",
            frame.program_id,
            frame.accounts.len(),
            frame.stack_height,
        );
    }

    let forest = observed
        .metadata()
        .get::<CpiForest>()
        .expect("CpiTree was registered");
    println!(
        "\nCpiTree hydrated {} top-level instruction(s).",
        forest.0.len()
    );

    println!(
        "\nNo call site and no renderer asked for any of this; the registry produced\n\
         it on the send. That is the seam: one interface, a configurable response."
    );
}
