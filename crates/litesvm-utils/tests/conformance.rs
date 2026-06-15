//! Same test, this backend: the litesvm build of the conformance scenario.

#[test]
fn conformance_on_litesvm() {
    let mut backend = litesvm_utils::LiteSvmBackend::new(litesvm::LiteSVM::new());
    testsvm::conformance::scenario(&mut backend);
}
