# ADR 0002: One engine port as a trait (`TestSVM`), not an enum of engines

- Status: Accepted
- Applies the premise in [ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md).

## Context

Tests must run against more than one execution engine: an in-memory litesvm for
speed, an instruction-level mollusk harness, and a live or forked cluster over
JSON-RPC for fidelity. A team should not have to choose at authoring time and
rewrite to switch.

## Decision

A single trait, `TestSVM`, is the port every engine implements, and a test names
the trait, never a concrete engine. The required core is the irreducibly
engine-specific part (`send` returning the neutral record, the account and clock
levers, `capabilities`, `aliases`); everything else is a default method over it.

The port began on the surfpool branches as `ExecutionBackend` with a single
in-memory backend; it was renamed `TestSVM` (and the record `model::Transaction`)
once a third engine made the generality concrete and "execution backend"
undersold what the seam had become.

## Consequences

- Choosing an engine is a one-line construction swap; the vocabulary, the model,
  and every renderer behave identically above it.
- A conformance scenario runs through every engine and asserts the same shape,
  so the trait is a checked contract, not a hopeful one.
- A new engine is the required-core methods plus whatever sockets it opts into;
  it inherits `actor`, `prop`, and the naming workflow as defaults.

## Alternatives considered

- **An `enum Engine { Lite, Rpc, Mollusk }` with match arms.** Rejected: every
  new engine edits every match site, and engines whose dependency graphs cannot
  coexist (litesvm's solana pins vs mollusk's) cannot live in one enum's crate
  anyway (see [ADR 0006](0006-thin-vocabulary-crate-engines-own-their-lockfile.md)).

## References

- [`../design/trait-boundaries.md`](../design/trait-boundaries.md),
  [`../design/endpoint-agnostic-architecture.md`](../design/endpoint-agnostic-architecture.md).
