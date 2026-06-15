# ADR 0000: The premise — trait boundaries, composability, and ecosystem reach

- Status: Accepted
- The premise the other ADRs apply. Read this first.

## Context

Solana's testing surface is fragmented. litesvm runs a program in-memory,
mollusk runs it at the instruction level against hand-built accounts, and a
surfnet (or any cluster) runs it over JSON-RPC. Each is its own API, so a test
written against one does not run against another, and a team picks an engine
early and is married to it.

Worse, the facts a test most wants are produced deep in the executor and are
hard to see from outside it: the CPI invocation structure, the per-frame
account privileges (the `invoke_signed` PDA that signs one frame down), the
decoded events. A tool that wants them today re-derives them from a weaker
source (the message header, the flat logs), partially and divergently, each tool
reimplementing the same partial view.

## Decision

Define the framework's boundaries with **traits**, so composition is by
interface, not by a concrete engine. There is one vocabulary, a port
(`TestSVM`) and an engine-neutral record (`model::Transaction`), that any
execution engine implements. "Support multiple engines" becomes a property of
the architecture rather than a rewrite per engine: a test names the trait, and
the engine is chosen at construction.

A trait boundary also makes **service visibility legible across the ecosystem**.
A capability either crosses the trait, in which case every consumer above
inherits it, or it does not, in which case the trait says so out loud (see
[ADR 0007](0007-declare-the-trace-asymmetry-via-capabilities.md) on capability
flags). So "what facts are visible where" is something you read off the
boundary, quickly, instead of discovering it by failure deep in a tool.

## Consequences

- **Land facts at the executor.** The widest-reach facts belong at the layer
  with first-hand knowledge of them (litesvm), so we land them there and every
  downstream consumer inherits them at once (see
  [ADR 0001](0001-executor-owns-the-execution-vocabulary.md)). A fact captured
  once at the executor serves every tool; the same fact reconstructed in each
  tool serves only that one.

- **Fork and adapt ecosystem repos to iterate without interrupting
  maintainers.** Proving a boundary often means a fact the executor does not yet
  expose. Rather than block on upstreaming it first (which stalls iteration) or
  wait on a maintainer's time, we fork and adapt the ecosystem repo and land the
  boundary there: the `cds-rs/litesvm` fork carries the `cpi_tree` parser and
  the `invocation-inspect-callback` hook; surfpool's render is wired the same
  way. The fork is a *working proposal*, not a divergence: it demonstrates the
  boundary in running code, which is a more persuasive thing to offer back than
  a description. The trait boundaries are what keep this safe: a forked engine
  slots in behind the same port every other engine satisfies, nothing downstream
  reaches around it, and the upstream's own API is untouched while we iterate.
  The boundaries that have graduated from our fork and been offered back are
  tracked in the [upstreaming ledger](../UPSTREAMING.md).

- **Hold the boundary discipline we ask for.** When we offer a boundary
  upstream, we ask the maintainer to expose a *fact at its source*, not to adopt
  another tool's surface API. We hold the same line in our own design: the
  vocabulary defines the types that cross boundaries; consumers name and render
  them. We do not reach around a trait to a concrete engine, and we do not ask a
  source to import our ergonomics.

## Alternatives considered

- **Build on one engine's concrete API.** Rejected: it marries every test to
  that engine, forecloses cross-engine discovery, and makes a second engine a
  rewrite rather than an adapter.

- **Block on upstreaming every boundary before using it.** Rejected: it stalls
  iteration on a maintainer's schedule, and a boundary already running in a fork
  is a stronger, more respectful contribution than a proposal in the abstract.

## References

- The boundary principle: [`../design/litesvm-boundary.md`](../design/litesvm-boundary.md).
- The architecture this premise produces: [`../design/trait-boundaries.md`](../design/trait-boundaries.md).
- The surfpool-era origin (pre-rename names): [`../design/endpoint-agnostic-architecture.md`](../design/endpoint-agnostic-architecture.md).
- ADRs [0001](0001-executor-owns-the-execution-vocabulary.md) through
  [0008](0008-key-self-cpi-events-by-program.md) are applications of this premise.
