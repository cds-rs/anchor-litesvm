# Capabilities: What's Visible Where

> For the chooser and the contributor: read what each engine surfaces, and where the gaps point.

Every engine implements the same `TestSVM` port, so a test written once runs on
any of them. The engines differ in what they can *show* you: litesvm witnesses
execution in-process, while a stock RPC endpoint sees only what the cluster
returns. Babelfish states that difference on the port, as a `Capabilities` flag
set, so a consumer reads what an engine surfaces before it renders.

```rust
{{#include ../../../crates/testsvm/src/lib.rs:capabilities}}
```

Total observability is the **standard**: the set litesvm reaches. Each engine
declares how much of it it provides, and that declaration is the contract a
consumer reads.

## The matrix

| Capability | litesvm | quasar-svm | mollusk | surfpool / RPC |
|---|:--:|:--:|:--:|:--:|
| `per_frame_trace` (authority / ownership graphs) | ✓ | ✓ | — | — |
| `structured_cpi` (frames from facts, not the log parse) | ✓ | ✓ | — | — |
| `atomic_send` (one budget-shared transaction) | ✓ | ✓ | — | ✓ |
| `fees` (the engine models fees) | ✓ | — | — | — |
| `instant_reset` (a fresh VM per test) | ✓ | ✓ | ✓ | — |
| `fork` (forks live cluster state) | — | — | — | ✓ |

litesvm carries the full observability set (`per_frame_trace`, `structured_cpi`,
`atomic_send`, `fees`). The others provide a subset alongside what each brings:
quasar-svm runs a genuinely independent SVM and sources both its per-frame trace
and its CPI frames from its own execution record, so it carries `structured_cpi`
too (it stops short of `fees`); mollusk runs at the instruction level and keeps
reset instant; surfpool / RPC reaches live cluster state (`fork`), and surfaces
the facts a stock endpoint witnesses.

## Reading a flag

A capability that crosses the trait is inherited by every consumer above it; one
that does not is declared at the trait, so you read it off the boundary. The
authority graph is built from `per_frame_trace`, so on an engine that provides it
the graph renders; on one that does not, a consumer caveats "no per-frame trace
on this endpoint" and reports what it has. An endpoint that later surfaces the
trace flips the flag, and the consumer renders the full graph with no change.

## The gaps are the work

Read the matrix the other way: each `—` marks a fact that engine has yet to
surface, which is the contributor's map. `per_frame_trace` is open on mollusk
because it runs at the instruction level; exposing its inner-instruction trace
flips the flag, and the authority graph and the inner-instruction names follow.
The standard names the target; the gaps name the next move, and that is the
feedback this framework hands back to each engine.

*The fact that would otherwise be lost here: which facts an engine can show you
at all, recoverable by reading one flag instead of running a transaction to find
out.*
