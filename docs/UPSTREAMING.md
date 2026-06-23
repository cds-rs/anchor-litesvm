# Upstreaming ledger

The boundaries that graduated from our iteration and have been offered back to
the repos they belong in. This is the visible output of the fork-and-adapt
premise ([ADR 0000](adr/0000-the-premise-trait-boundaries-and-ecosystem-reach.md)):
we land a boundary in a fork to prove it in running code, then offer it upstream
as a working proposal for the maintainers to decide on. A fact that belongs at
the executor is offered to the executor; a rendering that improves a tool's DX
is offered to that tool. We never ask a source to adopt our surface API; we
offer it a fact at its source, or a self-contained improvement, and let the
maintainers weigh it.

Each entry records the PR, its link, the justification (why it belongs
upstream, not just in our fork), and its current status.

| PR | Repo | Status |
|---|---|---|
| [Add `--tree` flag to `solana logs`](https://github.com/anza-xyz/agave/pull/12755) | anza-xyz/agave | Open |
| [Add CPI invocation tree to `TransactionMetadata`](https://github.com/LiteSVM/litesvm/pull/349) | LiteSVM/litesvm | Open |
| [Invalid `inner_instructions` (out-of-bounds `programIdIndex`)](https://github.com/LiteSVM/litesvm/issues/213) | LiteSVM/litesvm | Root cause identified; fix proposed |
| surfpool: render the CPI tree at `--no-tui` | solana-foundation/surfpool | Not yet opened (gated on [litesvm#349](https://github.com/LiteSVM/litesvm/pull/349)) |

---

## agave: `--tree` flag for `solana logs`

- **PR:** [anza-xyz/agave#12755](https://github.com/anza-xyz/agave/pull/12755),
  "Add `--tree` flag to `solana logs` for nested CPI rendering"
- **Status:** Open.
- **Justification:** Not part of the testing ecosystem, but the same idea our
  CPI-tree work rests on, applied at the standard CLI: `solana logs` prints a
  flat stream, and the nested CPI structure is already implicit in the
  `invoke [N]` / `success` markers. A `--tree` flag renders that structure
  inline, a developer-experience improvement for anyone reading program logs,
  independent of how they test. It is offered as a self-contained CLI
  enhancement for the agave maintainers to decide on; it asks nothing of the
  runtime.

## litesvm: CPI invocation tree on `TransactionMetadata`

- **PR:** [LiteSVM/litesvm#349](https://github.com/LiteSVM/litesvm/pull/349),
  "Add CPI invocation tree to `TransactionMetadata`"
- **Status:** Open.
- **Justification:** The flagship application of
  [ADR 0001](adr/0001-executor-owns-the-execution-vocabulary.md): the executor
  is the only layer that can produce the CPI structure faithfully, so the
  parse belongs there, not re-derived in each consumer. Landing `cpi_tree` on
  `TransactionMetadata` means every downstream tool (surfpool, anchor-litesvm,
  an indexer) inherits structured CPI rather than re-parsing flat logs. Scope is
  deliberately narrow: the log-to-tree parser only, no aliasing, decoding, or
  rendering opinion (those stay in the consumers). This is the fork boundary we
  have been building on, offered back at its source.

## litesvm: invalid `inner_instructions` (the root cause behind a surfpool bug)

- **Issue:** [LiteSVM/litesvm#213](https://github.com/LiteSVM/litesvm/issues/213),
  "`send_transaction` is returning `TransactionMetadata` with invalid
  `inner_instructions`" (Open).
- **Downstream symptom:** [solana-foundation/surfpool#315](https://github.com/solana-foundation/surfpool/issues/315),
  "Inner instructions contains reference to account index that is out of bounds"
  (Closed).
- **Root-cause analysis and proposed fix:** [gist](https://gist.github.com/cds-amal/a2b4aa6c55aa8586121f7dd2f645cb3b),
  with a branch carrying the fix under a unit test
  (`cargo test -p litesvm renders_before_after_frames`).
- **Status:** Root cause identified, fix proposed; PR offered pending the
  maintainers' agreement on the shape, not yet opened.
- **Justification:** The surfpool bug (#315) was a symptom; the defect lives in
  litesvm (#213). `send_transaction` returns a `TransactionMetadata` whose
  `inner_instructions` carry *execution-relative* account indices that no longer
  map to the message's account keys once loader accounts are appended, so any
  consumer reading them hits an out-of-bounds `programIdIndex`. The proposed fix
  reconciles at the litesvm boundary (an anti-corruption adapter at the
  `TransactionMetadata` seam, leaving the vendored agave code untouched), so
  *every* consumer of `inner_instructions` is corrected at the source rather than
  each tool working around the symptom. This is
  [ADR 0001](adr/0001-executor-owns-the-execution-vocabulary.md) in reverse: a
  defect in an executor-owned fact is fixed at the executor, and surfpool (which
  could only close #315 by working around it) inherits the real fix.

## surfpool: render the CPI tree at `--no-tui`

- **PR:** Not yet opened.
- **Status:** Gated on [litesvm#349](https://github.com/LiteSVM/litesvm/pull/349)
  landing: this is the consumer that reads the executor-owned `cpi_tree`, so it
  waits for the fact to exist at the source before it can consume it.
- **Justification:** The consumer side of the litesvm `cpi_tree` boundary above.
  surfpool's `--no-tui` output renders each transaction as a structured CPI tree
  instead of a flat log dump, by consuming the executor-owned tree rather than
  re-deriving it. It is the proof that landing the fact at the executor pays off
  immediately in a second consumer, which is also why it cannot lead: the
  ordering (executor first, consumer second) is the premise
  ([ADR 0001](adr/0001-executor-owns-the-execution-vocabulary.md)) showing up in
  the upstreaming sequence itself.
