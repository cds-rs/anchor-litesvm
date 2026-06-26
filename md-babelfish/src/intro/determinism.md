# Deterministic Identities

`Keypair::new()` hands you a fresh address every run. Fine, until you want to
*commit* a test's output: a structured-log dump or a `Report` re-rolls every address
(and the compute units derived from them) on each run, so it never diffs clean.

Seed the cast instead. **Declare your actors first**, by name:

```rust
let maker = ctx.cast_actor("maker");
let taker = ctx.cast_actor("taker");
```

[`ctx.cast_actor(name)`](../running/accounts-as-actors.md) derives a keypair from
`(program_id, name)`, funds it, and aliases it under `name`, in one line. The name is
the seed, so the same name yields the same address every run; the context rejects a
duplicate. For an exact starting balance, `cast_actor_with_sol(name, lamports)`.

Outside a context (a raw `litesvm` or Pinocchio harness), the same derivation is
`deterministic_keypair(domain, role)`; `ActorRegistry::new(domain)` adds a
duplicate-role guard when actors are created in more than one place.

## Actors first; everything else is a leaf

This is the **actors-first** idiom: the actors are the roots, and every mint, PDA,
and ATA *derives* from them. A random mint would churn the whole tree downstream, so
seed the mints too, with `cast_mint`:

```rust
let mint_a = ctx.cast_mint("A", &maker, 9);
```

`cast_mint` derives the mint from its name, creates it under `maker`'s authority, and
aliases it.

Pin the roots and the entire address tree is fixed: the escrow PDA derived from
`maker + seed`, the vault ATA from `escrow + mint_a`, and so on down. Declare the
cast at the top of the test; let the leaves fall out of it.

## The payoff: output becomes a snapshot

With identities seeded, a test's output is byte-stable across runs. That promotes it
from throwaway logging to a committable artifact: write the structured tree or the
`Report` to a file, commit it, and a later diff means the *behavior* changed, not
that the keypairs rolled.

[Part IV](../inspect/cpi-tree.md)'s views, the CPI tree, the mermaid diagrams, and the
authority and ownership graphs, become regression snapshots once the addresses under
them stop moving. The project's real
example programs lean on exactly this: each commits a generated test report that
diffs clean run to run.

## In CI: the snapshot is a regression gate

A byte-stable report is a golden file, so CI can regenerate it and fail on any drift:

```yaml
- run: just testrun                   # regenerate the committed report
- run: git diff --exit-code TESTRUN.md
```

A clean diff passes. A non-empty diff fails the job and shows exactly what moved: a
compute-unit delta, a reordered CPI, an added account. The reviewer decides whether
it was intended (commit the new snapshot) or a regression (fix the code). One gate
asserts the whole transaction shape at once, with no per-field assertions to maintain.

See [Test-Output Conventions](../appendix/conventions.md) for the full
commit-one-canonical-file workflow.
