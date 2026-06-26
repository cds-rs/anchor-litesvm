# ADR 0010: File-per-test capture; the report is a byproduct of the real suite

- Status: Accepted
- Applies the premise in [ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md).

## Context

A test suite's value is not only pass/fail; each execution carries a structured
story (the CPI tree, who signed for what, what each frame cost). We wanted a
committable per-suite report and a regression fingerprint built from that story.
The first instinct is an in-memory collector: each `#[test]` pushes its
observation into a `static` accumulator, and a `Drop` guard emits the report at
process exit.

That instinct breaks on how Rust tests actually run. `cargo test` compiles each
`tests/*.rs` to a *separate* binary and runs them as separate OS processes;
`cargo nextest` goes further and runs each `#[test]` in its *own* process. An
in-memory accumulator collects exactly one record per process: under nextest it
never accumulates more than one, and the `Drop` guard fires per test. The
single-process assumption an in-memory collector rests on is false under the
execution strategies people actually use. The only medium shared across those
processes is the filesystem.

There is a second reason, independent of process boundaries. If the report is
rendered by a parallel set of "render this scenario" functions (a lookalike of
the real tests), it can drift: the report can render a passing world while the
suite fails. The report should be a byproduct of the tests that actually ran.

## Decision

1. **Each test emits one lossless JSON record** to `target/test-results/<slug>.json`
   via `testsvm::report::record(Observation { .. })`, called inside the real test
   as it runs. Capture is per-test and process-local; no shared memory.
2. **A separate Reporter pass aggregates** the corpus into the rendered report and
   the fingerprint. How it normalizes and folds is [ADR 0011](0011-normalization-in-the-reporter.md)
   and [ADR 0012](0012-behavioral-signature-fingerprint.md).
3. **The record is lossless and self-describing.** It carries the execution facts
   *and* the alias table in effect, so the Reporter resolves addresses to roles
   with no live engine. It is `schema_version`-stamped so a stale schema is
   rejected, not misread.

## Consequences

- The capture survives `cargo test`, `cargo nextest`, split binaries, and CI
  alike: each is just "more processes writing files." The design has no
  single-process assumption to violate.
- The report is sourced from the real suite, so it cannot render a passing world
  the suite would fail. There is no parallel render-fn set to drift.
- The corpus is transient (`target/`, gitignored); the committed artifacts are
  the rendered markdown and the fingerprint. Staleness (a deleted test's file
  lingering) is the Reporter's concern: a full run cleans `target/test-results/`
  first, so a stale record cannot pollute the gate.
- A consequence that pays off in [ADR 0011](0011-normalization-in-the-reporter.md):
  because the corpus sits on disk, the Reporter can be re-run *without* re-running
  the suite (corpus-as-cache).

## Alternatives considered

- **In-memory collector + `Drop` guard.** Rejected: nextest is process-per-test,
  so the collector gathers one record per process and disintegrates. It encodes a
  single-process assumption the common tools violate, and degrades silently (a
  one-line "report" per test) rather than failing loudly.
- **A single `harness = false` binary that owns every scenario.** Rejected: it
  re-implements the suite as one process, reintroducing the drift (the report is
  no longer the real tests) and forfeiting per-test parallelism.

## References

- `record` / `Observation` / `ReportRecord`:
  [`crates/testsvm/src/report/observation.rs`](../../crates/testsvm/src/report/observation.rs).
- The Reporter:
  [`crates/testsvm/src/report/reporter.rs`](../../crates/testsvm/src/report/reporter.rs).
- Narrative: [`../design/observation-report.md`](../design/observation-report.md).
