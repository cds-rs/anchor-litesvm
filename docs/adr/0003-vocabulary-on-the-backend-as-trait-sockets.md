# ADR 0003: The test vocabulary lives on the backend, registered through trait sockets

- Status: Accepted
- Applies the premise in [ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md).

## Context

A test attaches names and decoders so rendered output reads in domain terms:
`pubkey -> name` aliases, instruction `(disc -> name)`, error `(code -> name)`,
and event decoders. That vocabulary could live in one of two places: on each
*result* (the test decorates every returned record) or on the *backend* (the
test registers once and every send carries it).

## Decision

Each facet is a `register_*` socket on `TestSVM`, with a default no-op body. An
engine that models the facet stores it in a table it holds; `send` stamps every
table onto the `model::Transaction` it returns. The rule: register on the
backend once, the record carries the vocabulary, every renderer reads it.

## Consequences

- One source of truth: the backend knows the program's names, and the result
  inherits them rather than the call site copying them onto every send.
- Uniform across engines: the same `register_program_instructions` /
  `register_cpi_event` call works on any backend; an engine that doesn't model a
  facet ignores it (the no-op default).
- A per-result `.with_*()` still exists for an override, but it is the exception,
  not the path.
- This is also what makes ecosystem visibility legible: a facet a backend can
  honor is a socket it overrides; one it can't is a no-op, visible at the trait.

## Alternatives considered

- **Decorate each result (`.with_aliases().with_names()`).** Rejected: it splits
  the source of truth (the backend has the names, the result wouldn't unless
  copied) and burdens every call site with remembering the vocabulary on every
  send. Events were briefly stuck in this shape and it was the friction that
  motivated this ADR.

## References

- [`../design/trait-boundaries.md`](../design/trait-boundaries.md).
