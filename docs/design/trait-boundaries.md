# Traits as boundaries: the shape of the testing vocabulary

**Status:** design doc. Records the architecture as it stands in this repo: the
layered crates, the `TestSVM` port, and the rule that every fact a test needs
from a backend crosses a *trait* boundary, never a concrete type. Companion to
[`litesvm-boundary.md`](litesvm-boundary.md) (which draws the executor/consumer
line) and [`endpoint-agnostic-architecture.md`](endpoint-agnostic-architecture.md)
(the surfpool/RPC angle; note it predates the `ExecutionBackend -> TestSVM`
rename, so read its `ExecutionBackend`/`ExecutionRecord` as today's `TestSVM`/
`model::Transaction`).

## Scope

In scope: why the crates are split the way they are, what the `TestSVM` trait is
the boundary *for*, and how the naming and decoding vocabulary (aliases,
instruction names, error names, event decoders) reaches a rendered result
without any consumer threading it by hand.

Out of scope: the renderers themselves ([`cpi-rendering.md`](cpi-rendering.md)),
the derive macros ([`bundled-pubkeys.md`](bundled-pubkeys.md)), and the
executor-owned-observer direction (sketched at the end of the boundary doc).

## The organizing idea

One vocabulary, many engines. A test is written once, in a single vocabulary,
and runs against any execution engine that implements that vocabulary: an
in-memory litesvm, an instruction-level mollusk harness, or a live cluster over
JSON-RPC. The thing that makes "any engine" real is that the vocabulary is a
*trait*, not a struct an engine happens to expose. Everything a test asks of a
backend (send this, fund that, name this account, decode that event) is a method
on `TestSVM`; nothing reaches around the trait to a concrete engine type.

So the boundary is the trait, and the discipline is: a fact a test needs from
the backend is registered *on the backend* through a trait method, and the
backend stamps it onto the record it returns. The layers above read it. They
never re-derive it, and they never re-attach it.

## The layers, bottom up

```
testsvm            the vocabulary: the TestSVM port, model::Transaction,
                   Frame, the (code -> name) tables, the event registry.
                   Thin type crates only; no engine, no framework.
   ^   ^   ^
   |   |   +----------------------------+
   |   +-----------------+              |
litesvm-utils      testsvm-mollusk   (RpcBackend lives in
  LiteSvmBackend     MolluskBackend    litesvm-utils behind
  + the renderers    (own workspace,   feature = "rpc")
  + Report           own lockfile)
   ^
   |
anchor-litesvm     the Anchor facade: AnchorContext is itself a TestSVM
  AnchorContext    engine, plus the bundle/cast/event sugar on top.
```

`testsvm` sits at the bottom on purpose: it carries only thin type crates
(`solana-pubkey`, `solana-instruction`, and friends, with loose ranges), no
engine and no program framework. That is what lets it sit in *any* engine's
dependency graph: litesvm's solana-3.x line and mollusk's agave pins can both
hold it without one forcing a version on the other. The vocabulary is the one
crate everyone shares; the engines are adapters that depend *up* into it.

**Remark.** This is why mollusk is its own workspace with its own lockfile, not
a member of the main one. No single lockfile can hold litesvm's and mollusk's
solana pins at once, so the vocabulary crate is the seam they meet at, each in
its own graph, rather than a shared workspace they fight over.

## The port: `TestSVM`

`TestSVM` (in `testsvm/src/lib.rs`) is the seam every engine implements and
every test speaks. It splits cleanly into a small required core and a layer of
default methods built over it.

The required core is the irreducibly engine-specific part:

- `send(&mut self, ixs, signers) -> model::Transaction` runs a transaction and
  returns the engine-neutral record (see below);
- the account and clock levers: `fund_sol`, `set_account`, `get_account`,
  `account_owner`, `deploy_program`, `warp_to_slot`, `warp_to_timestamp`,
  `clock`;
- `capabilities()` declares what this engine can witness (more on this under the
  executor boundary);
- `aliases()` hands out the engine's alias table.

Everything else is a default method written once over that core: the cast
helpers (`actor`, `prop`, `prop_at`, `deploy_from_file`), `label`, and the
registration sockets. An engine that implements the core gets the whole
vocabulary for free; it overrides a default only where it can do better (a
surfnet `RpcBackend`, for instance, overrides `register_alias` to *also* push
the alias to the endpoint's own renderer).

**Remark.** Lifting the cast and naming helpers to trait defaults over an
`aliases()` accessor (rather than duplicating them in each adapter) is the
decision that keeps a new engine cheap: a third backend is the core methods plus
whatever sockets it chooses to honor, and it inherits `actor`, `prop`, and the
naming workflow unchanged.

## The vocabulary as trait sockets

This is the part the title is about. A test needs to attach names and decoders
to an execution so the rendered output reads in domain terms (`alice` and
`Subscribe`, not base58 and a raw discriminator). There are two places that
attachment could live: on the *result* (the test decorates each returned record)
or on the *backend* (the test registers once, and every send carries it). The
design chooses the backend, through trait sockets:

| Vocabulary | Socket on `TestSVM` | Backing table |
|---|---|---|
| `pubkey -> name` aliases | `register_alias` | `Aliases` |
| instruction `(disc -> name)` | `register_instruction_name` / `register_program_instructions` | `InstructionNames` |
| error `(code -> name)` | `register_error_name` / `register_program_errors` | `ErrorNames` |
| logged event decoders | `register_event_decoder` | `EventRegistry` |
| self-CPI event decoders | `register_cpi_event` | `EventRegistry` |

Each socket has a default no-op body, so an engine that doesn't model a given
facet simply ignores the registration. An engine that does (every litesvm-backed
one) stores it in a table it holds, and `send` stamps every table onto the
`model::Transaction` it returns. The single rule that falls out:

> Register on the backend once; the record carries the vocabulary; every renderer
> reads it. No consumer re-threads a table, and no consumer re-derives a name.

The alternative (decorating each result with `.with_aliases(..).with_names(..)`)
works, but it makes the *call site* responsible for remembering the vocabulary on
every send, and it splits the source of truth: the backend knows the program's
names, yet the result wouldn't unless the test copied them over. Putting the
sockets on the trait makes the backend the single source, uniform across every
engine.

**Remark (events were the last to cross).** The event decoders joined the trait
late, and only after the registry moved *down* into `testsvm`: while it lived in
`litesvm-utils` (a layer above the trait), `TestSVM` could not name it, so events
were the one facet a test still had to attach to the result by hand. Moving the
registry beside the name tables closed that, and events became a socket like the
rest. The self-CPI socket (`register_cpi_event`) is keyed by `(program, prefix)`,
not the prefix alone, because the self-CPI tag is shared across every
anchor-compatible program; keying by the emitting program keeps a transaction
that composes two event-emitting programs from cross-decoding. (Logged events
keep a bare 8-byte key: Anchor derives those from the event name, so they are
unique across programs.)

## The model is the lingua franca

`model::Transaction` (in `testsvm/src/model.rs`) is what every `send` returns and
every renderer consumes: the structured CPI `frames`, the `account_keys` they
index against, the `logs`, the `Option` fee (absent, not zero), the optional per-frame
`trace`, and the four vocabulary tables (aliases, instruction names, error names,
events) in effect when the backend sent it. It is engine-neutral by construction:
it names no engine type, so the tree it renders is the same whether the frames
came from litesvm's `cpi_tree` parse or mollusk's.

Two builders keep this single-sourced:

- `model::Transaction::assemble(..)` is the one place a record is built from an
  adapter's raw extraction (frames, logs, outcome, plus the vocabulary). Every
  adapter calls it, so a change to naming or a new field touches one function,
  not three.
- `TransactionResult::into_model(..)` is the shared litesvm extraction (the
  `cpi_tree` parse to frames, then `assemble`), so the two litesvm-based senders
  (`LiteSvmBackend` and `AnchorContext`) produce their record one way, not two.

The bridge back up is `From<model::Transaction> for TransactionResult` in
`litesvm-utils`: it carries the trace *and the four vocabulary tables* onto the
rich result, so the tree, mermaid, authority, and ownership renderers resolve
names from the backend's registries with nothing re-attached. (That propagation
is the concrete payoff of putting the vocabulary on the record: the round trip
from a backend's `send` to a rendered `TransactionResult` loses nothing.)

## The type-erasure boundary

`testsvm` carries no `anchor-lang`, nor any program-framework, dependency. It
cannot name an event type or call `try_from_slice` on one. So the event decoders
that cross its boundary are type-erased closures:

```
type EventDecoder = Arc<dyn Fn(&[u8]) -> Option<Vec<(String, String)>> + Send + Sync>;
```

The concrete event type lives entirely inside the closure, built where the type
*is* known: `anchor-litesvm`'s `register_event::<E>()` constructs a
`move |bytes| E::try_from_slice(bytes)...` and hands `testsvm` the discriminator,
the name, and that closure; a hand-rolled Pinocchio decoder does the same with
its own field offsets. `testsvm` stores `(discriminator, name, decoder)` and
never sees a framework type. The instruction and error tables cross the same way,
only simpler: they are plain `(code, name)` pairs, no closure needed.

**This is what makes one vocabulary serve both Anchor and Pinocchio.** The
framework-specific knowledge (how to deserialize an event, what an instruction's
discriminator is) stays in the framework-aware crate; the vocabulary crate holds
only the erased result. A Pinocchio program with no IDL declares its `(code,
name)` tables through `litesvm-pinocchio`'s macros and plugs them into the same
sockets an Anchor program's IDL fills.

## The executor boundary

The one fact the vocabulary cannot manufacture is the per-frame privilege trace:
which accounts each frame saw as signer or writable, including the `invoke_signed`
PDAs a transaction message header cannot show. Only the executor witnesses it.
litesvm (the `cds-rs/litesvm` fork) exposes it through an
`invocation-inspect-callback` hook, and `LiteSvmBackend` installs a recorder on
it; a stock RPC cannot see it, so `RpcBackend` leaves the trace `None`.

`capabilities()` is how the trait surfaces the asymmetry: a `per_frame_trace`
flag lets a report annotate the degraded case instead of emitting a half-empty
authority graph. The deeper argument for where these facts belong (and the
direction that would dissolve the reconstruction the testing layers still do)
is the subject of [`litesvm-boundary.md`](litesvm-boundary.md).

## What it buys: one scenario, every engine

Because the boundary is the trait, a single scenario written against `TestSVM`
runs unchanged on each engine, and the conformance test in `testsvm` pins exactly
that: one scenario, executed through `litesvm`, `mollusk`, and an `RpcBackend`,
asserting each produces the same shape. A test that wants the in-memory speed of
litesvm and the fidelity of a forked cluster doesn't choose at authoring time; it
chooses the backend at construction, and the vocabulary, the model, and every
renderer above behave identically. The trait boundary is what turns "test against
X" into a one-line backend swap.

## Provenance: where this came from

This shape did not arrive at once, and the [endpoint-agnostic
doc](endpoint-agnostic-architecture.md) records its origin under the names it
carried then. The port began on the surfpool branches as `ExecutionBackend`,
with a single in-memory `LiteSvmBackend` and a record type called
`ExecutionRecord`. An `RpcBackend` over JSON-RPC followed (a surfnet endpoint),
which proved the real claim: the same scenario could run in-memory or against a
live cluster and render the same. A third engine, the mollusk adapter, made the
trait's generality concrete rather than aspirational, and at that point
"execution backend" undersold what the seam had become, so it was renamed
`TestSVM` and the record `model::Transaction`: not "the thing that executes" but
the vocabulary every engine and program framework speaks. The naming sockets,
the event sockets, and the type-erasure boundary accreted onto that port
afterward, the event sockets last (see the remark above).

So read the endpoint-agnostic doc for the surfpool round-trip and the RPC
trace asymmetry in their original form; read this doc for the port and the
vocabulary as the source defines them today. Where the two disagree on a name,
the source (and this doc) win.
