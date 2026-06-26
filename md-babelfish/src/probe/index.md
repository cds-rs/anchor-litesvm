# The Probe

> For the chooser and the contributor: one test, every runtime, run for real.

A test describes behavior, not allegiance: write it once, and any engine answers
to it. This chapter is that claim made runnable. One program (a counter), one
specification, four runtimes.

## The specification

The spec is written once, in the engine-neutral `TestSVM` vocabulary. It takes
any engine, deploys the counter, initializes it at 0, increments once, and reads
the count back:

```rust
{{#include ../../listings/probe/probe-spec/src/lib.rs:spec}}
```

## A runner per engine

Each runtime gets a thin runner. Only the two lines that name the engine change
between them; the spec is identical:

```rust
{{#include ../../listings/probe/runner-litesvm/tests/probe.rs:runner}}
```

Swap `LiteSvmBackend::new(LiteSVM::new())` for `QuasarBackend::new()` or
`MolluskBackend::new()` and the same spec runs on quasar-svm or mollusk. Each
runner is its own crate with its own lockfile, because the engines pull
incompatible dependency graphs (the litesvm, mollusk, and quasar graphs each
need their own lock) — the price of running genuinely independent engines.

## Every runtime answers

| Runtime | counter spec | initialize CU | increment CU |
|---|:--:|:--:|:--:|
| litesvm | ✓ | 6,364 | 3,403 |
| quasar-svm | ✓ | 6,364 | 3,403 |
| mollusk | ✓ | 6,364 | 3,403 |
| surfpool / RPC | ✓ (live) | — | — |

The three in-memory runners are CI-tested. The fourth runtime is a live surfnet,
so it ships as a gated example rather than a test.

## The deploy seam

One step differs by engine, and it is worth naming: in-memory engines deploy the
program in-process, so `run_counter_probe` reads the `.so` and loads it. Over a
stock RPC the program is deployed by the surfnet first, so that path calls
`run_counter_spec` directly — same spec, minus the in-process deploy:

```rust
{{#include ../../listings/probe/runner-surfpool/examples/probe.rs:example}}
```

*The fact that would otherwise be lost here: that one specification holds across
four independent runtimes, shown by running it rather than asserted.*
