//! Conformance scenarios: the shared artifact of cross-engine testing.
//! Generic over [`TestSVM`]; owned by neither engine nor program framework.
//! Each adapter crate runs these in its OWN tests, in its own dependency
//! graph: same test, different backend, rebuild. Engines never meet in one
//! graph.

use {crate::TestSVM, solana_pubkey::Pubkey, solana_signer::Signer};

/// Fund a payer, send a System transfer, assert the model, warp the clock.
/// Capability-gated assertions cover the engine differences.
pub fn scenario<B: TestSVM>(backend: &mut B) {
    // One declaration, three effects: deterministic keypair, alias, funding.
    let payer = backend.actor("payer", 1_000_000_000);

    let dest = Pubkey::new_unique();
    let ix = solana_system_interface::instruction::transfer(&payer.pubkey(), &dest, 2_000_000);
    let tx = backend.send(&[ix], &[&payer]);

    // Universal: every engine yields a model the renderers can consume.
    assert!(
        tx.error.is_none(),
        "transfer should succeed: {:?}",
        tx.error
    );
    assert!(
        !tx.frames.is_empty(),
        "structured frames present on every engine"
    );
    assert!(
        !tx.account_keys.is_empty(),
        "indices never ship without their frame"
    );
    assert!(!tx.logs.is_empty(), "logs are the floor on every engine");
    assert_eq!(
        backend.account_owner(&dest),
        Some(solana_system_interface::program::id()),
    );

    // Capability-gated: engines differ, and say so.
    let caps = backend.capabilities();
    if caps.fees {
        assert!(tx.fee.is_some(), "engines that model fees report them");
    } else {
        assert!(tx.fee.is_none(), "absent facts are absent, not zero");
    }
    if caps.per_frame_trace {
        assert!(tx.trace.is_some(), "trace-capable engines populate it");
    }

    // The lever that could not go cross-engine before this trait.
    backend.warp_to_timestamp(1_700_000_000);
    assert_eq!(backend.clock().unix_timestamp, 1_700_000_000);
}
