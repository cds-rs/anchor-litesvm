# ADR 0008: Key self-CPI event decoders by program

- Status: Accepted
- Applies the premise in [ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md).

## Context

An `emit_cpi!`-style event leaves no `Program data:` log; the program emits it
by invoking itself with `EVENT_IX_TAG ++ disc ++ fields` as the instruction
data, and the payload is recovered from the inner instruction's data (which the
trace carries). The tag is a constant, `Sha256("anchor:event")[..8]`, *shared by
every anchor-compatible program*, and the discriminator after it is short (often
a single byte). A registry keyed on that prefix alone collides across programs:
two composed programs with an event at disc 0 share a prefix.

## Decision

Key the self-CPI event registry by `(program, prefix)`, and pass the emitting
frame's program at decode. The renderer already holds `frame.program`, so it is
free to supply. Logged events keep a bare 8-byte discriminator key: Anchor
derives those from the event name, so they are unique across programs and need
no program qualifier.

## Consequences

- A transaction that composes two event-emitting programs decodes each one's
  events correctly; no cross-program collision, no silent mis-decode.
- The self-CPI socket (`register_cpi_event`) takes a `program_id`, which lines
  it up with `register_instruction_name` and the rest of the per-program
  vocabulary ([ADR 0003](0003-vocabulary-on-the-backend-as-trait-sockets.md)).

## Alternatives considered

- **Key by prefix alone.** Rejected: the shared tag plus a short discriminator
  collides the moment two programs both emit anchor-cpi events, exactly the
  composition the framework is meant to make legible.

## References

- [`../design/trait-boundaries.md`](../design/trait-boundaries.md).
