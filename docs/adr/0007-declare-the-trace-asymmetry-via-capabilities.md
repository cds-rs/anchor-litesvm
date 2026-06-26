# ADR 0007: Declare the per-frame-trace asymmetry through capabilities; don't hide it

- Status: Accepted
- Applies the premise in [ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md).

## Context

The per-frame privilege trace (which accounts each frame saw as signer or
writable, including `invoke_signed` PDAs) rides litesvm's in-process inspect
hook. A stock RPC endpoint never witnessed it, so an `RpcBackend` cannot
produce it. But the authority and ownership graphs are built from it. The
engines are genuinely asymmetric here, and the question is whether to hide that
or surface it.

## Decision

Model the trace as `Option<InstructionTrace>` on the record (`Some` in-memory,
`None` over a stock RPC), and expose a `Capabilities` flag (`per_frame_trace`)
on the port. A consumer reads the flag and annotates or skips the degraded case
rather than emitting a half-empty authority graph as if it were complete.

## Consequences

- The asymmetry is explicit: a report can caveat "no per-frame trace
  on this endpoint" instead of silently drawing a partial graph.
- This is the ecosystem-visibility principle made concrete: a capability that
  crosses the trait is inherited; one that doesn't is *declared* at the trait,
  so a consumer discovers it by reading the boundary, not by a confusing render.
- A future endpoint that surfaces the trace (surfpool exposing the record) flips
  the flag to `Some` with no consumer change.

## Alternatives considered

- **Always emit the graph.** Rejected: over an RPC it would be silently partial,
  which reads as complete and misleads.
- **Require the trace on every engine.** Rejected: a stock RPC cannot provide
  it; the requirement would exclude a real, useful backend.

## References

- [`../design/litesvm-boundary.md`](../design/litesvm-boundary.md),
  [`../design/endpoint-agnostic-architecture.md`](../design/endpoint-agnostic-architecture.md).
