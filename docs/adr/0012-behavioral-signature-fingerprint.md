# ADR 0012: The fingerprint is the behavioral signature: CU in, location out, fold by identity

- Status: Accepted
- Builds on [ADR 0010](0010-file-per-test-capture.md), [ADR 0011](0011-normalization-in-the-reporter.md).
- Relies on the normalized environment of [ADR 0009](0009-observability-parity-vs-operational-character.md).

## Context

The regression gate is a fingerprint: a hash that changes when execution changes
and stays put otherwise. The whole value is in *what it is sensitive to*. Get the
boundary wrong and it either churns on non-changes (noise that trains people to
ignore it) or misses real ones (false safety).

A captured, normalized record carries several fields: identity (group, test
name), location (source anchor, title, file), outcome (verdict, summary), and the
execution (the frame tree, with per-frame CU). Which of these should bind the
hash? Two calls were not obvious.

**CU.** The first instinct was to exclude CU as volatile. That conflated two
things. CU consumed is a function of (program binary, inputs, VM cost model), and
all three are pinned here: the `.so` is a committed fixture, the VM is pinned in
`Cargo.lock`, and inputs are deterministic (actors are seed-derived, and even
`Pubkey::new_unique()` is a per-process counter that resets identically each run,
so even a PDA's bump count, and thus its CU, is stable). CU does not churn
run-to-run. A CU shift means the program, the inputs, or the runtime changed,
which are exactly the things a regression gate should catch. CU is a real signal,
not noise. (The earlier "CU drifts" worry was the docs lesson: hard-coded CU in
prose goes stale. Here, catching the drift *is* the point.)

**Location.** The line a test sits on, its title, its file: none of these are
execution. A refactor that shifts every line, or a rename, must not trip the gate.
We saw this directly: adding imports to the suite shifted every scenario's source
line, and if the anchor were hashed, every test's fingerprint would have moved for
a change that altered nothing. Location is presentation; it belongs in the
rendered index (the Source links), not in the hash.

There is a subtler location leak: the Merkle fold *order*. If group members fold
in source-line order, moving a test below another reorders the fold and churns the
group hash, even with the line excluded from the per-record hash. So the fold must
order by identity, not by location.

## Decision

1. **The per-record fingerprint hashes the behavioral signature only:**
   `{ verdict, summary, frames }`, where each frame keeps its program (role-mapped),
   instruction name, outcome, and CU consumed. This is `NormalRecord::behavioral()`.
2. **CU is in.** It is deterministic in this pinned environment, so a CU shift is a
   real signal (a perf regression, a rebuild, a VM bump), not run-to-run noise.
3. **Location is out.** `anchor`, `title`, and `test_file` are excluded from the
   hash; they stay in `NormalRecord` for the rendered index (Source links,
   headings) but do not bind the gate.
4. **The group fold orders members by `test_name`** (identity), not by anchor, so
   code movement and reordering never churn the hash. The index still *displays* in
   source order.
5. The identity key is `(group, test_name)`; the encoding is JCS canonical JSON
   (sort object keys; preserve array order, because frame order is behavior; no
   lowercasing, because case is signal; integers exact); the fold is per-record ->
   per-group -> suite Merkle root.

## Consequences

- A code move or rename leaves the fingerprint unchanged; only the index's Source
  links update (reviewed, not gated). The import shift that moved every anchor in
  the mpl-core suite produced zero fingerprint change.
- A CU shift trips the gate. The committed fingerprint is therefore valid for a
  specific (program `.so`, VM version) pair; bump either and the fingerprint
  legitimately moves, and you rebaseline. The `verify` diff embeds the captured
  JSON so the rebaseline is informed. (Recording the `(program.so, vm_version)`
  pair *adjacent* to the root, so a CU delta is self-explaining rather than
  mysterious, is the v1.1 follow-on.)
- The big-number edge is proptest-proven, not asserted: `u64` round-trips exactly
  through the canonical encoder, and two CU values straddling the f64 boundary
  (`2^53+1` vs `2^53+2`) hash distinctly through the real struct path. `serde_json`
  keeps `u64` exact, so the gate is precise where a naive float path would collide.
- The verdict is the *test's* pass/fail, not the transaction's: a correct rejection
  (an `Expect::Rejects` scenario that the program rejects) reads as "passed," never
  a bare misleading `false`.

## Alternatives considered

- **Exclude CU.** Rejected: CU is deterministic in this pinned environment, so a
  shift is real signal; excluding it makes the gate blind to perf regressions and
  to silent program/VM changes.
- **Include location in the hash.** Rejected: location is presentation; hashing it
  churns the gate on every refactor that moves a line, the exact false positive the
  normalization layer ([ADR 0011](0011-normalization-in-the-reporter.md)) exists to
  prevent.
- **Dual hashes (with-range and without-range) to detect location-only changes.**
  Rejected: it adds machinery to *tolerate* a change we can *remove* from the gate
  entirely. Once location is out, a move is invisible to the gate; if you want to
  see a move, the v1.1 history diff shows it from the full corpus.

## References

- The behavioral view + normalize:
  [`crates/testsvm/src/report/normalize.rs`](../../crates/testsvm/src/report/normalize.rs).
- Canonical JSON + sha256:
  [`crates/testsvm/src/report/canonical.rs`](../../crates/testsvm/src/report/canonical.rs).
- Merkle fold + manifest + diff:
  [`crates/testsvm/src/report/fingerprint.rs`](../../crates/testsvm/src/report/fingerprint.rs).
- Big-number / variance / idempotence property tests:
  [`crates/testsvm/src/report/proptests.rs`](../../crates/testsvm/src/report/proptests.rs).
- The pinned environment: [ADR 0009](0009-observability-parity-vs-operational-character.md)
  (`EnvironmentConfig`).
- Narrative: [`../design/observation-report.md`](../design/observation-report.md).
