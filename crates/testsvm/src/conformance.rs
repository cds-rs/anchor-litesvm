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

    // The relocated renderers run on every engine's record, not just litesvm's.
    // The authority graph draws the transfer's top-level roles (signer + the
    // account it writes), which come from the message, so it renders the same on
    // every engine. Edges emit as Mermaid `-->|verb|`.
    let authority = tx.authority_graph_string();
    assert!(
        authority.contains("|signs|") && authority.contains("|writes|"),
        "authority graph renders the transfer's roles on every engine:\n{authority}"
    );

    // The ownership graph is the trace-gated one: an account's owner is not in
    // the message or the logs, only in the per-frame trace. A trace-capable
    // engine names the destination's owner (`-->|owns|`); one without degrades
    // to no owner edges, the graceful path the relocation documents.
    let ownership = tx.ownership_graph_string();
    if caps.per_frame_trace {
        assert!(
            ownership.contains("|owns|"),
            "trace-sourced owners reach the ownership graph:\n{ownership}"
        );
    } else {
        assert!(
            !ownership.contains("|owns|"),
            "without a trace the ownership graph has no owner to draw:\n{ownership}"
        );
    }

    // The lever that could not go cross-engine before this trait.
    backend.warp_to_timestamp(1_700_000_000);
    assert_eq!(backend.clock().unix_timestamp, 1_700_000_000);
}
