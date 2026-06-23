# Architecture overview: endpoint-agnostic anchor-litesvm + surfpool CPI rendering

**Status:** design doc. Records the system built on the `feat/surfpool` branches
(litesvm, anchor-litesvm, surfpool) and the decisions behind it. Companion to the
boundary doc `docs/design/litesvm-boundary.md` and the proof in
`docs/design/surfpool-cpi-tree-proof.md`.

## The organizing principle

One idea runs through everything: **the executor owns the vocabulary of an
execution; consumers name and render it.** litesvm runs the transaction, so it is
the only layer that witnesses the raw facts (the CPI nesting, the per-frame
privileges, the compute). It should *define and produce* those facts; the layers
above (surfpool, anchor-litesvm) should *interpret* them (aliases, links, color,
diagrams), not re-derive them.

This is not aesthetic. A fact captured once at the executor serves every consumer
at once; the same fact reconstructed in each consumer serves only that one. So the
design pushes facts down to litesvm and keeps naming up in the consumers. Most of
the decisions below are applications of this single split.

## The layers, bottom up

### litesvm: the shared seam

litesvm (the `cds-rs/litesvm` fork) owns two things this work leans on:

- `cpi_tree(logs: &[String]) -> Vec<CpiFrame>`: parses the flat Solana logs into
  the CPI invocation forest. The logs are the executor's artifact, so the parse
  belongs here. (A Dyck-language pushdown parser; `invoke` opens a frame,
  `success`/`failed` closes it.)
- `format_cpi_tree(header, frames)` and, the key addition, `format_cpi_tree_with(
  header, frames, program_label: &dyn Fn(&Address) -> String)`: a default ASCII
  renderer plus a **label hook**. litesvm owns the tree *structure*; the consumer
  injects the per-program *label* (an alias, a hyperlink). The default render is
  there for convenience; the hook is the augment seam.

  *Decision:* the renderer lives in litesvm (not just the parser) as a default,
  but naming is injected, not baked in. This is what later lets surfpool and
  anchor-litesvm both render the same frames with their own labels.

### surfpool: rendering + an augmentable output

surfpool wraps litesvm and serves it over JSON-RPC. Its `--no-tui` output now
renders each transaction as a CPI tree (replacing a flat log dump) by calling
`cpi_tree` + `format_cpi_tree_with` at the one event-loop site (`log_events`).

- **Encode by exception:** the render marks only the exception (failed frames);
  a clean deploy is a calm tree, a failed CPI is the one line that jumps out.
- **OSC 8 links:** the transaction header is a tty-gated hyperlink to Studio's
  `?t={sig}` view; per-account links wait on a Studio account-view URL (none
  exists yet; logged as a collaborative PR).
- **Client-augmentable aliases:** a new `surfnet_registerAlias(pubkey, name)`
  cheatcode lets *any* client name surfpool's output. See the round-trip below.

### anchor-litesvm: endpoint-agnostic testing

This is where most of the new architecture lives. The goal: the *same* scenario
runs against either an in-memory litesvm or a real surfnet over RPC, and renders
identically.

## The `ExecutionBackend` port

The seam that makes anchor-litesvm endpoint-agnostic. A trait (`litesvm-utils/
src/backend.rs`):

```
trait ExecutionBackend {
    fn send(&mut self, ixs, signers) -> ExecutionRecord;   // execute, return raw facts
    fn fund_sol / account_owner / get_account / deploy_program / capabilities;
    fn register_alias(&mut self, pubkey, name) {}          // default no-op
}
```

`ExecutionRecord` is the structured record: `logs`, `error`, `compute_units`,
`fee`, `message`, and `trace: Option<InstructionTrace>`. Two adapters:

- **`LiteSvmBackend`** (in-memory): wraps `LiteSVM` + the inspect-hook trace
  recorder. Full fidelity, `trace` is always `Some`.
- **`RpcBackend`** (behind feature `rpc`): a stock `solana_rpc_client::RpcClient`
  against a surfnet/cluster. `trace` is `None` (a stock RPC never witnessed the
  per-frame trace).

*Decision: the record's `trace` is the one endpoint-asymmetric field, and the
type makes that explicit.* The CPI tree is endpoint-independent (parsed from logs,
which both endpoints return). The per-frame privilege trace (signer/writable/owner
as presented to each frame, including `invoke_signed` PDAs) rides litesvm's
in-process inspect hook, which a stock RPC cannot see. A `Capabilities` flag
(`per_frame_trace`) lets a report annotate the degraded case rather than silently
emit a partial authority diagram. The asymmetry is not a defect to hide; it is the
spec for what a surfpool execution-record endpoint would later surface (v2).

*Decision: RpcBackend v1 is portable-degraded, not surfpool-coupled.* It works
against any cluster on the stock client (`trace` absent, said so in the report).
v1 `send` simulates for logs+compute then `send_and_confirm`s to persist state;
`fee` is `0` for now. The lossless path (pull the trace from surfpool) is a pure
addition later. Portable-first ships now and the registry work upgrades it without
touching the port.

## The bridge: `From<ExecutionRecord> for TransactionResult`

The decoupling move. `TransactionResult` is the rich type all of anchor-litesvm's
renderers (tree, mermaid, authority, ownership) consume; it builds a `CpiModel`
from logs (+ optional trace). A single `impl From<ExecutionRecord> for
TransactionResult` synthesizes the inner `TransactionMetadata` from a record, so
**any backend's output renders through the existing aliased renderers, unchanged.**

*Decision: bridge, don't fork the renderers.* `TransactionMetadata` is
`#[derive(Default)]` with public fields, so the record lifts in cleanly
(`inner_instructions` empty, the tree comes from logs; top `instruction` `None`).
This is what gives aliases on RPC output for free: the RPC record flows into the
same renderer the in-memory path uses.

## The `AnchorContext` rewire: the hybrid

`AnchorContext` is where scenarios live; its `send_*` methods needed to route
through the backend. The naive move (replace `pub svm: LiteSVM` with `Box<dyn
ExecutionBackend>`) was measured and rejected.

*Decision: hybrid, not full abstraction.* `pub svm` is used directly by ~175
dogfooder call sites (02-amm alone: 114) for setup and assertions
(`ctx.svm.create_token_mint(...)`). Removing it would shatter the cohort's
patterns. So: **keep `pub svm` (the in-memory path stays byte-identical, zero
churn, all tests green), and add an optional `backend: Option<Box<dyn
ExecutionBackend>>`** that `send_*` route through when set (`ctx.with_backend(...)`),
else the unchanged svm path. For the RPC backend `trace` is `None`, so the existing
`finish_send` trace-drain (empty) is correct, no restructuring needed. The
in-memory svm in an RPC context is a dead field; remote scenarios set up state
through the backend / cheatcodes instead. A wart, but a contained one, traded for
zero breakage.

## The alias round-trip into surfpool's own render

The symmetry that completes the picture. surfpool renders its *own* output; for it
to use the names a test knows, the test must push them.

- **Cheatcode:** `surfnet_registerAlias(pubkey, name)` emits a
  `SimnetEvent::AliasRegistered`; `log_events` accumulates a local alias map and
  feeds it to the `format_cpi_tree_with` label closure.

  *Decision: event-driven, not locker-threading.* The render runs in the cli's
  `log_events`, decoupled from the core SVM by a channel. Reading a core-stored
  alias map there would mean threading the `SurfnetSvmLocker` up through the cli
  (invasive). Emitting a `SimnetEvent` reuses the channel `log_events` already
  consumes, no new plumbing.

- **Client push:** `ExecutionBackend::register_alias` (default no-op; `RpcBackend`
  overrides to call the cheatcode via `RpcRequest::Custom`), and
  `AnchorContext::alias` forwards to the backend. So `ctx.alias(amm, "AMM")`
  against a surfnet makes surfpool's render print `Initialize … AMM`.

The result: **three renderers (anchor-litesvm's `Aliases`, surfpool's pushed map,
the in-memory path) name the same litesvm frames.** Structure lives once at the
executor; naming lives in each consumer. The reach thesis, fully expressed.

## Cross-cutting decisions

- **Dogfooders consume via path-deps, not git pins.** The turbin3-lineage dogfood
  repos were repointed to `path` deps on the local anchor-litesvm checkout, so they
  dogfood the unpushed `feat/surfpool` work with no premature push to the cohort
  remote. The compat-line repos (03-stake, web3-nft-marketplace) stay on
  `compat/anchor-0.31` with a backport obligation noted.
- **Adapters before abstraction.** The observer-registry prototype, the
  `ExecutionBackend`, and the alias cheatcode were all built adapter-first and
  proven live before being proposed for promotion. The vocabulary is discovered by
  dogfooding, not designed in the abstract.

## The fidelity contract (normative)

- CPI tree, per-frame compute, account deltas: available on **both** endpoints
  (logs + meta). Renderers depending only on these work everywhere.
- Per-frame privilege trace: in-memory always; over RPC only when surfpool
  surfaces it. The authority diagram degrades to a labeled note when absent;
  `capabilities().per_frame_trace` is the switch. No renderer silently emits a
  partial diagram.

## Open threads

- **Lifecycle-hook harness (Phase 3):** `before_all`/`before_each`/`after_*`, where
  `before_each`'s isolation is the real design content (cheap reset in-memory;
  namespacing on a shared surfnet). The next headline build; lets a whole dogfood
  suite run against surfpool.
- **RpcBackend v2 lossless:** surfpool surfacing the execution record (the trace)
  via a `surfnet_getExecutionRecord` endpoint, the collaborative seam with the
  surfpool maintainers, alongside the Studio per-account-view URL PR.
- **Legend / render configurability** (see the discussion that prompted this doc):
  surfpool's render has no legend; whether/how to make that configurable is open.
