# ADR 0006: The vocabulary crate is thin; an engine that can't share a lockfile is its own workspace

- Status: Accepted
- Applies the premise in [ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md).

## Context

The engines do not agree on their Solana versions. litesvm pins the solana-3.x
line; mollusk pins agave on different versions. A single Cargo lockfile cannot
hold both at once. Yet every engine must depend on the one vocabulary crate.

## Decision

`testsvm` depends only on thin Solana type crates (`solana-pubkey`,
`solana-instruction`, and friends) with loose version ranges, and on no engine.
So it sits in *any* engine's dependency graph without forcing a version on it.
An engine whose pins cannot share the main workspace's lockfile (the mollusk
adapter) lives in its own workspace with its own lockfile and depends *up* into
`testsvm`.

## Consequences

- Engines never fight over a shared lock; the vocabulary crate is the seam they
  meet at, each in its own graph.
- `testsvm-mollusk` is built and tested in its own tree, not by the main
  workspace's `cargo` invocations.
- The dependency arrows point one way: engines depend up into the vocabulary,
  never the reverse, which is what lets a forked engine ([ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md))
  slot in without disturbing the others.

## Alternatives considered

- **One workspace for all engines.** Rejected: the lockfile is unsatisfiable
  across litesvm's and mollusk's pins.
- **A fat vocabulary crate that pins Solana.** Rejected: it would force its
  version on every engine and exclude the ones that disagree.

## References

- [`../design/trait-boundaries.md`](../design/trait-boundaries.md).
