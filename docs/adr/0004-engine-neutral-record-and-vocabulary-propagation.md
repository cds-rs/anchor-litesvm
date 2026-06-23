# ADR 0004: One engine-neutral record; one builder; `From` carries the vocabulary up

- Status: Accepted
- Applies the premise in [ADR 0000](0000-the-premise-trait-boundaries-and-ecosystem-reach.md).

## Context

Every engine must produce one record type the renderers consume, or each engine
grows its own renderers. And the rich litesvm renderers (`TransactionResult`)
must see the vocabulary the backend registered ([ADR 0003](0003-vocabulary-on-the-backend-as-trait-sockets.md)),
or the test is forced to re-attach it after every send.

## Decision

`model::Transaction` is the engine-neutral record `send` returns: the structured
`frames`, the `account_keys` they index, the `logs`, the `Option` fee (absent, not zero),
the optional per-frame `trace`, and the four vocabulary tables (aliases,
instruction names, error names, events) in effect when the backend sent it.

`model::Transaction::assemble(..)` is the one builder every adapter calls, so a
new field or naming rule touches one function, not three.
`TransactionResult::into_model(..)` is the shared litesvm extraction the two
litesvm-based senders both use. And `From<model::Transaction> for
TransactionResult` propagates the trace *and the four vocabulary tables* onto
the rich result.

## Consequences

- The round trip from `backend.send()` to a rendered `TransactionResult` loses
  nothing: register on the backend, `.into()`, render named and aliased, no
  re-attach.
- A renderer is written once against `model::Transaction`, not once per engine.

## Alternatives considered

- **Per-engine record types and per-engine renderers.** Rejected: N renderers,
  and cross-engine output that drifts.
- **`From` carrying only the trace.** Rejected: it forced the test to re-attach
  the vocabulary on the rich result; this was the actual bug that prompted
  pinning the four tables onto the record.

## References

- [`../design/trait-boundaries.md`](../design/trait-boundaries.md).
