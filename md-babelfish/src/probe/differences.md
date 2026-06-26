# Reading the Differences

> For the contributor and the auditor: where the runtimes agree, and where they don't.

The probe ran one spec on four runtimes. They agreed on behavior and disagreed
on diagnostics, which is exactly the design: behavior is portable; diagnostics
vary by what each engine can witness (the
[Capabilities](../inspect/capabilities.md) matrix).

## Same behavior, same cost

Every runtime read the counter as `1`, and the compute cost was identical:
initialize `6,364` CU, increment `3,403` CU, on each engine. That is the
conformance result, the same BPF program giving the same answer. Had two engines
disagreed on the count or the cost, the spec would have caught it: the test
decides.

## Different diagnostics

Where they differ is what each can show you. The initialize transaction makes
one inner CPI, the System program's `CreateAccount`. quasar-svm carries a
per-frame trace, so it names that inner frame:

```text
└── counter::Initialize [1] ✓ 6364cu  signer=Alice
    └── System::CreateAccount [2] ✓ (no cu)
```

mollusk has no per-frame trace (`per_frame_trace: false`), so it reconstructs
the tree from the logs alone, and the inner frame is a bare `System`:

```text
└── counter::Initialize [1] ✓ 6364cu  signer=Alice
    └── System [2] ✓ (no cu)
```

Same transaction, same cost; one render names the inner instruction, the other
stops at the program. That difference is the `per_frame_trace` capability flag,
made visible. A consumer reads the flag and knows which render to expect, instead
of meeting a half-named tree by surprise.

## The gap is the finding

This is what the framework hands back to each engine: mollusk's bare `System` is
the precise, located place where the inner-instruction fact is lost. Map
mollusk's inner-instruction data to flip `per_frame_trace`, and the name appears
with no change to the spec or the renderer. The test decided what should be
observable; the gap names the next move.

*The fact that would otherwise be lost here: that two runtimes agree on behavior
to the compute unit while differing only in what they can show, the difference
read off a capability flag rather than discovered by surprise.*
