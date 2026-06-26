# ADR 0001: The executor owns the execution vocabulary; consumers name and render it

- Status: Accepted
- Applies the premise in [ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md).

## Context

A transaction's facts are witnessed only by the layer that runs it. The CPI
nesting, the per-frame account privileges, the compute consumed: these are the
executor's first-hand knowledge. A consumer that re-derives a fact from a weaker
source gets a partial or wrong answer. The clearest case: an `invoke_signed` PDA
presents as a signer one frame down, where the transaction message header,
which a consumer would reconstruct from, cannot see it.

## Decision

Facts are defined and produced at the executor (litesvm); the layers above name,
link, color, and diagram them, and never re-derive them. litesvm owns the
`cpi_tree` parse (the logs are its artifact) and the per-frame privilege trace
(via its inspect hook); `litesvm-utils` and `anchor-litesvm` consume those facts
and add aliases, links, and diagrams.

## Consequences

- A fact captured once at the executor serves every consumer at once: the same
  frames render identically under surfpool's output and anchor-litesvm's tree.
- Where the executor cannot yet expose a fact, the consumer reconstructs it as
  an *acknowledged* stopgap, and the standing direction is to push it down to
  the source rather than entrench the reconstruction (see
  [`../design/litesvm-boundary.md`](../design/litesvm-boundary.md)).
- This is the reason for the `cds-rs/litesvm` fork: `cpi_tree` and the
  `invocation-inspect-callback` are facts landed at the executor so every
  consumer inherits them ([ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md)).

## Alternatives considered

- **Re-derive each fact in each consumer.** Rejected: N consumers means N
  partial reimplementations that drift, and the weaker source cannot recover
  what only the executor saw.

## References

- [`../design/litesvm-boundary.md`](../design/litesvm-boundary.md),
  [`../design/endpoint-agnostic-architecture.md`](../design/endpoint-agnostic-architecture.md).
