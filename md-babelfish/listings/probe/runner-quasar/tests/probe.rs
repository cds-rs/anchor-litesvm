//! The probe spec on quasar-svm. Same spec, different engine.

// ANCHOR: runner
use {testsvm::TestSVM, testsvm_quasar::QuasarBackend};

const COUNTER_SO: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../counter/target/deploy/counter.so");
const GOLDEN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../golden");

#[test]
fn counter_on_quasar() {
    let mut engine = QuasarBackend::new();
    let run = probe_spec::run_counter_probe(&mut engine, COUNTER_SO);

    assert!(run.initialize.error.is_none(), "initialize: {:?}", run.initialize.error);
    assert!(run.increment.error.is_none(), "increment: {:?}", run.increment.error);
    assert_eq!(run.final_count, Some(1), "the counter spec reads 1 on every engine");

    let caps = engine.capabilities();
    probe_spec::assert_observability_conformance(&run, &caps, GOLDEN_DIR);
}
// ANCHOR_END: runner
