# Test-Output Conventions

This is the house standard for projects that test with `anchor-litesvm`: how tests emit output, and how a README presents it. It exists so every consuming project reads the same way, and so test output is a committable, diffable artifact rather than ephemeral console scroll. It's the "exactly how it's spelled" companion to [Accounts as Actors](../running/accounts-as-actors.md), which is the "why."

The reference implementation is **`01-escrow`** (a two-actor, two-token, time-gated program). When a sentence below needs a concrete example, that's the one to open.

**Scope note:** this describes a convention, not an API the crate enforces. Nothing here is checked by the compiler; it's a pattern you adopt. The pieces it leans on (`Report`, the `cast_*` vocabulary, `deterministic_keypair`, `print_markdown_pair`) are real and live in the crate.

## Why: the four pillars

Good tests are **fast**, **deterministic**, **low-cost**, and **enabling**. Determinism here means two distinct things, and both matter:

- *Reproducibility*: the same run produces the same observations, byte for byte.
- *Predictive validity*: those observations correspond to what production does.

These conventions buy reproducibility (seeded identities) and spend it on legibility (a committable report a reviewer can read without running anything). They do not, and cannot, buy predictive validity for what LiteSVM doesn't model (inter-transaction ordering, the fee market, congestion); say so faithfully in any doc that presents the output.

## Test-output contract

1. **Deterministic identities.** Derive every identity from its name, not `Keypair::new()`. In a test context, `ctx.cast_actor(name)` is the one-line path: it derives the keypair from `(program_id, name)`, funds it, aliases it, and rejects a duplicate name (`cast_actor_with_sol` for an exact balance). Seed the *mints* too, not just the signers: PDAs and ATAs derive from the mint pubkeys, so a random mint churns the whole address space. `ctx.cast_mint(name, &authority, decimals)` casts a mint the same way. (Outside a context, the underlying derivation is `deterministic_keypair(domain, role)`, with `ActorRegistry::new("<app>/v1")` adding a duplicate-role guard when actors are created in more than one place. See [PDAs & Token Helpers](../instructions/pdas-and-tokens.md).)

2. **One `Report` per test.** Thread a `Report` through each test: `step` / `note` carry intent (prose), `snapshot` / `check` carry observed values. `check` is the assertion (expected vs actual) *and* a report line. On `Drop` it writes one `target/md-reports/<slug>.md`.

3. **`check` for confirmations, `transition` for state changes.** A `- [x]` checklist reads as "all good", which works against a report documenting a transition, and actively misleads in one documenting a *violated* invariant (a green list of confirmed violations reads like a passing feature). `transition(label, before, expected_after, actual_after, meaning)` renders a neutral before/after/what-it-means table row and still asserts (soft, like `check`), so presentation and enforcement never split. Consecutive calls collapse into one table.

4. **Per-transaction trees stay too.** Keep `print_markdown_pair()` (or `print_logs_structured()`) on the tx chains: that documents a single transaction's CPI tree; the `Report` documents the whole scenario. They complement, they don't replace. (The [rendering views](../inspect/cpi-tree.md) are what these calls emit.)

5. **Assemble one canonical file.** A `just test-md` recipe wipes `target/md-reports/`, runs the suite, and concatenates the per-scenario files (sorted, `LC_ALL=C`) into **`docs/testing/test-report.md`** behind a "GENERATED, do not edit" banner. That file is the canonical, committed, diffable view: because identities are seeded it is byte-stable, so a change in its diff is a change in behavior. Regenerate after any test change.

6. **gitignore / track.** `target/` (hence `target/md-reports/`) is gitignored; `docs/testing/test-report.md` is committed.

## README contract

1. **Table of contents** after the intro: a `## Contents` list linking the in-page sections, plus a **"Latest test run"** entry pointing at `docs/testing/test-report.md` (the canonical test file). It is a section in its own right, not a footnote.

2. **A `## Tests` section** with: a one-paragraph summary of the scenarios; the `just t` / `just tt` / `just test-md` triad (run / run-verbose / assemble); and a pointer to the report as the canonical view.

3. **Inline captures are historical.** If the README pastes any CPI trees, label them explicitly as illustrative snapshots and tell the reader to reason from the *shape*, not the literal addresses/CU (those belong to whatever run produced them). The committed report is the source of truth; the README prose and the on-chain program are not regenerated from it, so they're the only things allowed to drift.

## First-principles reminder

The test is the source of truth; the trace is derived and re-runnable. Regenerate a trace, or label it as a capture and point at the report, rather than hand-maintaining a pasted one as if it were canonical. A doc that shows output it cannot reproduce is the stale-comment failure mode the whole structured-logging effort argues against. (It's the same principle that keeps this book's rendered samples correctly labeled as per-run captures; see [Reading Compute & Fees](../inspect/compute-fees.md).)
