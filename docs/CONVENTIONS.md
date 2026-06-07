# anchor-litesvm test + README conventions

This is the house standard for client apps that test with `anchor-litesvm`: how
the tests emit output, and how the README presents it. It exists so every
consuming project reads the same way and so the test output is a committable,
diffable artifact rather than ephemeral console scroll.

The reference implementation is **`01-escrow`** (a two-actor, two-token,
time-gated program). When a sentence below needs a concrete example, that's the
one to open.

Scope note: this describes a convention, not an API the crate enforces. Nothing
here is checked by the compiler; it's a pattern you adopt. The pieces it leans on
(`Report`, `ActorRegistry`/`deterministic_keypair`, `create_token_mint_at`,
`print_markdown_pair`) are real and live in this crate.

## Why (the four pillars)

Good tests are **fast**, **deterministic**, **low-cost**, and **enabling**.
Determinism here means two distinct things, and both matter:

- *Reproducibility*: the same run produces the same observations, byte for byte.
- *Predictive validity*: those observations correspond to what production does.

These conventions buy reproducibility (seeded identities) and spend it on
legibility (a committable report a reviewer can read without running anything).
They do not, and cannot, buy predictive validity for what LiteSVM doesn't model
(inter-transaction ordering, the fee market, congestion); say so honestly in any
doc that presents the output.

## Test-output contract

1. **Deterministic identities.** Derive every keypair from a fixed domain +
   role, not `Keypair::new()`. Use `ActorRegistry::new("<app>/v1")` when actors
   are created in more than one place (so its duplicate-label guard does real
   work); a single-site consumer can use bare `deterministic_keypair(domain,
   role)`. Seed the *mints* too, not just the signers: PDAs and ATAs derive from
   the mint pubkeys, so a random mint churns the whole address space. Create
   mints with `TestHelpers::create_token_mint_at(authority, mint_kp, decimals)`.

2. **One `Report` per test.** Thread a `Report` through each test: `step`/`note`
   carry intent (prose), `snapshot`/`check` carry observed values. `check` is the
   assertion (expected vs actual) *and* a report line. On `Drop` it writes one
   `target/md-reports/<slug>.md`.

3. **Per-transaction trees stay too.** Keep `print_markdown_pair()` (or
   `print_logs_structured()`) on the tx chains: that documents a single
   transaction's CPI tree; the `Report` documents the whole scenario. They
   complement, they don't replace.

4. **Assemble one canonical file.** A `just test-md` recipe wipes
   `target/md-reports/`, runs the suite, and concatenates the per-scenario files
   (sorted, `LC_ALL=C`) into **`docs/testing/test-report.md`** behind a
   "GENERATED, do not edit" banner. That file is the canonical, committed,
   diffable view: because identities are seeded it is byte-stable, so a change in
   its diff is a change in behavior. Regenerate after any test change.

5. **gitignore / track.** `target/` (hence `target/md-reports/`) is gitignored;
   `docs/testing/test-report.md` is committed.

## README contract

1. **Table of contents** after the intro: a `## Contents` list linking the
   in-page sections, plus a **"Latest test run"** entry pointing at
   `docs/testing/test-report.md` (the canonical test file). It is a section in
   its own right, not a footnote.

2. **A `## Tests` section** with: a one-paragraph summary of the scenarios; the
   `just t` / `just tt` / `just test-md` triad (run / run-verbose / assemble);
   and a pointer to the report as the canonical view.

3. **Inline captures are historical.** If the README pastes any CPI trees, label
   them explicitly as illustrative snapshots and tell the reader to reason from
   the *shape*, not the literal addresses/CU (those belong to whatever run
   produced them). The committed report is the source of truth; the README prose
   and the on-chain program are not regenerated from it, so they're the only
   things allowed to drift.

## First-principles reminder

The test is the source of truth; the trace is derived and re-runnable. Never
hand-maintain a pasted trace as if it were canonical: regenerate it, or label it
as a capture and point at the report. A doc that shows output it cannot reproduce
is the stale-comment failure mode the whole structured-logging effort argues
against.
