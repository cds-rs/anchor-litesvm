# ADR 0009: Split capability flags into observability and operational; require observability parity

- Status: Accepted
- Refines [ADR 0007](0007-declare-the-trace-asymmetry-via-capabilities.md).
- Applies the premise in [ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md).

## Context

ADR 0007 modeled the per-frame trace as `Option<InstructionTrace>` plus a
`per_frame_trace` capability, and said a consumer "annotates or skips the
degraded case rather than emitting a half-empty authority graph." That framing
treated every flag in `Capabilities` the same way: a `false` meant "this engine
is less capable, so render around it."

Running one probe spec through litesvm, mollusk, and quasar showed the framing
conflates two different things. The formerly least-capable backend (mollusk)
reached byte-identical output with the others, but only by *convention*: each
adapter happened to feed `model::Transaction::assemble(...)` the right shape
because we were careful, not because anything checked it. Pulling on that thread,
the `Capabilities` column turns out to carry two different animals:

- **Observability flags** (`per_frame_trace`, `structured_cpi`): what the engine
  can *show* about an execution. In this lineup the executor computes these facts
  by construction: litesvm's in-process inspect hook, Agave's
  `solana-transaction-context` (which mollusk and quasar both run), and surfpool
  (which embeds litesvm) all hold the per-frame trace during execution. A `false`
  here is never "this engine is genuinely less capable"; it is *unimplemented
  surfacing* of data that is reachable at a boundary you (or a collaborator)
  control. This is the reach thesis ([ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md))
  as a theorem.
- **Operational flags** (`fork`, `fees`, `atomic_send`, `instant_reset`): what
  the engine *does*. Genuine character. An in-memory engine cannot fork a live
  cluster (there is no cluster to reach, and it is not manufacturable); mollusk
  genuinely does not deduct fees. These are not TODOs.

The audit also surfaced one real divergence, kept as-is: mollusk declares
`atomic_send: false` and routes a multi-ix send through
`process_instruction_chain`, though it *has* an atomic, shared-budget path
(`process_transaction_instructions`). A capability a backend *declines* is
legitimate; the contract governs the data a backend *claims*, not which APIs it
chooses to call.

## Decision

1. **Classify every `Capabilities` flag** as observability or operational.
2. **Observability flags must reach full cross-engine render parity.** A `false`
   observability flag is a tracked TODO, not an accepted degraded output.
3. **Operational flags gate which scenarios an engine can run, never how its
   output renders.** Do not run a fork scenario on a non-fork engine; that says
   nothing about the engine's renders.
4. **The degraded renderer retires for observability.** There is ONE total
   renderer, a pure function of the record; sparse data simply renders sparse, it
   is not a mode a consumer selects. `fork` is the sole genuine unreachable:
   reach you cannot manufacture, and it gates scenarios rather than producing a
   degraded render.

The decision is enforced by a conformance harness rather than by review. One
shared probe spec runs per backend: capture the `model::Transaction` record,
validate it against invariants (`validate_observability` returns a typed
`Vec<ObservabilityViolation>`; e.g. `per_frame_trace => trace` populated,
`structured_cpi =>` frames sourced from the trace, frame/trace shape agreement,
1-based `stack_height`, every traced account resolves to a pubkey), render each
artifact, and assert byte-identical output across the in-memory engines
(`assert_golden`). "Audit mollusk" becomes "mollusk passes the spec," in CI.

Parity requires a normalized environment, so observation is not confused with
configuration: `EnvironmentConfig` pins the compute-unit ceiling equal across
engines before the first send (litesvm's solana graph defaults to the old
200,000 per-instruction limit and Agave 4.0 reports the 1,400,000
per-transaction max; that lone difference renders as the `M` in `consumed N of
M` and would break parity). The ceiling is declared once, in
`EnvironmentConfig::default()`, applied through `TestSVM::configure`.

## Consequences

- The render path stops branching on which backend produced the record. A
  renderer that must ask "which engine is this?" is the smell the split removes.
- This refines, it does not contradict, ADR 0007: the asymmetry stays *declared*
  through capabilities, but "annotate or skip the degraded case" now applies only
  to *operational* reach (a fork the endpoint never witnessed), not to
  observability. An observability `false` is a failing conformance TODO, not a
  render-around.
- New observability surfaces (events, account deltas, mermaid sequences, compute
  and fees) join as rows in the same harness: a structured input the backend
  provides, a renderer testsvm owns, a parity assertion the harness runs.
- surfpool's observability flags are open TODOs deferred to the maintainer
  conversation (it embeds litesvm, so the data is already in hand; surfacing it
  is wiring, not a capability it lacks), not accepted degraded gaps.

## Alternatives considered

- **Keep one flag type; render around every `false`.** Rejected: it hides
  unimplemented surfacing behind "this engine is less capable," which is the same
  silent-partial failure ADR 0007 set out to avoid, now applied to data that is
  actually reachable. The "degraded render" reads as complete and misleads.
- **Capability maximalism (require every flag `true`).** Rejected: operational
  flags are genuine engine character. mollusk legitimately declines `fees`; an
  in-memory engine cannot `fork`. Forcing them true would either exclude real
  backends or fabricate facts.

## References

- Conformance harness: [`crates/testsvm/src/conformance.rs`](../../crates/testsvm/src/conformance.rs)
  (`validate_observability`, `ObservabilityViolation`, `assert_golden`); the
  cross-engine probe under `md-babelfish/listings/probe`.
- `EnvironmentConfig` / `TestSVM::configure`:
  [`crates/testsvm/src/lib.rs`](../../crates/testsvm/src/lib.rs).
- [ADR 0007](0007-declare-the-trace-asymmetry-via-capabilities.md);
  [`../design/litesvm-boundary.md`](../design/litesvm-boundary.md).
