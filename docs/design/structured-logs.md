# Transaction structured logs: design

## Scope

This doc covers the renderer that turns a transaction's flat Solana log
stream into an annotated CPI invocation tree (the
`TransactionResult::print_logs_structured()` output), and the
layers that compose around it (`Aliases`, signer extraction, decoded
instruction names, the `send_ok` / `send_err_named` integration).

Definitions:

- **Frame**: one invocation of a program. A top-level instruction opens a
  root frame; each CPI opens a nested frame. The runtime brackets every
  frame with `Program X invoke [n]` and `Program X success|failed`.
- **Event**: one element in the renderer's flat intermediate stream
  ([`Event::Open`] / [`Event::Close`]). A well-formed transaction
  produces a balanced bracket sequence.
- **Alias**: a `(Pubkey, name)` substitution applied at render time.
- **Legend**: the footer of the rendered output listing
  `(name, full_pubkey)` for every alias that actually appeared in this
  render (well-known program aliases filtered out).

# Part 1: the tree renderer

Source files: `crates/litesvm-utils/src/transaction/tree.rs` (renderer),
`crates/litesvm-utils/src/transaction/tree/tests.rs` (tests).

## What's printed

```
── voting::CastVote ──
Transaction  signers=[alice]
└── voting ✓ 12340
    ├── System::CreateAccount ✓ 1230
    └── Token::TransferChecked ✓ 5670
Compute Units (this run): 19240
Fee: 5000 lamports

Legend (1):
  alice = 7xKXt...J9aB
```

The single-line `── <program>::<ix-name> ──` opener replaces an earlier
`=== Structured Transaction Logs ===` + `Instruction: <name>` pair plus
a `====` footer; once `Instruction:` itself became the de-facto opener,
both banners read as ceremony (especially in multi-tx tests, where each
print would carry its own pair). Batches (multi-ix transactions with no
single canonical name) skip the opener entirely; the renderer's
`Transaction  signers=[...]` line leads.

The legend's `(1)` is a count, not a header level. Well-known program
names (`System`, `Token`, ...) don't appear in it; they're noise once
you recognize them. The `(this run)` on the CU footer is a reminder
that the value drifts across runs (e.g. Anchor's `find_program_address`
iterates a different number of bumps for different random pubkeys).

## Inputs

The renderer takes three things from a transaction:

| Input | From | Role |
|---|---|---|
| `&[String]` | `TransactionMetadata::logs` | The flat log stream. Source of truth for the bracket structure. |
| `&InnerInstructionsList` | `TransactionMetadata::inner_instructions` | DFS pre-order CPI list; used to decode instruction names from discriminator bytes. Pass `&Vec::new()` if not available; events still emit, just without decoded names. |
| `&SignerInfo` | derived from `Message` (see [`signers::extract`]) | Lets each root frame display `signer=X`. |

Plus a `&mut LegendCollector<'_>` carrying the `&Aliases` resolver and
recording which aliases actually fired.

## Pipeline

Two passes, with a small enum as the intermediate:

```text
logs + inner_instructions
        │
        ▼
   to_events()  ──── walks logs, classifies each line, brackets opens/closes,
        │              decodes ix names from discriminator (well-known programs)
        │              or from "Program log: Instruction: <Name>" (Anchor)
        ▼
   Vec<Event>     [Open { program, instruction }, Open { ... }, Close { outcome, cu }, ...]
        │
        ▼
   fmt_events()  ──── box-drawing render; resolves aliases via collector;
        │              annotates root frames with signer=X
        ▼
   String
```

`render(...)` is the public-ish entry that wires both passes; the
`TransactionResult::logs_structured_string` method composes it with the
header (instruction name) and footer (CU, fee, legend).

## Data model: the event stream

```rust,ignore
enum Event {
    Open {
        program: String,          // raw base58, alias resolution at format time
        instruction: Option<String>,
    },
    Close {
        outcome: Outcome,         // Success | Failed { ... } | Truncated
        cu: Option<u64>,
    },
}

enum Outcome {
    Success,
    Failed { message: Option<String>, diagnostics: Vec<String> },
    Truncated,                    // synthetic close for an Open with no matching status line
}
```

Why a flat event stream rather than a tree? Two reasons:

1. **Logs are inherently flat.** Solana emits a linear stream of
   `Program X invoke [n]` and `success|failed` lines. The bracket
   discipline lets you reconstruct a tree, but the linear form matches
   how the data arrives and is easier to fold across.
2. **CU attribution stays unambiguous.** Compute units travel on the
   `Close` event, so there's no way to misattribute a child frame's CU
   to its parent during a partial parse.

`Outcome::Truncated` is its own variant (not just
`Failed { message: None, diagnostics: vec![] }`) so the formatter can
render a `(truncated)` marker rather than `✗`, and so an honest
"failed with no message" case doesn't get conflated.

## Frame annotations

Each `Open` carries the program ID; the rendered frame line composes
that with the annotations below, all collected at render time:

| Annotation | Source | Renders as |
|---|---|---|
| Program name | `Aliases` (well-known seeded by default, user-extended via `.with(pk, name)`); falls back to `<8>…<4>` truncation | `Token`, `voting`, `7xKXt…J9aB` |
| Instruction name | (a) `decode_instruction` against discriminator, for well-known programs; (b) `Program log: Instruction: <Name>` emitted by Anchor's dispatcher | `Token::TransferChecked`, `voting::CastVote` |
| Outcome | `Close::outcome` | `✓`, `✗ <message>`, `(truncated)` |
| CU | `Close::cu` from `Program X consumed N of M compute units` line | `12340` |

Root frames additionally get a `signer=X` annotation derived from
`SignerInfo::per_root[root_idx]`. CPI frames don't repeat signers (a
CPI by definition invokes a program; the *caller* held the signers,
which are already shown at the root).

### Two sources for the instruction name

The `Event::Open::instruction` field has two populators that fight for
the slot. First-write-wins (the second is guarded by an `is_none()`
check in the log-line branch of `to_events`):

1. **`decode_instruction(program_id, data)`** matches the first 1-8
   bytes of the instruction data against a small per-program
   discriminator table (System, SPL Token, ATA). Fires inline as each
   frame opens via the `ix_iter` walk of `inner_instructions`.
2. **`Program log: Instruction: <Name>`** is Anchor's generated
   dispatcher's "the handler starts here" log line, also emitted by SPL
   Token for its own ops. Fires inside `to_events`'s log-line loop and
   patches the *most recent* `Open` frame whose `instruction` is still
   `None`.

Why this ordering, you may wonder. The discriminator decode is cheaper
and more authoritative for the programs it covers. The Anchor
log line is the catch-all that gives user programs their names without
any IDL or per-program registration; it's correct to let the
discriminator decode win when both are available, since the
discriminator is the canonical handler identifier.

### Well-known programs

Listed in `aliases.rs::WELL_KNOWN_PROGRAMS` (System, Token, Token-2022,
AssociatedToken, ComputeBudget, BPFLoaderUpgradeable, Memo, Memo-v1).
Seeded by `Aliases::with_well_known()` (which `Aliases::default()`
delegates to). A user can override any of them via `.with(pk, "myname")`;
later inserts shadow earlier ones.

**N.B.** A user-renamed well-known program *does* show up in the
legend. The `aliases::is_well_known_name` filter checks the name, not
the pubkey, so a user-chosen name escapes the filter.

### Legend

`LegendCollector` wraps the `&Aliases` for the duration of the render
and records `(name, Pubkey)` pairs the first time each alias fires.
At footer time, the well-known names are filtered out
(`is_well_known_name`); the remainder is rendered as
`Legend (N):\n  name = full_pubkey\n` lines, in first-appearance order.

Dedup uses a linear scan over `Vec<(&str, Pubkey)>` because N is small
(dozens at the high end). A `HashMap` would lose the insertion order
that determines legend stability.

# Part 2: the composable layers

The renderer alone doesn't get you to a useful test log. A few other
concerns plug in around it: the alias map, the signer extractor, the
captured `InstructionInfo`, and the `send_ok` / `send_err_named`
shortcuts that drive the failure-path print.

## Layer map

```text
                                                  ┌──────────────────┐
                                                  │ Aliases          │
                                                  │  user-extensible │
                                                  └──────┬───────────┘
                                                         │ &Aliases
                                                         ▼
   ┌──────────────────┐   InstructionInfo   ┌────────────────────────┐
   │ TransactionResult├────────────────────►│ logs_structured_string │
   │  .instruction    │                     │  (composes header +    │
   │  .message        │   SignerInfo        │   tree::render +       │
   │  .logs/inner_ix  ├────────────────────►│   footer + legend)     │
   └────────┬─────────┘                     └────────────────────────┘
            │                                            ▲
            │                                            │
            ▼                                            │
   ┌────────────────────────┐                            │
   │ TransactionHelpers     │                            │
   │  send_ok(.., &aliases) │── failure-path print ──────┘
   │  send_err_named(...)  │
   └────────────────────────┘
```

## Available metadata, by source

| Metadata | Captured by | Carried on | Used where |
|---|---|---|---|
| Top-level instruction's program ID + data bytes | `InstructionInfo::from_instruction(&ix)` (in `send_instruction`) | `TransactionResult.instruction: Option<InstructionInfo>` | Header line; decoded ix name via `decode_instruction` |
| Source `Message` | clone of `tx.message` (in `send_instruction` / `send_transaction_result`) | `TransactionResult.message: Message` | `signers::extract` (signer set + per-root signer slices) |
| `inner_instructions` (DFS pre-order CPI list) | runtime, on `TransactionMetadata` | `TransactionResult.inner` | Per-frame decoded ix names for CPI children |
| `Aliases` map | caller (`Aliases::default().with(...)`) | passed in per call | Friendly name + legend at render time |
| Log stream | runtime, on `TransactionMetadata` | `TransactionResult.inner` | The whole pipeline; bracket structure + outcomes + CU |

`InstructionInfo` deliberately stays a minimal struct
(`{ program_id, data }`); the runtime's `Instruction` type also carries
account metas, but those are recoverable from the `Message`, so cloning
the whole thing would be redundant. `data` is `Box<[u8]>` (the original
allocation could be hundreds of bytes; we only ever read the first 1-8,
but copying the rest is cheaper than carrying lifetime-tracked slices).

## Ergonomic decisions (with cross-refs)

These are the choices that affect call-site shape. Each has a "why"
that boils down to ergonomics under real test workloads.

### `print_logs_structured()` (no arg), reading aliases from storage

The alias map is per-test data (the actors the test built), not
ambient. ADR-0002 originally surfaced it as a parameter (`print_logs_structured(&Aliases)`)
to keep the renderer pure. The convergent dogfood evidence then showed that
every real caller threads the same alias table through every send + print
call in a scenario, so [ADR-0003][adr-0003] reshaped the surface:
`TransactionResult` stores an optional `Aliases` (set by `with_aliases`
or stashed by the trait `send_*` methods on the way through), and
`print_logs_structured()` / `logs_structured_string()` read from storage
or fall back to `Aliases::default()`. The "different alias maps per
print on the same result" path is still reachable via `result.with_aliases(other_table).print_logs_structured()`.

### Bare-LiteSVM helpers take `&Aliases`; AnchorContext helpers don't

`TransactionHelpers::send_ok` / `::send_err_named` on `LiteSVM` accept
`aliases: &Aliases` and stash it on the returned result so a chained
`.print_logs_structured()` works without re-threading. The
`AnchorContext` wrappers (`ctx.send_ok` / `ctx.send_err_named`) read
`&self.aliases` and don't take the parameter at all. The split keeps
non-Anchor users unaffected while collapsing the Anchor surface to one
implicit table. Choice recorded in [ADR-0003][adr-0003] (which
supersedes the relevant parts of [ADR-0002][adr-0002]).

### `Aliases::default()` seeds well-known programs

Most renders involve System/Token/AssociatedToken; pre-seeding means
the default render is already readable without any per-test setup.
The seeded names are filtered out of the legend so the legend stays
focused on test-specific actors.

### `signer=X` on top-level frames only

Repeating signers on every CPI frame is visual noise; the
*invocation* is the unit that has signers (top-level only, since CPIs
inherit caller authority). A reader who needs to know "who signed
this CPI's parent" looks up one frame.

### Per-run CU label "(this run)"

Per-frame CU drifts across runs because Anchor's
`find_program_address` iterates a variable number of bumps for
different random pubkeys. The `(this run)` label is a one-time hint
that says "don't diff this number across runs and conclude there's a
regression."

### Decoded ix name on the header AND on every frame

The header (`Instruction: <Program>::<Name>`) is what the caller of
`send_instruction` actually issued; the per-frame names show what
each CPI invoked. Both are useful: the header for "what test am I
reading", the per-frame for "what did the program do." Same decoder
table feeds both; header is decoded once per render, frames are
decoded once per `Open`.

### Chain methods consume `self`; `tap` and `_with` cover the rest

Every chainable method on `TransactionResult` (`assert_success`,
`assert_failure`, `assert_success_with`, `assert_failure_with`,
`assert_error`, `assert_error_code`, `print_logs`,
`print_logs_structured`, `tap`) takes `self` and returns `Self`. The
chain ends in an owned binding the caller can keep using:

```rust,ignore
let result = svm.send_ok(ix, &[&payer], &aliases)
    .print_logs_structured()           // alias table stashed on the result by send_ok
    .assert_success();
```

Read-only methods (`compute_units`, `is_success`, `error`, `logs`,
`has_log`, `find_log`, `fee`, `inner`) stay `&self -> T`. They compose
into a chain through `tap`, which borrows the result for the closure
and hands ownership back:

```rust,ignore
let result = svm.send_ok(ix, &[&payer], &aliases)
    .tap(|r| println!("CU used: {}", r.compute_units()))
    .assert_success();
```

For the common "outcome holds AND predicate holds" pattern,
`assert_success_with` and `assert_failure_with` fold both checks into
one chain step (the `_with` suffix follows the same convention as
`Vec::with_capacity`):

```rust,ignore
let result = svm.send_ok(ix, &[&payer], &aliases)
    .assert_success_with(|r| r.compute_units() < 100_000);
```

# References

- Code: `crates/litesvm-utils/src/transaction/` (`tree.rs`,
  `aliases.rs`, `signers.rs`, `tree/tests.rs`).
- Public API: `TransactionResult::print_logs_structured`,
  `TransactionResult::logs_structured_string`,
  `TransactionHelpers::send_ok`, `TransactionHelpers::send_err_named`.
- [ADR-0002][adr-0002]: `&Aliases` parameter choice on the send helpers (superseded in part by ADR-0003).
- [ADR-0003][adr-0003]: aliases on `AnchorContext` and on `TransactionResult` (the storage-based shape of `print_logs_structured`).

[adr-0002]: ../adr/0002-send-helpers-accept-aliases.md
[adr-0003]: ../adr/0003-aliases-on-context-and-result.md
