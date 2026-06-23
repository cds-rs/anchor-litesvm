# Architecture Decision Records

These record the decisions the testing framework rests on: the ones already
made and shipped, not proposals. Each is a short
[Nygard-style](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions.html)
record (Status, Context, Decision, Consequences, Alternatives), subject to
review.

The decisions were made across the framework's development; this folder
*solidifies* them, with each Context drawn from the prose design docs and the
commit history that produced it. The narrative docs explain how the pieces fit
together; these explain why each piece is shaped the way it is.

| ADR | Decision |
|---|---|
| [0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md) | **The premise**: trait boundaries for composability across engines, ecosystem service-visibility, and fork-and-adapt to iterate without interrupting maintainers (read first) |
| [0001](0001-executor-owns-the-execution-vocabulary.md) | The executor owns the execution vocabulary; consumers name and render it |
| [0002](0002-one-engine-port-as-a-trait.md) | One engine port as a trait (`TestSVM`), not an enum of engines |
| [0003](0003-vocabulary-on-the-backend-as-trait-sockets.md) | The test vocabulary lives on the backend, registered through trait sockets |
| [0004](0004-engine-neutral-record-and-vocabulary-propagation.md) | One engine-neutral record; one builder; `From` carries the vocabulary up |
| [0005](0005-no-framework-dependency-decoders-as-closures.md) | The vocabulary crate names no program framework; decoders cross as closures |
| [0006](0006-thin-vocabulary-crate-engines-own-their-lockfile.md) | The vocabulary crate is thin; an engine that can't share a lockfile is its own workspace |
| [0007](0007-declare-the-trace-asymmetry-via-capabilities.md) | Declare the per-frame-trace asymmetry through capabilities; don't hide it |
| [0008](0008-key-self-cpi-events-by-program.md) | Key self-CPI event decoders by program |

## Narrative companions

- [`../design/trait-boundaries.md`](../design/trait-boundaries.md): how the
  layers, the port, and the vocabulary fit together (the overview these ADRs
  decompose).
- [`../design/litesvm-boundary.md`](../design/litesvm-boundary.md): the
  executor/consumer line (ADR 0001, 0007).
- [`../design/endpoint-agnostic-architecture.md`](../design/endpoint-agnostic-architecture.md):
  the surfpool-era origin, under the pre-rename `ExecutionBackend` names
  (provenance for ADR 0002).
