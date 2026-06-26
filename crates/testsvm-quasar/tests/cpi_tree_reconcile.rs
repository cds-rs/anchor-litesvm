//! Reconciliation: the instruction name the adapter takes from the LOGS must
//! agree with the name decoded from the instruction DATA discriminator in the
//! structured trace. Two independent witnesses (Anchor's `Program log:
//! Instruction:` line and its 8-byte `sha256("global:<name>")` discriminator),
//! one judge (this test). Establishes equivalence and tracks regression: if
//! quasar's log format or the trace's data ever drift apart, this fails.

use {
    solana_instruction::{AccountMeta, Instruction},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    std::str::FromStr,
    testsvm::TestSVM,
    testsvm_quasar::QuasarBackend,
};

const COUNTER_SO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/counter.so");

// Anchor discriminators: sha256("global:<name>")[..8]. The map is the program's
// instruction set; decoding reverses data -> name against it.
const COUNTER_IXS: &[([u8; 8], &str)] = &[
    ([175, 175, 109, 31, 13, 152, 155, 237], "initialize"),
    ([11, 18, 104, 9, 104, 174, 59, 33], "increment"),
];

fn counter_id() -> Pubkey {
    Pubkey::from_str("8E6a1bwRyKjw8YhXYPspSUStESC7mKNkG5hAzz8oERPj").unwrap()
}
fn system_program() -> Pubkey {
    Pubkey::from_str("11111111111111111111111111111111").unwrap()
}

/// Decode an instruction's name from its 8-byte Anchor discriminator.
fn name_from_data(data: &[u8]) -> Option<&'static str> {
    let disc: [u8; 8] = data.get(..8)?.try_into().ok()?;
    COUNTER_IXS.iter().find(|(d, _)| *d == disc).map(|(_, n)| *n)
}

/// Extract the instruction name Anchor's dispatcher logs.
fn name_from_logs(logs: &[String]) -> Option<String> {
    logs.iter()
        .find_map(|l| l.strip_prefix("Program log: Instruction: ").map(str::to_string))
}

#[test]
fn data_discriminator_reconciles_with_logged_name() {
    let mut backend = QuasarBackend::new();
    backend.deploy_from_file(&counter_id(), COUNTER_SO, "counter");
    let alice = backend.actor("Alice", 10_000_000_000);
    let counter =
        Pubkey::find_program_address(&[b"counter", alice.pubkey().as_ref()], &counter_id()).0;

    // initialize(start = 0): accounts [payer, counter, system].
    let mut init_data = COUNTER_IXS[0].0.to_vec();
    init_data.extend_from_slice(&0u64.to_le_bytes());
    let init = Instruction {
        program_id: counter_id(),
        accounts: vec![
            AccountMeta::new(alice.pubkey(), true),
            AccountMeta::new(counter, false),
            AccountMeta::new_readonly(system_program(), false),
        ],
        data: init_data,
    };
    // increment(): accounts [counter, payer].
    let increment = Instruction {
        program_id: counter_id(),
        accounts: vec![
            AccountMeta::new(counter, false),
            AccountMeta::new(alice.pubkey(), true),
        ],
        data: COUNTER_IXS[1].0.to_vec(),
    };

    for (ix, expected) in [(init, "initialize"), (increment, "increment")] {
        let tx = backend.send(&[ix], &[&alice]);
        assert!(tx.error.is_none(), "{expected} should succeed: {:?}", tx.error);

        // Witness 1: the data discriminator on the top-level counter frame.
        let trace = tx.trace.as_ref().expect("quasar fills the structured trace");
        let top = trace
            .0
            .iter()
            .find(|t| t.program_id == counter_id() && t.stack_height == 1)
            .expect("the top-level counter frame");
        let decoded = name_from_data(&top.data).expect("a known counter discriminator");

        // Witness 2: the name Anchor logged.
        let logged = name_from_logs(&tx.logs).expect("Anchor logs the instruction name");

        // The judge: they must name the same instruction (Anchor logs PascalCase,
        // the discriminator hashes the snake_case method, so compare case-insensitively).
        assert!(
            decoded.eq_ignore_ascii_case(&logged),
            "name drift: data discriminator decodes to {decoded:?}, logs say {logged:?}"
        );
        assert_eq!(decoded, expected);
    }
}
