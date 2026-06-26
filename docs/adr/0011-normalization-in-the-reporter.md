# ADR 0011: Normalization runs in the Reporter; overrides are free functions, not a trait

- Status: Accepted
- Builds on [ADR 0010](0010-file-per-test-capture.md).
- Departs, locally and deliberately, from [ADR 0002](0002-one-engine-port-as-a-trait.md) / [ADR 0003](0003-vocabulary-on-the-backend-as-trait-sockets.md).

## Context

A captured record ([ADR 0010](0010-file-per-test-capture.md)) is raw: program ids
as base58, CU counts, the alias table, source locations. Before it can be rendered
or fingerprinted it must be *normalized* into a canonical form (addresses to role
labels, presentation stripped). Two questions follow: *where* does normalization
run, and *how* does a maintainer customize it for their program.

**Where.** Normalization could run at capture time (the test normalizes before
emitting) or at Reporter time (the raw record is emitted, the Reporter
normalizes). Capture-time looks simpler, but it bakes the normalization policy
into the corpus: changing a rule means re-running the whole suite to regenerate
every record. Reporter-time keeps the corpus raw, so the corpus becomes a *cache*:
tweak a normalization or fingerprint rule and re-run only the Reporter over the
existing JSON. That matters because normalization policy is exactly the thing you
iterate on; the slow part (executing the programs) should happen once per change
to the *tests*, not once per change to a *rule*. (Reporter-time normalization is
possible only because the record is lossless including the alias table, which
[ADR 0010](0010-file-per-test-capture.md) already requires.)

**How.** The rest of the framework customizes behavior through traits with
per-engine impls ([ADR 0002](0002-one-engine-port-as-a-trait.md)'s `TestSVM`,
[ADR 0003](0003-vocabulary-on-the-backend-as-trait-sockets.md)'s vocabulary
sockets). The reflex is a `Normalize` trait. That reflex is wrong here, for a
concrete reason: every test emits the *same* `Record` type. There are no distinct
per-program types to hang trait impls on; a trait would have exactly one impl.
Dispatch has to key on *data* (which group, which test), not on the type. A
registry of free functions keyed by group/test is exactly that.

## Decision

1. **Normalization runs in the Reporter**, over the on-disk corpus. The corpus is
   a cache: a rule change re-runs only the Reporter, not the suite.
2. **The default transform carries the universal, engine-neutral
   canonicalizations** (program ids to role labels via the captured alias table;
   the behavioral projection of [ADR 0012](0012-behavioral-signature-fingerprint.md)).
   These are true for every Solana program, so they live in the default, not in
   every maintainer's override.
3. **Program-specific shaping is a free function** registered with the Reporter,
   keyed by group/test (`Reporter::override_records`), not a `Normalize` trait. One
   `Record` type means dispatch on data, not on types.

## Consequences

- Retuning normalization or the fingerprint does not re-run the suite. The slowest
  work (executing the programs) happens once per change to the tests, not once per
  change to a rule.
- The override surface stays small and honest: the default handles what is
  universal; a maintainer writes a function only for genuine program-specific
  shaping (bucket an amount, fold plugin state, drop a nonce).
- This is a deliberate *local* departure from the trait pattern. The pattern is
  right when impls differ by type (one per engine); it is wrong when there is one
  type and dispatch is by data. We record the departure so a future reader does
  not "fix" it into a single-impl trait.

## Alternatives considered

- **Capture-time normalization.** Rejected: it bakes policy into the corpus,
  forfeiting corpus-as-cache; every rule change costs a full suite run.
- **A `Normalize` trait with per-program impls.** Rejected: there is one `Record`
  type, so the trait would carry a single impl and could not dispatch on the
  program. Free functions keyed by group/test are the honest mechanism.

## References

- `normalize_default`, the default transform:
  [`crates/testsvm/src/report/normalize.rs`](../../crates/testsvm/src/report/normalize.rs).
- `Reporter::override_records`:
  [`crates/testsvm/src/report/reporter.rs`](../../crates/testsvm/src/report/reporter.rs).
- The trait pattern this departs from: [ADR 0002](0002-one-engine-port-as-a-trait.md),
  [ADR 0003](0003-vocabulary-on-the-backend-as-trait-sockets.md).
- Narrative: [`../design/observation-report.md`](../design/observation-report.md).
