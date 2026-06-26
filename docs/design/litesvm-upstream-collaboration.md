# LiteSVM upstream: the collaboration landscape

**Status:** design survey. Maps the upstream LiteSVM issue tracker against this
project's reach thesis, so the contributions we offer land as good-faith
collaboration rather than as critique. Companion to
[`litesvm-boundary.md`](litesvm-boundary.md) (the executor/consumer line) and
[`../UPSTREAMING.md`](../UPSTREAMING.md) (the concrete filings).

## Scope

In scope: the maintainer's documented posture on the public tracker, the gap
clusters that fit the thesis, the moves those clusters recommend, and a
self-audit of whether our own `cpi_tree` fork respects the line we ask upstream
to hold.

Out of scope: the contributions themselves (tracked in `UPSTREAMING.md`), and
the decision of whether to ship a runtime bump to the cohort (decided by the
surfpool line elsewhere).

## The thesis, stated up front

Two claims drive this survey:

1. **A richer Solana testing story comes from collaboration, not duplication.**
   LiteSVM, Mollusk, surfpool, Anchor, and codama each own a slice; the story is
   richer when they share authoritative sources than when each reimplements the
   others.
2. **Sources should define the types that cross boundaries.** When the executor
   (LiteSVM) defines the trace, the transaction metadata, and the instruction and
   error vocabularies that downstream tools consume, every tool inherits one
   correct definition; when it does not, each consumer reinvents (or forks) and
   they drift. This is the same "land facts at the executor" spine as the rest of
   the project (the IDL-as-lingua-franca work, the execution-observer registry).

The point of the survey: the upstream maintainer has already shown, in the
tracker, exactly which framing they accept and which they reject. We can aim our
contributions at the accepted framing.

## The maintainer's posture (the finding the rest rests on)

Reading how the maintainer and co-maintainers responded to past requests, a clear
and actionable pattern emerges. It is not "collaborative vs not"; it is a line
drawn in a specific place.

| Issue | Ask | Outcome | What it tells us |
|---|---|---|---|
| #198 | Adopt Mollusk's `Check` validation + low-level control | **Open**, pushed back | Rejects importing another tool's *surface API*: a special API "that does the same thing as `assert_eq!` for no clear benefit" is declined, to avoid proliferating such APIs. |
| #260 | Expose Agave's `iterate_vm_traces` so users stop forking LiteSVM | **Closed/completed** via PR #261 | Accepts *exposing a boundary*: custom trace handlers were added so the asymmetric-research tracing fork could be deleted and ride upstream. The framing offered was "reconcile efforts on a PR that suits both parties." |
| #197 | Build comprehensive Anchor integration into LiteSVM | **Closed**, deferred to external crates + codama clients | Avoids dependency hell and in-tree framework coupling; happy to point at external crates that consume LiteSVM cleanly. (This project lives in that thread.) |

So the rule the maintainer is enforcing, whether or not they would phrase it this
way: **LiteSVM should be a clean source with well-defined boundaries, not a host
for other tools' ergonomics.** That is not an obstacle to the thesis; it *is* the
thesis. The contributions that fit are the ones that make LiteSVM define the
boundary types better, so that Mollusk-style validation, this project's
observability, and surfpool's streaming all consume one source.

N.B. #260 is the precedent to cite in every future proposal: the case where "we
are forking you because you do not expose X" was answered with "here is X,
upstream, with a custom-handler hook." That is the move to repeat.

## Gap clusters

### A. Cross-boundary types (the thesis, directly)

- **#213 (open, from surfpool):** `send_transaction` returns
  `TransactionMetadata` whose `inner_instructions` carries a `programIdIndex` of
  13 when only indices 0..=12 are valid. This is the exact type our CPI-tree /
  trace recorder consumes, and the type surfpool consumes. A wrong field at the
  source is a wrong CPI tree in *every* downstream tool: the single best exhibit
  for "fix the type at the source, everyone inherits the fix."
- **#317 (open):** simulation output differs between the gRPC and RPC interfaces.
  Two interfaces over one engine that disagree: a boundary-consistency bug that
  bears directly on the surfpool reach proof (the gRPC/RPC seam).
- **#260 (closed):** trace exposure, covered above. The positive model.
- **#350 / #345 (open):** state-persistence snapshots to the JS SDK, and JS
  memory not released. The Rust-core / JS-binding boundary.

### B. Collaboration with other tooling teams

- **#198 (open):** Mollusk's `Check` + instruction-level testing. Do not re-pitch
  "adopt Mollusk's API" (already declined). Re-pitch as: expose the
  instruction-level execution + validation *hooks* so a Mollusk-style `Check`
  layer can be built *on* LiteSVM by whoever wants it (the #260 framing). The
  value users cite is instruction-level testing without the transaction/signer
  ceremony, not performance.
- **#197 (closed):** Anchor. Resolved toward external crates. Confirms our lane:
  the framework lives outside LiteSVM and consumes it.

### C. Runtime fidelity and freshness

- **SIMD-0312 / `create_account_allow_prefund` / System instruction 13:
  unraised** (confirmed by five empty tracker searches). Our ATA dogfood hits
  this: the program CPIs System ix 13, which the bundled `solana-system-program
  3.1.14` does not handle (it processes 0..=12). A clean, novel, reproducible gap
  to file, with the dogfood repro and the `system_processor.rs` evidence.
- **#338 (open):** automate mainnet feature-pubkey updates: the mechanism by
  which SIMD activations (like SIMD-0312) stay current. Relevant context for the
  filing: it is partly a freshness problem.
- **#196 (open):** rent-exemption logic only checked for empty accounts; runtime
  fidelity touching account-creation paths (ATA creation).
- **#305 (closed):** feature-gate account data not injected into the runtime.
  Related freshness/activation plumbing.

## Recommended moves

1. **File the SIMD-0312 gap** as a cluster-C runtime-freshness report: "the
   bundled system program predates System ix 13 (`create_account_allow_prefund`,
   SIMD-0312); programs that target it cannot be tested." Include the dogfood
   repro and the `system_processor.rs` evidence (handles 0..=12 only), and tie it
   to #338 (the freshness mechanism).
2. **Engage #213** (surfpool's metadata bug) as the flagship boundary-types
   exhibit: where "the source defines the type" is most visibly true, the
   reporter is a collaborator, and a fix improves every consumer at once.
3. **Reframe #198** away from "adopt Mollusk's API" toward "expose
   instruction-level execution + validation hooks," citing #260 as precedent. Do
   not ask LiteSVM to *be* Mollusk; ask it to expose the boundary so a
   `Check`-style layer can be built on top.
4. **Position the project's own work** (IDL-as-lingua-franca, the
   execution-observer registry, the trace-sourced renderers) as the consumer side
   of exactly this contract: LiteSVM defines the vocabulary; this project,
   surfpool, and codama clients speak it. The narrative is not "LiteSVM is
   uncollaborative"; it is "here is the boundary-defining contract LiteSVM is
   already half-enforcing (#260), and here is what the ecosystem builds once it is
   enforced consistently."

## Exhibit: the dual-harness user (#198)

The sharpest comment on #198 comes from a user who runs both litesvm and mollusk,
and it states the project's premise from the ergonomics side. Their team reaches
for mollusk to unit-test instructions (one or several) without signer or
transaction-API ceremony, which sped up their workflow; performance, they note,
is not the reason. Their argument for why instruction-level testing loses almost
nothing: a transaction is barely more than its instruction list. The whole delta
is signatures (derivable from the `is_signer` flags on the account metas), a
recent blockhash (always valid in a program test), and the account-keys table
plus signer count in the message header ("literally just a compressed version of
a list of uncompiled instructions").

The compression claim is correct, and it is the same relationship
`model::Transaction` is built on. The reductive step that follows ("so drop the
transaction") overreaches: a compression of X is not redundant with X. The thin
layer the message adds is small in bytes and dense in bugs and observability:

- the account-keys table is the index space where #213 lives (a `programIdIndex`
  out of range is a table-index bug that cannot even be expressed before the
  instruction list is compressed to a message);
- the signer set is derivable, but whether the runtime honors an account as
  signing authority across a CPI frame (including `invoke_signed` PDA derivation)
  is an execution fact the metas do not carry: the authority graph, which
  `Check`-style assertions have no vocabulary for;
- the blockhash is irrelevant on the happy path and decisive for the replay and
  expiry failure modes.

Measured by byte size the layer is ceremony; measured by where bugs and authority
facts live, it is the point.

This user is not an argument against the project; they are its target, describing
the gap from inside it. They run two harnesses for one job because neither alone
gives them instruction-level ergonomics and transaction-level observability
together. The vocabulary layer gives them both:

- **one spec, both engines.** The test is written once against `TestSVM` and runs
  on mollusk (the fast inner loop) and litesvm (the integration check) by
  swapping the backend, instead of being written twice in two vocabularies.
- **the ergonomics they already liked.** `send(&[Instruction], signers)` is
  instruction-group level: hand it the ixs, no `Transaction::new`, no blockhash
  fetch, no message compilation. The "no transaction-API shenanigans" feel is the
  default, not a separate mode.
- **observability on the fast engine.** Mollusk's `Check::*` gives
  resulting-account assertions but not execution shape. The same structured model
  renders on mollusk as on litesvm: the named CPI tree, the authority graph, the
  per-frame compute. The fast inner-loop engine stops being the blind one.
- **one assertion vocabulary, proven equal.** Instead of `Check::*` in one suite
  and hand-written `assert_eq!` after `send_transaction` in the other, the model
  and its renders are the assertions, and the conformance harness asserts they are
  byte-identical across engines (modulo declared capabilities). A green assertion
  on mollusk means what it means on litesvm.
- **the transaction-only question, answered without a second suite.** The benefit
  they grant transactions ("does it all fit, is it atomic") is the `atomic_send`
  capability: run the same spec on litesvm to get it, and read the flag to know
  which engine answers it. Mollusk declares `atomic_send: false` and chains
  multi-ix sends, so the harness says exactly when the transaction engine is
  needed rather than letting the difference pass silently.
- **a differential oracle for free.** Because one spec runs on both and the
  harness compares observability output, a divergence between mollusk's runtime
  and litesvm's on the user's own program surfaces as a conformance failure,
  rather than as two separately-green suites that secretly disagree.

Caveats worth stating: the two engines run different runtime versions (mollusk's
agave-4.0, litesvm's solana-3.x in this setup), so cross-engine equality is about
the render shape modulo declared capabilities and program-tracked compute
numbers, not bit-identical runtimes; and `send` still takes the signer set
(mollusk skips sigverify, an operational capability, but the keypairs are still
named). Neither changes the shape of the help: write once, run on the fast engine
and the integration engine, and get on both the observability that `Check` cannot
express.

## The fork as evidence

The pattern in the tracker is that teams fork LiteSVM when it does not expose or
support what they need:

- an asymmetric-research team forked it for VM tracing, the fork that #260
  eventually made unnecessary by exposing custom handlers upstream;
- this project forked it (cds-rs) to add `cpi_tree`: structure that LiteSVM's own
  logs only expose as strings;
- a further fork would be needed to unblock the ATA create path, because the
  bundled system program predates System ix 13 (SIMD-0312).

That is three forks chasing capabilities a single upstream source could define
once. The argument is not "LiteSVM is bad"; it is the opposite: LiteSVM is
critical enough that the whole ecosystem builds on it, and *because* it is
critical, the cost of it not engaging the tooling community is paid in forks,
duplicated maintenance, and drift. #260 is the proof that the better equilibrium
exists: absorb the fork upstream by exposing the boundary, and everyone
downstream stops maintaining their own copy. The case is that critical
infrastructure should treat that absorption as a first-class responsibility.

## Self-audit: does `cpi_tree` respect the boundary line?

We hold the type-boundary thesis, so we owe it of ourselves: does the `cpi_tree`
contribution in our fork respect the same line we ask the maintainer to hold?
The verdict is yes, and it is a positive exemplar:

1. **It defines a structured type for LiteSVM's own output, at the source.**
   Before `cpi_tree`, the CPI structure of a transaction was available only as
   `TransactionMetadata.logs: Vec<String>`, stringly typed; every consumer had to
   re-parse those strings to recover the tree. `cpi_tree(logs) -> Vec<CpiFrame>`
   makes LiteSVM the source that defines the `CpiFrame` type for its own output.
   That is the thesis in the *good* direction (contrast #213, where a boundary
   type is defined wrong at the source and every consumer inherits the bug).
2. **The extension point is a callback, not a dependency.** The one place a
   consumer injects knowledge LiteSVM does not have (program names) is a function
   argument:
   ```rust
   pub fn format_cpi_tree_with(
       header: &str,
       frames: &[CpiFrame],
       program_label: &dyn Fn(&Address) -> String,  // the consumer augments the label
   ) -> String
   ```
   LiteSVM stays ignorant of aliases, IDLs, and decoders; the consumer passes its
   alias lookup *in*. This is the #260 pattern ("supply custom handlers") the
   maintainer accepted, and the inverse of the #198 pattern (importing another
   tool's surface API) the maintainer declined. The argument is a pure function,
   so the dependency points the right way (caller to LiteSVM, never the reverse).
3. **It does not change the boundary type's shape.** The additions to
   `TransactionMetadata` are *methods* (`cpi_tree(&self)`, `pretty_cpi_tree(&self)`)
   derived on demand from the existing `logs` field; no new fields, no change to
   the serialized wire shape. So it carries none of the #213-style risk.

Caveats worth disclosing:

- The default renderer (`pretty_cpi_tree`) is more surface than the strict
  parser. The minimal ask is `cpi_tree()` (the type-defining parser) plus the
  `format_cpi_tree_with` hook; the default render is convenience and can move
  downstream.
- The test-fixture `build.rs` shells out to `cargo build-sbf`, adding a
  build-time toolchain dependency. Gate it behind a test feature or commit the
  fixtures, so a plain `cargo build` of LiteSVM does not require the SBF
  toolchain.

## The debugging layer (the semantic layer above the steppers)

The SBPF debugging landscape, as it stands: four independent efforts, all at the
register/assembly level, none sharing a vocabulary with each other or with the
testing layer above.

- **anza-xyz/sbpf**: the official VM crate (JIT, verification, execution
  tracing); carries the `debug_port` on the invoke context.
- **mollusk** (`sbpf-debugger` feature): fronts Agave's debug port, persists ELF
  SHA-256 to symbol mappings for a debugger client, plus register traces with
  disassembly.
- **blueshift-gg/sbpf**: an assembly toolchain with an interactive REPL.
- **sbpf-coverage**: coverage + trace disassembly, instruction to source mapping.

The reading: this is Ethereum's first act replaying. The EVM had opcode-level
steppers early (the Remix debugger, geth tracers); the lost decade was the
*semantic* layer above them, unspecified until ethdebug was designed
retroactively. Solana is fragmenting at the register level right now, observable
in four repos. The testing vocabulary's facts (frames, per-frame privilege
traces, account deltas, declaration-derived names) are the semantic layer those
debuggers would otherwise each reinvent: the debugger is just another consumer of
the executor's facts, the reach thesis extended one layer up.
