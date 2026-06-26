# ADR 0013: A committed baseline tarball for content diffs; the history is git, not a run store

- Status: Accepted
- Builds on [ADR 0010](0010-file-per-test-capture.md), [ADR 0012](0012-behavioral-signature-fingerprint.md).

## Context

The fingerprint ([ADR 0012](0012-behavioral-signature-fingerprint.md)) is the gate:
a hash that trips when behavior changes. A tripped gate raises two follow-on needs.
First, *what* changed, in content, not just which test's hash moved. Second, the
*history*: when did each behavioral state begin, and did this commit change
behavior versus the last.

The first plan answered both with a local run-history layer: archive each distinct
run-shape as a content-addressed `tar.gz`, index them in a reffile, keep the last N,
and diff the two newest. Building it surfaced that the history half is a
reimplementation of git, and a worse one. Once `fingerprint.txt` is committed, its
history *is* git's: `git log -p -- report/fingerprint.txt` is the durable, shared,
unbounded, commit-granular record of every behavioral change, each entry tied to the
code change that caused it. A run-keyed ring buffer in `target/` is ephemeral,
capped, local, and keyed on the wrong unit (runs, not commits). The two operations
that actually matter are already covered without it: "do my uncommitted changes move
behavior" is `verify` / `--check` (current run vs the committed `fingerprint.txt`),
and "did this commit change behavior" is `git diff` of `fingerprint.txt`. The ring
buffer's only unique capability, diffing two arbitrary *uncommitted* runs, is the one
nobody needs.

The *content* need survives that cut, though. `git diff` of `fingerprint.txt` shows
which test hashes moved, not the before/after content. Three ways to have the
content: commit every rendered page (readable in git, but dozens of churning files);
commit only the fingerprint (you know *that* it changed, never *what*); or commit one
compressed blob of the full corpus.

## Decision

1. **Drop the run-history ring buffer.** No reffile, no per-run archive, no
   last-N retention. The history is git; the commit is the unit of history.
2. **Commit one `baseline.tar.gz`** beside `fingerprint.txt`: the full results
   corpus as a single deterministic compressed blob. It is the content source that
   lets a project get exact diffs without committing every rendered page (and its
   churn), and without being blind to content as a fingerprint-only repo is.
3. **`--explain`** explodes the committed `baseline.tar.gz` and diffs its records
   against the fresh run, leading with `Changed` (see the triage below). That is the
   "what changed" answer; `--check` stays the tripwire; git stays the history.
4. **Read the per-test diff as additive vs regression.** The classification
   (`Added` / `Changed` / `Removed`) *is* the signal: `Added` is new coverage with
   every existing hash byte-identical (existing behavior provably untouched, the
   easy-accept rebaseline); `Changed` is an existing test's behavioral move (the
   regression surface, since the hash is behavioral and cannot drift on a refactor);
   `Removed` confirms an intentional deletion. The renderer leads with `Changed`.

## Consequences

- The framework keeps only the pieces that earn their place: deterministic tarball
  IO and the per-test diff. The reffile/archive state machine is gone.
- A project chooses its commit policy: `fingerprint.txt` alone (gate, no content
  diffs), `+ baseline.tar.gz` (gate plus compact content diffs, this ADR), or
  `+ rendered pages` (gate plus git-readable content, more churn). The baseline
  tarball is the middle, churn-free option.
- The baseline tarball is byte-deterministic (sorted entries, normalized tar
  headers, gzip mtime 0), so a no-op regenerate does not churn it in git.
- Rebaselining commits a new `fingerprint.txt` + `baseline.tar.gz`; an all-`Added`
  diff accepts freely, a `Changed` diff is reviewed against the rebaseline rules.
- The environment-provenance block (`program_hash`, `vm_version`) is a separate,
  still-deferred layer; it explains *why* a fingerprint moved and is orthogonal to
  this decision.

## Alternatives considered

- **A local run-history ring buffer (reffile + per-run `tar.gz` archive, last N).**
  Rejected: the history it tracks is already git's, at the right (commit) granularity;
  it is ephemeral, capped, and local by comparison. Its only unique capability is
  diffing two uncommitted runs, which has no use when the commit is the unit of record.
- **Commit every rendered page for content history.** Rejected for projects that
  want a clean repo: dozens of files churn (e.g. on title or ordering changes), where
  one compressed blob does not. (Projects that prefer git-readable diffs may still
  commit pages; this ADR offers the blob as the churn-free alternative, not a mandate.)
- **Commit only `fingerprint.txt`.** Kept as a valid minimal policy, but it answers
  only *that* behavior changed, never *what*; the baseline tarball is what adds the
  content answer without the page churn.

## References

- Baseline write/diff + the per-test diff:
  [`crates/testsvm/src/report/history.rs`](../../crates/testsvm/src/report/history.rs)
  (`write_baseline`, `baseline_diff`, `diff_record_maps`, `render_explain`).
- `Reporter::write_baseline` / `explain`:
  [`crates/testsvm/src/report/reporter.rs`](../../crates/testsvm/src/report/reporter.rs).
- Rebaseline rules + triage: [`../design/observation-report.md`](../design/observation-report.md)
  ("When to create a new baseline").
- The behavioral fingerprint this gates on: [ADR 0012](0012-behavioral-signature-fingerprint.md).
