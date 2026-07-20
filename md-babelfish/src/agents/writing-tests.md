# Writing Tests

One canonical suite shape. The code below is the escrow listing, compiled and
run in CI; adapt names, keep the structure.

## Anatomy

1. **World**: one `setup` function builds everything the scenario needs and
   returns one struct. The suite owns it (`tests/common/mod.rs`).
2. **Cast list**: inside setup, every account is created deterministically and
   aliased before any instruction runs.
3. **Act**: `ctx.tx(&signers).build(bundle, args).send_ok()` (or
   `send_err_named` for an expected failure).
4. **Assert**: `md.check(label, expected, actual)` records and asserts in one
   call; account state comes from `ctx.load` (`ctx.try_load` for a `Result`),
   balances from `ctx.svm.token_balance` / `ctx.svm.get_balance`.

## World setup (the scenario verb)

```rust
{{#include ../../listings/escrow/programs/escrow/tests/common/mod.rs:setup}}
```

The shortcut form: `ctx.cast_actor("maker")` replaces the registry dance
(deterministic keypair, 100 SOL, aliased), `ctx.cast_actor_with_sol(name,
lamports)` casts at an exact stake, and `ctx.cast_account("recipient")` covers
passive accounts. For token plumbing, `ctx.cast_mint(name, &authority, decimals)`
casts a mint and `ctx.fund_ata(&owner, &mint, &authority, amount)` hands a holder
a balance in its aliased ATA, each in one call.

## Happy path, narrative shape

```rust
{{#include ../../listings/escrow/programs/escrow/tests/test_make.rs:make}}
```

The plain Arrange // Act // Assert shape is this test minus the `Report`:
build the context, call setup, send, then `assert_eq!` on the same accessors.
Prefer the narrative shape in a suite; the Markdown reports it writes under
`target/md-reports/` double as committed, byte-reproducible baselines.

## Expected failure, with the clock

```rust
{{#include ../../listings/escrow/programs/escrow/tests/test_refund.rs:negative}}
```

`send_err_named("EscrowNotExpired")` asserts the failure by its Anchor error
name; no error-code arithmetic. `ctx.svm.advance_days(19)` moves the clock
(seconds/slots variants exist; `warp_to_timestamp` pins an absolute time).

`send_err_named` is Anchor-context sugar. A suite on the bare `TestSVM`
trait (a Pinocchio program on mollusk, say) asserts on
`model::Transaction::error` instead; see
[Backends](backends.md#asserting-failure-at-the-trait-level).

## Policy loops

Each helper-mediated send is its own transaction under a fresh blockhash, so
a rate-limit or spend-cap test resends the identical instruction in a plain
loop; no blockhash management appears anywhere in the test:

```rust
for _ in 0..3 {
    ctx.tx(&[&payer])
        .build(w.bundle, program::instruction::Spend { amount: 10 })
        .send_ok();
}
```

## Events

Register a program's events once, at setup, and the structured views render
them by name with destructured, alias-substituted fields (`🔔 Transfer { from:
maker, amount: 100 }`) instead of the raw `Program data:` blob. One line pulls
every event from the IDL:

```rust
ctx.register_events_from_idl(include_str!("../../target/idl/my_program.json"));
```

Per type instead of the whole IDL: `ctx.register_event::<my_program::events::Transfer>()`
(the event must `#[derive(Debug)]`). Registration rides on the backend, so every
send afterward carries it and the events appear in the CPI tree, the mermaid
note, and so in `report_execution`; nothing is attached per result.

A program that emits via self-CPI rather than a `Program data:` log (an
`emit_cpi!` engine, or a Pinocchio program on the bare `TestSVM` trait) registers
a decoder through `register_cpi_event(program_id, prefix, name, decoder)`
instead; the program id is part of the key, since the self-CPI tag is shared
across anchor-compatible programs.

## Snapshots

End a transacting test with `ctx.report_execution(&mut md)` to append the
execution overview (CPI tree, named actors, compute). Because every actor is
deterministic, the file is byte-stable and belongs in version control as a
regression baseline.

`report_execution` (and `spotlight`) are main-line only. On the
`compat/anchor-0.31` line, assemble the report from the log-based pieces it
does have, `logs_structured_string()`, `mermaid_string()`, and
`print_markdown_pair()`, since the trace-based execution snapshot isn't
available there. See [Anchor Version Compatibility](../appendix/anchor-compat.md).

When you splice a rendered piece into a `Report` by hand with `md.block`, match
the block type to the content. `MarkdownBlock::Fenced { lang, body }` wraps
*plain* text in a fresh code fence (the CPI tree, a log dump). `MarkdownBlock::Raw(..)`
splices a fragment that already carries its own fence, verbatim. The graph and
mermaid strings (`authority_graph_string`, `ownership_graph_string`,
`mermaid_string`) are already ` ```mermaid ` blocks, so they go in as `Raw`;
wrapping one in `Fenced` nests it inside a `text` code block, and it renders as
source instead of a diagram.

## Narrative on frood (source-free)

When the program under test arrives as a committed `.so` plus a Codama IDL
(no program crate anywhere in the graph), the same narrative discipline runs
on frood, the sol-babelfish story engine. The fundamental difference from the
`Report` builder above: nothing is written down as it happens. `Story` mints
a `Moment` per transaction and samples every registered observation into it;
the report is a projection of that trajectory, rendered once when the world
drops, and only when a report was asked for
(`FROOD_LINK_REPORT_DIR=<dir> cargo test`; a plain run pays nothing beyond
the flag read).

The suite declares its report standard once, in code, beside the world it
configures (there is no manifest to drift apart from the tests):

```rust
{{#include ../../listings/frood-narrative/tests/narrative.rs:standard}}
```

A world derives `Reporter`, holds the `Story` and its cast, and renders at
`Drop`. Views decode through `#[derive(FromValue)]`; an observation is
registered once and sampled at every moment on its own; a law (`monotonic`,
`latch`, `constant`, or a free predicate) is evaluated at every mint, and a
break is located to its `T`:

```rust
{{#include ../../listings/frood-narrative/tests/narrative.rs:view}}
```

```rust
{{#include ../../listings/frood-narrative/tests/narrative.rs:world}}
```

Sends are two verbs. `when_ok` asserts success and panics with the story so
far at the caller's line (setup, happy-path beats: anything whose failure
means "stop here"). `when_err` asserts a named refusal and settles it as a
Then claim (the security half of a suite):

```rust
{{#include ../../listings/frood-narrative/tests/narrative.rs:sends}}
```

A send outside `when`'s bundle vocabulary (a foreign program's instruction
in the transaction, explicit account metas) is still a named beat, through
`run_instruction_as`/`run_instructions_as`; leave it unlabeled and it
renders as a bare "(instruction)" heading, invisible to `count_actions`:

```rust
{{#include ../../listings/frood-narrative/tests/narrative.rs:raw}}
```

A test threads beats and settles terminal facts with `finally`; the
conclusion runs at world drop, so a law that broke and that nothing ever
asserted on still fails the test:

```rust
{{#include ../../listings/frood-narrative/tests/narrative.rs:narrative}}
```

A single test that deviates from the standard assigns its own
`ReportConfig::of(...)` through `report_state().config_mut()`, in the test
that owns the deviation (typed, rename-safe, runner-agnostic):

```rust
{{#include ../../listings/frood-narrative/tests/narrative.rs:attenuate}}
```

The rendered projection opens with the cast (every registered alias, full
addresses, sorted), addresses each transaction as `T<n>` with day offsets,
narrates clock warps between moments ("9 days pass"), tables the changed
observations with grouped integers (`10,000,000,000,000`), settles refusals
as "refused: <name>" claims with the observed error, draws the per-moment
diagram set the standard selects (a `folder` rides several views behind one
summary line), and closes with every law's and finally's
verdict.
