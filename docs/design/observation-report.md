# Observation report and behavioral fingerprint: design

## Scope

This doc covers how a test suite turns its executions into two committable
artifacts: a per-scenario **report** (a sectioned index linking to per-scenario
pages) and a **fingerprint** (a Merkle hash that acts as a regression gate). It
explains how the pieces fit; the *why* of each shaped decision lives in the ADRs
it points to ([0010](../adr/0010-file-per-test-capture.md),
[0011](../adr/0011-normalization-in-the-reporter.md),
[0012](../adr/0012-behavioral-signature-fingerprint.md)).

In scope: the capture/normalize/encode/fold/aggregate pipeline, the data shapes
that flow through it, how a consumer crate drives it, and the committed
`baseline.tar.gz` + `--explain` content diff. Out of scope (designed, not yet
built): the `(program.so, vm_version)` environment-provenance block. A run-history
ring buffer was considered and **dropped** as redundant with git, which already
holds the commit-granular fingerprint history; the surviving piece is the single
committed baseline tarball (see [ADR 0013](../adr/0013-baseline-tarball-history-is-git.md)).

## The shape of the problem

A test's value is not only pass/fail; each execution carries a structured story
(the CPI tree, who signed, what each frame cost). We want to commit that story as
a browsable report, and we want a small, stable signal that trips when behavior
changes and stays quiet otherwise. The constraints that shape the pipeline:

- The report must be a byproduct of the *real* suite, not a parallel set of
  render functions that can drift from it.
- Capture must survive how tests actually run, including `cargo nextest`
  (process-per-test), so it cannot hold state in memory across tests.
- The fingerprint must be sensitive to behavior and *blind* to non-behavioral
  churn (a moved line, a renamed title), or people learn to ignore it.

## The pipeline

```
  test run (any harness)
        |  each test: testsvm::report::record(Observation { .. })
        v
  target/test-results/<slug>.json        lossless capture (gitignored)
        |
        v
  Reporter (a workspace-local pass over the corpus)
        |--- normalize : ReportRecord -> NormalRecord   (roles, CU kept, location kept-for-render)
        |--- encode    : behavioral view -> canonical JSON -> sha256
        |--- fold      : per-record -> per-group -> suite Merkle root
        |--- render    : sectioned index.md (+ per-scenario pages)
        |--- verify    : diff a committed fingerprint vs the fresh one
        v
  committed: report/index.md, report/fingerprint.txt
```

### Capture (`observation.rs`)

Each test calls `record(Observation { group, title, test_name, test_file,
manifest_dir, expect, tx })` as it runs. That writes one JSON `ReportRecord` to
`target/test-results/`. The record is *lossless and self-describing*: it carries
the execution facts (the frame tree, with per-frame CU) and the alias table in
effect, so the later Reporter can map addresses to roles with no live engine. It
is `schema_version`-stamped.

Capture is per-test and process-local; nothing accumulates in memory. That is
what lets it survive split binaries and nextest (see
[ADR 0010](../adr/0010-file-per-test-capture.md)). The `Expect` (`Succeeds` /
`Rejects`) is the *declared intent*; combined with the transaction's actual
outcome it yields a *verdict* (`passed` / `failed`) so a correct rejection reads
as passed, never a misleading `false`.

### Normalize (`normalize.rs`)

The Reporter, not the test, normalizes; the raw corpus stays a *cache* so a rule
change re-runs only the Reporter (see
[ADR 0011](../adr/0011-normalization-in-the-reporter.md)). The default transform
maps each frame's program id to its role label (using the captured alias table),
projects the verdict and summary, and keeps the frame tree with CU. Program-
specific shaping is a free function registered with the Reporter, keyed by
group/test; it is not a trait, because every test emits the same `ReportRecord`
type, so dispatch keys on data, not on type.

`NormalRecord` keeps location (`anchor`, `title`, `test_file`) for rendering, but
exposes `behavioral()` -> `{ verdict, summary, frames }`, the projection the
fingerprint hashes.

### Encode and fold (`canonical.rs`, `fingerprint.rs`)

The behavioral view is encoded as JCS canonical JSON: object keys sorted
recursively, array order preserved (frame order is behavior), no lowercasing
(case is signal), integers exact. `sha256` of that is the per-record fingerprint.
`merkle` folds per-record hashes into a per-group hash (members ordered by
`test_name`, so code movement never churns the fold) and the group hashes into a
suite root. The `Manifest` is the committed `fingerprint.txt`: a `group / test /
hash` line per record, the group hashes, and the root.

Why the hash is *behavioral* (CU in, location out) is the keystone decision; see
[ADR 0012](../adr/0012-behavioral-signature-fingerprint.md).

### Aggregate, render, verify (`reporter.rs`)

`Reporter::from_dir` loads the corpus (rejecting a foreign `schema_version`).
`group_order` sets section order; members display in source (anchor) order even
though they fold by `test_name`. `write` emits `index.md` (a section per group:
Scenario, Verdict ✅/❌, Summary, and a Source link back to the test) and
`fingerprint.txt`. `verify(committed)` diffs the committed manifest against the
fresh one and `render_changes` reports each changed test by group and name, with
the captured observation JSON embedded so a developer sees what the run produced.

## Driving it from a consumer

A consumer crate adds two things: a `record(..)` call in each scenario (carrying
its `Expect`), and a small `report` binary that runs the Reporter after the
suite. The shape:

```
cargo test            # the suite runs; each test writes its JSON record
cargo run --bin report          # the Reporter writes index.md + fingerprint.txt
cargo run --bin report -- --check   # CI gate: diff vs committed, exit 1 on change
```

The mpl-core dogfood does exactly this across four groups (Core, Account
ownership, Agent identity, Execution delegate), committing a sectioned `index.md`
(57 scenarios, all green) and a byte-reproducible `fingerprint.txt`.

## The fingerprint as a gate

The committed `fingerprint.txt` is the regression anchor. Because the hash is
behavioral, a refactor that moves lines or renames a test leaves it unchanged
(only the index's Source links update, and those are reviewed, not gated). A CU
shift *does* move it: CU is deterministic in this pinned environment (committed
`.so`, `Cargo.lock`-pinned VM, seed-derived keys), so a shift is real signal, and
the committed fingerprint is valid for that `(program.so, VM version)` pair.
Bump either and you rebaseline, informed by the `verify` diff. The big-number
edge (a `u64` CU never collapsing through a float) is proptest-proven, not
asserted.

### When to create a new baseline

A *baseline* is the committed `fingerprint.txt` (and, under v1.1, an optional
committed baseline tarball). The gate trips when a run's behavioral root differs
from the committed baseline. *Rebaselining* is accepting the current state as the
new reference and committing it. The governing principle:

> **Rebaseline only a change you can explain and have accepted. Never rebaseline to
> silence a change you cannot explain.**

**First, read the per-test diff, not the root.** The suite root moves the instant
anything changes, including adding a scenario, so the root alone says nothing. The
per-test classification (`Added` / `Changed` / `Removed`, from `Manifest::diff` or the
baseline `--explain`) is the diagnosis, and it splits the move into additive vs
regression:

- **`Added`** (new manifest lines, every existing test's hash byte-identical): you
  added coverage and existing behavior is *provably* untouched. A pure-additions diff
  is the easy-accept case.
- **`Changed`** (an existing test's hash moved): because the hash is the behavioral
  signature, this is a *real* behavioral move, never refactor noise. This is the
  regression surface; it is what you actually review against the rules below.
- **`Removed`** (a line disappeared): usually an intentional deletion, occasionally a
  test that silently stopped running. Confirm it was intentional.

So a diff that is all `Added` rebaselines freely; the moment there is a `Changed`
entry, the rules turn on which kind of move it is:

**Rebaseline (after reading the diff):**

1. **Intentional behavior change.** You changed the program or a test on purpose,
   the diff shows exactly the change you meant, and the new behavior is correct.
   The common case. Commit the new fingerprint; name the change in the message.
2. **Behavior-preserving rebuild.** The program was rebuilt and the diff is
   CU-only: frames, instructions, and outcomes are identical, just the numbers
   moved. The baseline is valid per `(program.so, VM)` pair, so a rebuild needs a
   fresh one. (v1.1b makes this mechanical: `program_hash` moved and nothing in
   the behavioral tree did.)
3. **Runtime or toolchain bump.** The VM or a pinned dep changed; the diff is
   CU/encoding-only with behavior identical. (v1.1b: `vm_version` moved.)

**Do not rebaseline; investigate:**

4. **Unexplained change.** The behavioral root moved but you intended no change
   (and, under v1.1b, `program_hash` and `vm_version` are both unchanged).
   Something altered behavior under identical conditions: a regression, a framework
   bug, or a determinism leak. Rebaselining here commits the bug as the new normal.
5. **Non-determinism.** The fingerprint changes run-to-run with no code, program,
   or VM change at all. Fix the cause first (an unseeded key, a clock read, an
   unsorted collection, a `Pubkey::new_unique()` call-order dependence); a
   non-deterministic baseline makes the gate worthless. Baseline once it is stable.
6. **You cannot tell.** If the diff does not map to a change you made, treat it as
   case 4, not case 1. "I'll just accept it" is how a gate rots.

**Mechanics.** Rebaselining is a deliberate, reviewed commit, never automatic.
Regenerate (`cargo run --bin report`), read the `--check` diff (which tests changed)
and, when you want the exact content, `--explain` (explode the committed
`baseline.tar.gz` and diff its records against the fresh run, leading with `Changed`).
Confirm the change matches a "rebaseline" case above, then commit the new
`fingerprint.txt` and `baseline.tar.gz` with a message naming *why* (intentional
change / rebuild / VM bump), so the baseline's history stays auditable in git.
`--check` must never write a new baseline on mismatch: auto-accept is equivalent to
deleting the gate.

The committed `baseline.tar.gz` is the one compressed blob that holds the full
results corpus, so a project gets exact content diffs without committing every
rendered page (and without the page churn). The behavioral *history* itself lives in
git: `git log -p -- report/fingerprint.txt` is the durable, commit-granular record of
every behavioral change, and `report --against <git-ref>` parses an older committed
`fingerprint.txt` and diffs it against the current run. There is no separate run-level
history store; the commit is the unit of history, and git already keeps it.
