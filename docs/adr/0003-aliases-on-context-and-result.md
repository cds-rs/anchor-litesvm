# ADR-0003: aliases live on `AnchorContext` and on `TransactionResult`

- **Status:** Accepted
- **Date:** 2026-05-25
- **Source:** convergent dogfood evidence from `~/sol/02-amm` and `~/sol/voting`; capstone LOI framing
- **Supersedes:** [ADR-0002](0002-send-helpers-accept-aliases.md) (partial; see "What survives, what changes")

## Context

[ADR-0002](0002-send-helpers-accept-aliases.md) settled "where does `&Aliases` go" by adding it to every `send_*` helper and to `TransactionResult::print_logs_structured`. The decision explicitly rejected a "context-bound aliases" alternative on the grounds that hidden state would force test authors to remember reset semantics between scenarios.

Two reference test suites later, the evidence has come in (see `project_dogfood_test_patterns` for the longer write-up):

- The AMM (`~/sol/02-amm`) and voting (`~/sol/voting`) suites both built a `Scenario` (resp. `Bootstrap`) type that *owns* an `Aliases` table and threads it from `self.aliases` into every `send_*` / `print_logs_structured` call internally. The "hidden state" the original ADR worried about did materialize, but it lives one layer above the framework: in the per-suite scenario type, not in the framework itself.
- Both suites independently arrived at an identical workaround for `Aliases::with`'s consuming-builder signature: `std::mem::take(&mut self.aliases); self.aliases = taken.with(...)`. When two callers reach for the same dance, it's a sign the API didn't fit.
- The capstone LOI names the Vault / Escrow / CPAMM user guide as the deliverable's docs half. The convergent `Scenario`-owns-aliases pattern will be the canonical shape in that guide. Friction that survives into the guide gets paid by every newcomer who reads it.

The original ADR-0002 wasn't wrong on its own terms: an external `&Aliases` per call *is* the most predictable surface in isolation. What changed is the surrounding context: with two independent suites converging on the same wrapper-type pattern, the framework absorbing that pattern stops being "hidden state" and starts being "the shape these tests actually want."

## Considered options

Three concrete questions; one decision per axis.

### Where does the absorbed alias state live?

| Option | What it owns | Coverage |
|---|---|---|
| **A. `AnchorContext` owns `Aliases`** | A field on the existing Anchor wrapper. Bare LiteSVM `TransactionHelpers` continues to take `&Aliases` as before. | Anchor users; non-Anchor users keep the old path. |
| B. New `Harness` wrapper in `litesvm-utils` | A composing wrapper at the lower layer; `AnchorContext` would compose it. | Both Anchor and non-Anchor users at the cost of a new type to learn. |
| C. Store aliases only on `TransactionResult` | Each `send_*` stashes the table on the returned result; no other state. | Helps the trailing `print_logs_structured` call but not the `send_*` calls themselves. |

### Should `AnchorContext::send_ok` also call `print_logs_structured` automatically?

| Option | Convention | Cost |
|---|---|---|
| **A. No, leave printing to the caller** | Matches voting's convention (print on happy, quiet on negative). Caller writes `ctx.send_ok(...).print_logs_structured()` if they want output. | Caller still chains a method; no behavior change for existing callers. |
| B. Yes, always print on success | Matches AMM's convention. Removes the trailing call entirely. | Behavior change: every existing `send_ok` caller starts seeing structured output without asking for it. |
| C. Add `send_ok_print` variant | Caller picks per call. | More API surface; explicit at every call site. |

### How should `TransactionResult::print_logs_structured` handle the alias parameter going forward?

| Option | Signature | Migration |
|---|---|---|
| A. Leave as-is, takes `&Aliases` | `print_logs_structured(self, &Aliases) -> Self` | No changes; alias parameter still threaded once at the print site. |
| B. Add an aliases-free overload via storage | Both versions coexist; the new no-arg one reads from `Option<Aliases>` storage. | Two methods with related names; the new one is the ergonomic path. |
| **C. Replace with storage-based version** | `print_logs_structured(self) -> Self`; aliases come from `self.aliases` or `Aliases::default()`. | Breaking change to current callers; cleanest end state. |

## Decision

**Where: A** (`AnchorContext` owns `Aliases`).  
**Auto-print: A** (no, leave printing to the caller).  
**Print API: C** (replace `print_logs_structured` with the storage-based no-arg version).

### What survives, what changes

ADR-0002 isn't entirely overturned; only its `TransactionResult` and `AnchorContext` aspects are.

**Survives from ADR-0002.** The bare `TransactionHelpers` trait on `LiteSVM` still takes `&Aliases` per call:

```rust,ignore
fn send_ok(&mut self, ix: Instruction, signers: &[&Keypair], aliases: &Aliases)
    -> TransactionResult;
fn send_err_named(&mut self, ix: Instruction, signers: &[&Keypair],
                  aliases: &Aliases, error_name: &str) -> TransactionResult;
```

These helpers are the framework's only API surface for non-Anchor users (LiteSVM directly). Removing the parameter would force those callers to either go through `TransactionResult::with_aliases` after every send or to live with `Aliases::default()` on the failure path. Neither is an improvement.

**Changes from ADR-0002.**

1. `TransactionResult` gains a private `aliases: Option<Aliases>` field, a public `with_aliases(self, Aliases) -> Self` setter, and `print_logs_structured(self) -> Self` (no arg) plus `logs_structured_string(&self) -> String` (no arg) that read from storage or fall back to `Aliases::default()`.

2. The trait `send_*` methods stash the `&Aliases` they take onto the returned result via `with_aliases(aliases.clone())`. Cheap clone (two small HashMaps). Callers chaining `.print_logs_structured()` no longer pass aliases.

3. `AnchorContext` gains an `aliases: Aliases` field (default `Aliases::default()`), an `alias(&mut self, pk, label) -> &mut Self` extender, and `send_ok(ix, signers)` / `send_err(ix, signers)` / `send_err_named(ix, signers, error_name)` methods that read `&self.aliases` and forward to the bare trait. No `&Aliases` parameter at the call site.

4. `Aliases::add(&mut self, pk, name) -> &mut Self` (additive). `Aliases::with` keeps its consuming-builder signature for the seed-on-construction case; `add` is the accumulation companion that both dogfood suites reinvented around `std::mem::take`.

### Rebutting the original "hidden state" concern

ADR-0002 rejected context-bound aliases on the grounds that "tests that build different alias maps per scenario would have to remember to reset between calls." Three responses:

- **The convergent pattern is one alias table per scenario.** Neither dogfood suite reaches for "different alias maps within one scenario." Authority rotation, role overloads, and per-actor naming all flow through `self.aliases.add(...)` with later-wins semantics. The framework matching that pattern is alignment, not surprise.
- **`AnchorContext` already owns the SVM lifecycle.** Adding an `Aliases` field to a type that already owns the SVM, the program ID, and the payer is a small extension. A reader who finds `ctx.aliases` mid-test sees it the same way they see `ctx.svm.warp_to_timestamp` or `ctx.airdrop`.
- **The escape hatch survives.** Any test that genuinely needs a different alias table per call can build one externally and use `result.with_aliases(local_table).print_logs_structured()`, or drop down to `ctx.svm.send_ok(ix, signers, &local_table)` for the bare-LiteSVM path. The context-owned default is the common case, not the only case.

## Outcome

Shipped on the `class/ask` branch. Aliases::add and the storage-based print methods land in `crates/litesvm-utils/src/transaction.rs`, the context wrappers in `crates/anchor-litesvm/src/context.rs`. Tests in `crates/litesvm-utils/src/transaction/tests.rs` and `crates/anchor-litesvm/src/lib.rs` cover the alias-flow paths end to end. The previous worked examples in `EVALUATING.md` and `crates/litesvm-utils/README.md` were updated to feature the context-owned path as the lead.

## Consequences

- Anchor-focused tests collapse `ctx.svm.send_ok(ix, &[&s], &aliases).print_logs_structured(&aliases)` to `ctx.send_ok(ix, &[&s]).print_logs_structured()`. The two `&aliases` references become zero at the test surface and one (`ctx.alias(pk, "name")` in setup) at the registration site.
- Bare LiteSVM users are unchanged on the `send_*` side; their `print_logs_structured` calls do change shape, but the trait stashes aliases automatically, so the no-arg form reads through.
- The `Aliases::add` method gives both dogfood suites a path to delete their `std::mem::take` workaround.
- Scenario types that want per-scenario alias tables (the AMM / voting pattern) now have a choice: keep the table on their own scenario struct (as they do today) or move it onto `ctx.aliases`. Both work; the framework absorbs the friction either way.
- Future widening: if a need for "per-call alias override on `AnchorContext`" surfaces, it can land as a `send_ok_with(ix, signers, &Aliases)` companion without disturbing the default-path API. Not implemented; YAGNI.

## Implementation pointers

- `crates/litesvm-utils/src/transaction/aliases.rs`: `Aliases::add` plus tests.
- `crates/litesvm-utils/src/transaction.rs`: `TransactionResult::with_aliases`, `print_logs_structured()` (no arg), `logs_structured_string()` (no arg), `send_ok` / `send_err` / `send_err_named` stash via `with_aliases(aliases.clone())`.
- `crates/anchor-litesvm/src/context.rs`: `aliases` field, `alias()` extender, `send_ok` / `send_err` / `send_err_named` reading `&self.aliases`.
- Tests for the storage and AnchorContext flow paths: `crates/litesvm-utils/src/transaction/tests.rs`, `crates/anchor-litesvm/src/lib.rs::integration_tests`.
