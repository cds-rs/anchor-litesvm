# Anchor Version Compatibility

The framework ships on two lines. Default to the main line; reach for the
compatibility line only when a dependency pins an older Anchor.

| line | branch | Anchor | use it when |
|---|---|---|---|
| main | `turbin3` | 1.0+ | the default: the full surface |
| compat | `compat/anchor-0.31` | 0.31 | a dependency forces Anchor 0.31 (Metaplex `mpl-core`) |

The compat line exists for one reason: `mpl-core` (Metaplex Core) pins
`anchor-lang` 0.31, so a program that CPIs into it cannot build against the main
line until `mpl-core` supports a newer Anchor. Point the dependency at the branch:

```toml
anchor-litesvm = { git = "https://github.com/cds-rs/anchor-litesvm", branch = "compat/anchor-0.31" }
```

## What the compat line has

The `AnchorContext` setup surface is the same: the cast vocabulary (`cast_actor`,
`cast_actor_with_sol`, `cast_account`, `cast_mint`, `fund_ata`, `alias_ata`),
`build_with_program_from_file`, and the `Bundle` derives. The account-loading API
matches the main line too: `try_load` for a `Result`, `load` to panic on a missing
or malformed account. And the *log-based* narration carries over: the `Report`
recorder, the structured CPI tree (`print_logs_structured`), the mermaid sequence
diagrams, and `print_markdown_pair`.

A litesvm + Anchor test reads identically on either line:

```rust
let mut ctx = AnchorLiteSVM::build_with_program(program::ID, "program", PROGRAM_SO);
let maker = ctx.cast_actor("maker");
let mint = ctx.cast_mint("A", &maker, 6);
let acct: MyState = ctx.load(&pda);
```

## What it lacks

The compat line predates the `TestSVM` trait extraction, so the engine-agnostic
layer isn't there: no `TestSVM` trait, no `prop_mint` / `prop_token_account`
fabrication, no mollusk or RPC backends, and `AnchorContext` does not implement
`TestSVM`. The compat line is litesvm + Anchor only; for Pinocchio or
multi-engine testing, use the main line.

It also lacks the *trace-based* observability: the authority and ownership graphs
and the per-test execution snapshot (`report_execution`). These read a per-frame
execution trace recorded through a litesvm callback (`invocation-inspect-callback`)
that the main line's litesvm carries but compat's pinned `litesvm 0.6.1` does not.
Closing the gap is a real undertaking rather than a quick port: the callback-bearing
litesvm is on a newer solana than anchor-0.31 and `mpl-core` allow, so the callback
would have to be backported onto the older litesvm and the trace-consuming renderers
ported alongside it. Until then, the log-based views above (tree, mermaid, `Report`)
are the narration compat has, since they never touch the trace; for the full suite,
use the main line.

## Moving off it

When `mpl-core` supports a newer Anchor, move the dependency back to `turbin3`:
the suite inherits the full surface unchanged. The cast vocabulary it already
uses is identical, so the only edit is the branch name. (If you pinned an older
compat commit, it may spell the account loader `get_account` rather than
`try_load`/`load`; rename those call sites.)
