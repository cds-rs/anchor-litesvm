# ADR 0005: The vocabulary crate names no program framework; decoders cross as closures

- Status: Accepted
- Applies the premise in [ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md).

## Context

The vocabulary crate (`testsvm`) must serve both Anchor and Pinocchio programs.
If it depended on `anchor-lang` (to name an event type or call `try_from_slice`),
it would force that dependency on every engine that holds the vocabulary and on
Pinocchio programs that have no business compiling Anchor.

## Decision

`testsvm` carries no program-framework dependency. The framework-specific
knowledge crosses its boundary type-erased:

- Event decoders are `EventDecoder = Arc<dyn Fn(&[u8]) -> Option<Vec<(String,
  String)>> + Send + Sync>` closures, built where the concrete type *is* known.
  `anchor-litesvm`'s `register_event::<E>()` constructs a `move |bytes|
  E::try_from_slice(bytes)...`; a Pinocchio decoder does the same with its own
  field offsets. `testsvm` stores `(discriminator, name, decoder)` and never
  sees the type.
- Instruction and error names cross as plain `(code, name)` pairs, no closure
  needed.

## Consequences

- One vocabulary serves both worlds: an Anchor program registers from its IDL, a
  Pinocchio program declares its `(code, name)` tables through
  `litesvm-pinocchio`'s macros, and both fill the same sockets.
- The framework-aware crate owns the framework knowledge; the vocabulary crate
  stays a thin, framework-free seam (see [ADR 0006](0006-thin-vocabulary-crate-engines-own-their-lockfile.md)).

## Alternatives considered

- **An `anchor-lang` dependency in the core.** Rejected: it poisons every
  non-Anchor engine and every Pinocchio program with a framework they don't use.

## References

- [`../design/trait-boundaries.md`](../design/trait-boundaries.md).
