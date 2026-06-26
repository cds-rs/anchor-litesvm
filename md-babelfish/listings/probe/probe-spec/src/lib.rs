//! The probe specification: one counter scenario, written once in the TestSVM
//! vocabulary, run against any engine. The test is the specification; the
//! runtime answers to it.

// ANCHOR: spec
use {
    solana_instruction::{AccountMeta, Instruction},
    solana_pubkey::{pubkey, Pubkey},
    solana_signer::Signer,
    testsvm::{model, TestSVM},
};

/// The counter program under test: the same `.so`, loaded into every engine.
pub const COUNTER_ID: Pubkey = pubkey!("8E6a1bwRyKjw8YhXYPspSUStESC7mKNkG5hAzz8oERPj");
const SYSTEM_PROGRAM: Pubkey = pubkey!("11111111111111111111111111111111");

// Anchor's 8-byte instruction discriminators: `sha256("global:<name>")[..8]`.
const INITIALIZE: [u8; 8] = [175, 175, 109, 31, 13, 152, 155, 237];
const INCREMENT: [u8; 8] = [11, 18, 104, 9, 104, 174, 59, 33];

/// What one runtime answered for the counter spec.
pub struct ProbeRun {
    /// The initialize transaction, as the engine witnessed it.
    pub initialize: model::Transaction,
    /// The increment transaction.
    pub increment: model::Transaction,
    /// The counter's `count`, read back from the account after both ran.
    pub final_count: Option<u64>,
}

/// Deploy the counter from its `.so`, then run the spec. In-memory engines
/// (litesvm, mollusk, quasar) deploy in-process; over a stock RPC the program
/// is deployed by the surfnet first, so that path calls [`run_counter_spec`]
/// directly. Deploy is the one step that differs by engine; the spec does not.
pub fn run_counter_probe<E: TestSVM>(engine: &mut E, counter_so: &str) -> ProbeRun {
    // Normalize the execution environment before the first send: a declarative
    // config the engine reads, pinning the compute-unit ceiling so every engine
    // runs the same budget. Without this, litesvm's solana graph defaults to
    // 200,000 and agave 4.0 reports 1,400,000, and that lone difference breaks
    // render parity. The default config carries the normalized ceiling; each
    // engine honors it through its own knob.
    engine.configure(&testsvm::EnvironmentConfig::default());
    engine.deploy_from_file(&COUNTER_ID, counter_so, "counter");
    run_counter_spec(engine)
}

/// The specification, against an engine that already has the counter deployed:
/// initialize at 0, increment once, read the count back. The engine is the only
/// variable; this is identical for all.
pub fn run_counter_spec<E: TestSVM>(engine: &mut E) -> ProbeRun {
    let alice = engine.actor("Alice", 10_000_000_000);
    let counter =
        Pubkey::find_program_address(&[b"counter", alice.pubkey().as_ref()], &COUNTER_ID).0;
    engine.register_alias(&counter, "Counter");

    let mut init_data = INITIALIZE.to_vec();
    init_data.extend_from_slice(&0u64.to_le_bytes());
    let initialize = engine.send(
        &[Instruction {
            program_id: COUNTER_ID,
            accounts: vec![
                AccountMeta::new(alice.pubkey(), true),
                AccountMeta::new(counter, false),
                AccountMeta::new_readonly(SYSTEM_PROGRAM, false),
            ],
            data: init_data,
        }],
        &[&alice],
    );

    let increment = engine.send(
        &[Instruction {
            program_id: COUNTER_ID,
            accounts: vec![
                AccountMeta::new(counter, false),
                AccountMeta::new(alice.pubkey(), true),
            ],
            data: INCREMENT.to_vec(),
        }],
        &[&alice],
    );

    let final_count = engine
        .get_account(&counter)
        .map(|a| u64::from_le_bytes(a.data[8..16].try_into().unwrap()));

    ProbeRun { initialize, increment, final_count }
}
// ANCHOR_END: spec

/// Assert the probe's initialize transaction conforms: its structured record
/// satisfies the observability invariants, and its CPI tree + authority graph
/// match the shared golden. Every in-memory engine calls this against the same
/// `golden_dir`, so a divergence on any engine fails the golden the others pass.
pub fn assert_observability_conformance(
    run: &ProbeRun,
    caps: &testsvm::Capabilities,
    golden_dir: &str,
) {
    let tx = &run.initialize;

    if let Err(violations) =
        testsvm::conformance::validate_observability(&tx.frames, tx.trace.as_ref(), caps)
    {
        let joined = violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n - ");
        panic!("observability record invalid:\n - {joined}");
    }

    testsvm::conformance::assert_golden(
        &format!("{golden_dir}/counter_cpi_tree.txt"),
        &tx.pretty_cpi_tree(),
    );
    testsvm::conformance::assert_golden(
        &format!("{golden_dir}/counter_authority_graph.txt"),
        &tx.authority_graph_string(),
    );
}
