# Transaction CPI rendering: design

## Scope

This doc covers the family of renderers that turn a single transaction's
execution into a human-readable view of its cross-program-invocation (CPI)
structure. There are four views today, and they all sit on one shared model:

- an annotated box-drawing **tree** (`print_logs_structured()`),
- a Mermaid **sequence diagram** (`print_mermaid()` / `..._with_lifelines()`),
- an **authority graph** (`print_authority_graph()`: who signs what, who
  writes what),
- an **ownership graph** (`print_ownership_graph(&svm)`: who owns the accounts
  that were written).

Two further renderers work at the *test* altitude rather than per transaction
(the authority *flow* and the account *index*); they read the same model and are
covered under [Per-test renderers](#per-test-renderers-authority-flow-and-account-index).

It also covers the layers that compose around the renderers: the `Aliases`
table, signer extraction, the captured top-level instruction, and the
`send_ok` / `send_err` / `send_err_named` integration that drives the
failure-path print.

Out of scope: the derive macros (`BundledPubkeys` and friends), the `Report`
test-output helpers, and the SVM setup helpers. They are documented with their
own code.

This started life as a single "structured logs" tree renderer. It grew a
sequence-diagram view, then two graph views, and the shared machinery was
factored out underneath them; the content here describes where it landed.

Definitions, used throughout:

- **Frame**: one invocation of a program. A top-level instruction opens a root
  frame; each CPI opens a nested child frame.
- **Alias**: a `(Pubkey, name)` substitution applied at render time, so a tree
  reads `voting::CastVote` instead of a base58 smear.
- **Legend**: the tree footer listing `(name, full_pubkey)` for every
  user alias that actually appeared (well-known program names filtered out).
- **Authority**: a required signer. **Owner**: the program an account's
  `Account.owner` field points at (the program allowed to mutate it).

## The shape: one model, a family of renderers

The dataflow is a straight line with one fan-out at the end:

```text
litesvm  ──►  litesvm::cpi_tree  ──►  model.rs   ──►  renderer.rs  ──►  ┌─ tree.rs       (box-drawing tree)
              (the "tree-api":        (CpiModel:       (the Renderer    ├─ mermaid.rs    (sequenceDiagram)
               structural parse        resolve names,   port + shared   ├─ authority.rs  (signs / writes)
               of the log stream)      errors, accounts) services)      └─ ownership.rs  (owns)
```

The important property: a renderer reads the `CpiModel` and **nothing else**.
It does not re-parse logs, re-thread inner instructions, or re-decode anything.
That single source of truth is the whole point of the split.

The model itself has two upstream inputs, not one: the `cpi_tree` log parse
(structure, outcomes, compute) and a runtime **trace** (per-frame account
privileges, recorded on litesvm's `invocation-inspect-callback`). `model.rs`
unifies them before any renderer runs, so "single source of truth" holds
downstream of the model even though two sources feed it. The trace exists because
the message header sees only top-level privileges, so an `invoke_signed` PDA is
invisible to it; [`litesvm-boundary.md`](litesvm-boundary.md) is the why and the
where-it-should-live.

It earns its keep because the work the renderers share is exactly the work
that is easy to get subtly wrong: walking the CPI tree, matching inner
instructions to frames in DFS pre-order, decoding instruction names from
discriminators, lifting Anchor error names out of the logs. When the tree and
the sequence renderer each did all of that independently, a change like
"surface the Anchor error name instead of the raw code" had to be made (and
kept in sync) in two places. Pulling it into `model.rs` made it one place, and
made each new view (the two graphs) a pure additive `Renderer` impl with no
new plumbing.

So the layering is hexagonal in the ports-and-adapters sense: `model.rs` is the
domain (the resolved CPI model), `renderer.rs` is the port (the `Renderer`
trait the views implement), and `tree` / `mermaid` / `authority` / `ownership`
are adapters. `transaction.rs` shrinks to "build the model once, pick a
renderer, hand back its string."

## The tree-api boundary: `litesvm::cpi_tree`

Source files: `crates/litesvm-utils/src/transaction/model.rs`.

The structural parse (turning the flat `Program X invoke [n]` /
`Program X success|failed` log stream into a nested tree, classifying
outcomes, attributing logs and compute units to frames) lives upstream in
`litesvm::cpi_tree`. We call `cpi_tree(logs)` and get back a `Vec<CpiFrame>`:

```rust,ignore
// litesvm::cpi_tree (upstream)
pub struct CpiFrame {
    pub program_id: Pubkey,
    pub outcome: CpiOutcome,                 // Success | Failed { message } | Truncated
    pub compute_units: Option<ComputeUnits>,
    pub instruction_name: Option<String>,    // from "Program log: Instruction: <Name>"
    pub logs: Vec<FrameLog>,                  // Msg / Data tokens for this frame
    pub children: Vec<CpiFrame>,
}
```

**Remark (an abandoned avenue).** This boundary used to live inside this crate:
an in-repo parser folded the log stream into a flat `Vec<Event>` of
`Open` / `Close` brackets via a `to_events()` pass, and a `fmt_events()` pass
rendered it. We argued at the time that a flat event stream beat a tree because
"logs are inherently flat." It worked, but it was a second copy of a fiddly
parser that Solana tooling already needs everywhere, so it was upstreamed into
`litesvm::cpi_tree` and deleted here. The model below is a genuine tree now,
not a flat stream; the old rationale did not survive contact with having four
renderers that all want to recurse.

## The model (`model.rs`)

`model::build(...)` takes the raw pieces of a `TransactionResult` and produces
a `CpiModel`. This is the transformation every renderer shares.

```rust,ignore
pub(super) struct CpiModel {
    pub header: Option<Header>,   // top-level instruction, for a renderer's header line
    pub roots: Vec<Root>,         // the CPI forest, one entry per top-level instruction
    pub tx_signers: Vec<Pubkey>,  // required signers, account_keys order (fee payer first)
    pub error: Option<String>,    // tx-level error string, if the send failed
    pub compute_units: u64,       // consumed by this run
    pub fee: u64,                 // lamports
}

pub(super) struct Root { pub signers: Vec<Pubkey>, pub frame: ResolvedFrame }

pub(super) struct ResolvedFrame {
    pub program: Pubkey,
    pub instruction_name: Option<String>,  // resolved: log-derived OR discriminator-decoded
    pub outcome: Outcome,                  // Failed message already resolved to the Anchor name
    pub compute_units: Option<u64>,        // consumed; None when the frame emitted no cu line
    pub accounts: Vec<AccountRef>,         // the accounts this instruction touched
    pub logs: Vec<FrameLog>,               // passed through, for renderers that surface events
    pub children: Vec<ResolvedFrame>,
}

pub(super) enum Outcome { Success, Failed { message: Option<String> }, Truncated }

pub(super) struct AccountRef {
    pub pubkey: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
    pub owner: Option<Pubkey>,   // None until fill_owners runs (see "Two stopgaps")
}
```

The point of `build` is that everything a renderer could want is resolved
here, once:

| Resolved in the model | How |
|---|---|
| Instruction name | The upstream log-derived name, else a discriminator decode for well-known programs (System, SPL Token, ATA). See below. |
| Failure message | The Anchor `Error Code: <Name>` lifted out of the frame logs (`extract_anchor_error_name`), falling back to the runtime message. So `Failed.message` is already `EscrowExpired`, not `custom program error: 0x1770`. |
| Compute units | `ComputeUnits::consumed`, or `None` (native programs emit no `consumed N of M` line; the tree renders that absence as `(no cu)` rather than dropping it silently). |
| Accounts + roles | Reconstructed from the message and inner instructions by `build`, then overwritten per frame from the runtime trace (`fill_from_trace`), the only source that sees inner-frame privileges. See "Two stopgaps". |
| Header | The top-level instruction's program + decoded name, for the section header. |

### Two sources for the instruction name

`ResolvedFrame.instruction_name` has two populators, and the order matters:

1. **`decode_instruction(program_id, data)`** matches the first 1-8 bytes of
   the instruction data against a small per-program discriminator table
   (System's 4-byte LE tag, SPL Token's 1-byte tag, ATA's `Create` variants).
   It fires for CPI children (depth > 1), where the matching inner instruction
   supplies the data bytes.
2. **`Program log: Instruction: <Name>`** is Anchor's generated dispatcher's
   "the handler starts here" line, which `litesvm::cpi_tree` already attaches
   to the frame as `instruction_name`.

The decode wins when both are available (`frame.instruction_name.clone().or(decoded)`
reads "keep the log-derived name, else the decoded one"; for the well-known
programs the log name is usually absent, so the decode fills it). The
discriminator is the canonical handler identifier; the Anchor log line is the
catch-all that names user programs without any IDL or per-program
registration.

**N.B. (the iter-threading invariant).** Matching inner instructions to frames
is positional: Solana emits `inner_instructions` in DFS pre-order per root, and
`resolve_frame` advances one inner instruction per CPI child as it descends.
The advance has to happen for every child even when we do not use its decoded
name, or the iterator desyncs from later siblings. Root frames (depth 1) do not
pull from this iterator; their name comes straight from the frame.

### The header

For single-instruction sends the model carries a `Header { program,
instruction_name }`, decoded from the captured `InstructionInfo` (the program
id and data of the one instruction the caller issued). Batches (multi-ix sends)
carry no header; the tree's `Transaction  signers=[...]` line leads instead.

## The renderer port (`renderer.rs`)

```rust,ignore
pub(super) trait Renderer {
    fn render(&self, model: &CpiModel, aliases: &Aliases) -> String;
}
```

A renderer takes the resolved model plus the alias table and returns a complete
string in its own format, owning its own framing (header, footer, fences). The
caller (`transaction.rs`) builds the model, constructs the renderer, and prints
what it returns.

Two services live here because more than one adapter needs them:

- **`LegendCollector`** wraps the `&Aliases` for a render pass and records, in
  first-appearance order, each `(name, Pubkey)` pair that actually resolved.
  An adapter resolves every pubkey through the collector, so a legend reflects
  only the aliases that appeared in *this* render. It lives in the port (not in
  one adapter) because each adapter resolves pubkeys in its own traversal
  order, and the legend ordering has to follow that order.
- **`NodeIds` + `node_label`** are for the two flowchart graphs. A Mermaid node
  id must be a bare identifier; two distinct names that sanitize to the same id
  (`a.b` and `a-b` both `a_b`) would silently merge into one node. `NodeIds`
  hands out unique ids (first claimant keeps the sanitized form; later
  collisions get a `_2`, `_3` suffix), and `node_label` keeps the readable name
  bare when it is already safe, quoting it otherwise. This is the flowchart
  counterpart to the sequence renderer's `participant Id as "name"` trick.

## The adapters

### Tree (`tree.rs`)

`TreeRenderer` owns its whole frame: the `── program::ix ──` section header,
the box-drawing body, and the `Compute Units / Fee / Legend` footer.

```text
── voting::CastVote ─────────────────────────────────────────
Transaction  signers=[alice]
└── voting ✓ 12340cu  signer=alice
    ├── System::CreateAccount ✓ (no cu)
    └── Token::TransferChecked ✓ 5670cu
Compute Units (this run): 19240
Fee: 5000 lamports
Legend (1):
  alice = 7xKXt…J9aB
```

Notes that are easy to misread:

- The single-line `── program::ix ──` opener fills to a fixed width with `─` so
  it reads as a section break, not a label. Batches omit it.
- `signer=X` is annotated on **top-level frames only**. A CPI by definition
  inherits the caller's authority; repeating signers on every child is noise.
  N.B. `signer=X` means "X is a required signer referenced in this
  instruction's accounts", not "X authorized this specific call".
- `(no cu)` is surfaced explicitly so a reader does not mistake a native
  program's missing cu line for a parser drop.
- `(this run)` on the footer is a reminder that per-frame cu drifts across runs
  (Anchor's `find_program_address` iterates a variable number of bumps for
  different random pubkeys); do not diff the number across runs and conclude
  there is a regression.
- The legend lists only user aliases (`is_well_known_name` filters out System,
  Token, and friends), in first-appearance order. A user who *renames* a
  well-known program does see it in the legend: the filter checks the name, not
  the pubkey.

`fmt_tree` (the body, minus header/footer) is exposed so the body tests can
assert the tree shape without the framing.

### Mermaid (`mermaid.rs`)

`MermaidRenderer` emits a fenced ```mermaid `sequenceDiagram`. Two modes:

- **Plain**: one `->>` arrow per CPI edge, fire-and-forget. Failed frames get a
  trailing `note over <target>: ✗ <msg>`. Compact; good for "what got called".
- **Lifelines**: paired `->>+` (call, activate) and `-->>-` (return,
  deactivate) arrows, so the synchronous "parent stays active while children
  run" nesting is visible. The return arrow carries `ok (Ncu)` or the error.

Events (`Program data:`) and, opt-in via `ANCHOR_LITESVM_MERMAID_LOGS`, logs
(`Program log:`) surface as informational dashed arrows back to the tx
initiator. Children render before the parent's return line, because Solana runs
the inner CPIs before the parent's post-CPI check fires; splitting the call and
return lines is what keeps that chronology honest.

### Authority graph (`authority.rs`)

`AuthorityGraph` emits a Mermaid `flowchart`:

```text
signer --signs--> program --writes--> account
```

It reads `ResolvedFrame.accounts`. For each frame: a signer account draws a
`signs` edge to the program; a writable non-signer account draws a `writes`
edge from the program. Read-only non-signer accounts are dropped, to keep the
graph about authority and state change. Nodes carry a role with precedence
(`program > signer > writable`), so a pubkey seen in more than one role is
drawn once at its highest. Edges dedup across frames and descend through CPI
children.

N.B. the same caveat as the tree's `signer=`: "signs" is the account-list
relationship (X is a required signer referenced by an instruction to P), not a
claim about intent. A writable signer (a fee payer) renders as a signer; its
writability is left implicit.

### Ownership graph (`ownership.rs`)

`OwnershipGraph` emits a Mermaid `flowchart` of which program owns each
**written** account:

```text
owner-program --owns--> account
```

The owner usually differs from the writer, and that gap is the point. When an
Escrow program CPIs into the Token program to write a token account, the writer
is Token but the owner is Token; when the AssociatedToken program creates an
account, the System program does the `CreateAccount` (the *writer*) while the
account ends up owned by Token (the *owner*). The authority graph shows the
writer; the ownership graph shows the owner; the difference is invisible from
the logs alone.

Which is exactly why ownership needs help the others do not (see below).

### Per-test renderers: authority flow and account index

The four views above each render a *single* `TransactionResult`. Two more
renderers work at the *test* altitude, accumulating across every send in a
scenario, and read the same trace-fed model:

- **`authority_story.rs`** renders the authority *flow* as a Mermaid
  `sequenceDiagram`, one section per transaction: who signed, and which transfers
  the program signed as a PDA via `invoke_signed`. It is the view the per-tx
  mermaid (call structure) structurally cannot draw.
- **`account_index.rs`** renders the account *census*: every account the test
  touched, classified by owner program and authority class, with ATA parent
  edges recovered by reverse-derivation.

`AnchorContext` surfaces them as `ctx.authority_story()` / `ctx.account_index()`,
bundled by `report_execution`. Both depend on the runtime trace
([`litesvm-boundary.md`](litesvm-boundary.md)): the message header alone cannot
say who signed an inner frame. The book's Part IV covers the user-facing surface.

## Two stopgaps (the litesvm ask)

The graphs lean on data that `litesvm::cpi_tree`'s `CpiFrame` does not carry.
Per-frame privileges come from a runtime trace (a `TraceRecorder` riding
litesvm's `invocation-inspect-callback`), which overwrites the weaker `Message`
reconstruction; the final account owner comes from a post-execution lookup. Both
are documented as stopgaps in `model.rs`. The ownership argument (why these
belong on the frame, and the migration that deletes the reconstruction) is
[`litesvm-boundary.md`](litesvm-boundary.md).

| Data | Where we get it now | The ask |
|---|---|---|
| **Per-frame privileges** (`AccountRef`: `is_signer` / `is_writable` / `owner` as presented to the frame) | The runtime trace (`fill_from_trace`), riding the inspect hook; `build` fills a `Message` + inner-instruction fallback first, which knows top-level privileges only. | Carry the account list + privileges on `CpiFrame`. Then both the trace recorder and the message reconstruction go away. |
| **Account owner** (`AccountRef.owner`) | A post-execution `svm.get_account(pk).owner` lookup, one per account. `build` leaves `owner = None`; `model::fill_owners(model, closure)` fills it, and the closure (`\|pk\| svm.get_account(pk).map(\|a\| a.owner)`) keeps `model.rs` free of any svm dependency. | Carry the owner on the frame. Then `build` fills it and `fill_owners` goes away. |

This is why `ownership_graph_string` takes `&LiteSVM` and the other render
methods do not: ownership is the one view that needs the second lookup. The
`account_graphs` example (`cargo run -p anchor-litesvm --example account_graphs`)
renders all four views for a real ATA creation and shows the owner-vs-writer
gap concretely; it is the artifact for the litesvm conversation.

## The composable layers

The renderer alone is not a useful test log. A few concerns plug in around it.

### Aliases live on the context and on the result

The alias table is per-test data (the actors a test built), not ambient. The
shape we landed on:

- `TransactionResult` holds a private `aliases: Option<Aliases>`, set by
  `with_aliases(self, Aliases) -> Self`. `print_logs_structured()` /
  `logs_structured_string()` and the graph methods read from it, falling back
  to `Aliases::default()` (well-known programs only).
- The bare `TransactionHelpers` trait on `LiteSVM` still takes `aliases: &Aliases`
  per `send_*` call, and stashes it on the returned result via
  `with_aliases(aliases.clone())` (a cheap two-small-HashMap clone), so a
  chained `.print_logs_structured()` reads through without re-threading.
- `AnchorContext` owns an `aliases: Aliases` field, extended with
  `ctx.alias(pk, "name")`, and its `send_ok` / `send_err` / `send_err_named`
  read `&self.aliases` and forward to the bare trait. No `&Aliases` at the
  Anchor call site at all.

**Remark (an abandoned avenue).** The first cut threaded `&Aliases` explicitly
through every `send_*` and `print_logs_structured` call, on the reasoning that
an external table per call is the most predictable surface and "context-bound
aliases would force tests to remember reset semantics." Two dogfood suites then
independently built a per-scenario type that *owns* one alias table and threads
`self.aliases` into every call, and both reinvented the same
`std::mem::take`-based workaround for `Aliases::with`'s consuming-builder
signature. When two callers reach for the same dance, the API did not fit. So
the table moved onto `AnchorContext` (one table per scenario is the real
shape), `Aliases::add(&mut self, ..)` was added as the in-place accumulation
companion to the consuming `with`, and the per-call parameter was dropped
everywhere except the bare-LiteSVM trait (the only surface non-Anchor users
have). The escape hatch survives:
`result.with_aliases(other_table).print_logs_structured()` still gives you a
different table per print.

### Signers, and the captured instruction

- `signers::extract(&message)` produces a `SignerInfo { tx_signers, per_root }`:
  the required signers in account-keys order (fee payer first), and per
  top-level instruction the subset of required signers its accounts reference.
  Drives `signers=[...]` and `signer=X`.
- `InstructionInfo { program_id, data: Box<[u8]> }` is captured in
  `send_instruction` (before the `Instruction` is consumed into a
  `Transaction`) and carried on `TransactionResult` for single-ix sends. It
  stays minimal: the account metas the runtime `Instruction` also carries are
  recoverable from the `Message`, so cloning the whole thing would be
  redundant. `data` is the full bytes (we only read the first 1-8, but copying
  a few hundred bytes beats lifetime-tracked slices).

### The fluent chain

Every chainable method on `TransactionResult` (`assert_success`,
`assert_failure`, `print_logs`, `print_logs_structured`, `print_mermaid*`,
`print_authority_graph`, `print_ownership_graph`, `tap`) takes `self` and
returns `Self`, so a chain ends in an owned binding the caller keeps:

```rust,ignore
let result = svm.send_ok(ix, &[&payer], &aliases)
    .print_logs_structured()              // alias table stashed by send_ok
    .assert_success();
```

Read-only methods (`compute_units`, `is_success`, `error`, `fee`, `inner`, ...)
stay `&self -> T` and compose into a chain through `tap`, which borrows the
result for a closure and hands ownership back:

```rust,ignore
svm.send_ok(ix, &[&payer], &aliases)
    .tap(|r| println!("CU used: {}", r.compute_units()))
    .assert_success_with(|r| r.compute_units() < 100_000);
```

`assert_success_with` / `assert_failure_with` fold "outcome holds AND predicate
holds" into one step (the `_with` suffix follows `Vec::with_capacity`'s
convention). `print_markdown_pair` is a convenience that wraps the tree in a
```console fence and the lifelines diagram in a `<details>` block, so
`cargo test -- --nocapture` output drops straight into a markdown file.

## References

- Code: `crates/litesvm-utils/src/transaction/` (`model.rs`, `renderer.rs`,
  `tree.rs`, `mermaid.rs`, `authority.rs`, `ownership.rs`, `aliases.rs`,
  `signers.rs`, and `transaction.rs` for the public surface).
- Demo: `crates/anchor-litesvm/examples/account_graphs.rs`.
- Public API on `TransactionResult`: `print_logs_structured` /
  `logs_structured_string`, `print_mermaid` / `mermaid_string` /
  `..._with_lifelines`, `print_markdown_pair`, `print_authority_graph` /
  `authority_graph_string`, `print_ownership_graph(&svm)` /
  `ownership_graph_string(&svm)`, and the `TransactionHelpers` /
  `AnchorContext` `send_*` families.
