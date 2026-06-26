# Architecture Decision Records

These record the decisions the testing framework rests on: the ones already
made and shipped, not proposals. Each is a short
[Nygard-style](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions.html)
record (Status, Context, Decision, Consequences, Alternatives), subject to
review.

The decisions were made across the framework's development; this folder
*solidifies* them, with each Context drawn from the prose design docs and the
commit history that produced it. The narrative docs explain how the pieces fit
together; these explain why each piece is shaped the way it is.

| ADR | Decision |
|---|---|
| [0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md) | **The premise**: trait boundaries for composability across engines, ecosystem service-visibility, and fork-and-adapt to iterate without interrupting maintainers (read first) |
| [0001](0001-executor-owns-the-execution-vocabulary.md) | The executor owns the execution vocabulary; consumers name and render it |
| [0002](0002-one-engine-port-as-a-trait.md) | One engine port as a trait (`TestSVM`), not an enum of engines |
| [0003](0003-vocabulary-on-the-backend-as-trait-sockets.md) | The test vocabulary lives on the backend, registered through trait sockets |
| [0004](0004-engine-neutral-record-and-vocabulary-propagation.md) | One engine-neutral record; one builder; `From` carries the vocabulary up |
| [0005](0005-no-framework-dependency-decoders-as-closures.md) | The vocabulary crate names no program framework; decoders cross as closures |
| [0006](0006-thin-vocabulary-crate-engines-own-their-lockfile.md) | The vocabulary crate is thin; an engine that can't share a lockfile is its own workspace |
| [0007](0007-declare-the-trace-asymmetry-via-capabilities.md) | Declare the per-frame-trace asymmetry through capabilities; don't hide it |
| [0008](0008-key-self-cpi-events-by-program.md) | Key self-CPI event decoders by program |
| [0009](0009-observability-parity-vs-operational-character.md) | Split capability flags into observability and operational; observability must reach render parity, operational gates scenarios (refines 0007) |
| [0010](0010-file-per-test-capture.md) | File-per-test capture (survives nextest's process-per-test); the report is a byproduct of the real suite, not a parallel render set |
| [0011](0011-normalization-in-the-reporter.md) | Normalization runs in the Reporter (corpus-as-cache); overrides are free functions keyed by data, not a `Normalize` trait (one `Record` type) |
| [0012](0012-behavioral-signature-fingerprint.md) | The fingerprint is the behavioral signature: CU in (deterministic signal), location out (presentation), fold by `test_name` so code movement never churns it |
| [0013](0013-baseline-tarball-history-is-git.md) | A committed `baseline.tar.gz` gives content diffs without page churn; the history is git, not a run-store (the run-history ring buffer was dropped as redundant) |

## Narrative companions

- [`../design/observation-report.md`](../design/observation-report.md): the
  capture -> normalize -> fingerprint -> report pipeline and how a consumer drives
  it (narrative for ADR 0010, 0011, 0012).
- [`../design/trait-boundaries.md`](../design/trait-boundaries.md): how the
  layers, the port, and the vocabulary fit together (the overview these ADRs
  decompose).
- [`../design/litesvm-boundary.md`](../design/litesvm-boundary.md): the
  executor/consumer line (ADR 0001, 0007).
- [`../design/endpoint-agnostic-architecture.md`](../design/endpoint-agnostic-architecture.md):
  the surfpool-era origin, under the pre-rename `ExecutionBackend` names
  (provenance for ADR 0002).
- [`../design/pinocchio-test-evolution.md`](../design/pinocchio-test-evolution.md):
  the adoption arc of a real Pinocchio suite onto the vocabulary (narrative for
  ADR 0001).
- [`../design/litesvm-upstream-collaboration.md`](../design/litesvm-upstream-collaboration.md):
  the upstream tracker survey behind the fork-and-adapt premise (ADR 0000).
