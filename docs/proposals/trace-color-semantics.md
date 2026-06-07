# Color semantics for execution-trace rendering (reserve color for state, not alarm)

The mermaid renderer used to wrap every failed frame in a pale-red `rect`
region. Reviewer feedback on a real diagram (the nft-staking restake question,
2026-06-01) was blunt and correct: the regions dominate the diagram, read as
"everything in here is broken", and pull the eye away from the one edge that
actually failed. Commit `75d7bfa` (HEAD) / `ca2c2e3` (compat) removed them; the
`✗` notes and `--x` arrows are now the only failure markers.

That was the subtractive half. This proposal records the constructive half of
the same feedback: when color comes back, what should it mean?

## The principle

Color should encode *what kind of thing happened*, not emphasis. The proposed
vocabulary, from the feedback verbatim:

| Color | Meaning |
|---|---|
| Green | account mutation (state actually changed) |
| Blue | CPI (control crossed a program boundary) |
| Orange | compute-heavy operation (above some threshold) |
| Red | terminal failure, at the **origin frame only** |

Red appears exactly where the error originates, never on the propagation chain:
a parent that fails because its child failed is a consequence, not a cause, and
painting it red double-counts the alarm. The current renderers (post-`75d7bfa`)
still mark every failed edge with `✗`/`--x`; distinguishing origin from
propagation is worthwhile even before any color lands (e.g. origin gets the
error text, ancestors get a bare `✗`).

## Where it would apply

Three render targets, in increasing order of freedom:

1. **The structured tree (`style.rs`)**: already has ANSI color plumbing behind
   `ANCHOR_LITESVM_COLOR`. The cheapest place to pilot the vocabulary; the
   constraint is that terminal color is per-line, which fits per-frame
   semantics well.
2. **The mermaid renderer (`mermaid.rs`)**: the most constrained target.
   Mermaid sequence diagrams have no first-class per-arrow color; the available
   knobs are `rect` regions (just removed for failures, and the same blob
   problem would apply to any color), notes, themes, and participant styling.
   Realistically, mermaid gets the *vocabulary applied to glyphs*, not color:
   distinct markers for mutation/CPI/heavy/failure-origin.
3. **A purpose-built trace viewer** (the real target of the feedback): owns its
   renderer, so the full color vocabulary applies. Out of scope for this crate
   today; this proposal exists so the semantics are already settled if/when
   that viewer happens.

## Constraints and open questions

1. **Green (mutation) needs before/after account state.** The CPI tree is
   parsed from logs, and logs don't say which accounts changed. Detecting
   mutation requires snapshotting accounts around the transaction, which is
   exactly what the [account state diff proposal](account-state-diff.md)
   builds. Green is blocked on (or rather, composes with) that work.
2. **Orange (compute-heavy) needs a threshold definition.** Absolute (>50k CU)?
   Relative to the transaction's budget? Relative to sibling frames? A
   percentile over the suite's history? The feedback doesn't say, and the right
   answer probably differs between "spot the hot CPI in this tx" (relative) and
   "this instruction regressed" (historical). Needs a decision before
   implementation.
3. **Blue (CPI) is almost free.** Every non-root frame in the tree IS a CPI;
   the information is already there. The only question is whether marking
   every CPI carries information or just adds ink (in a tree where everything
   below depth 1 is blue, blue means nothing). Possibly blue should mark only
   *cross-program* edges where the callee is not a well-known builtin.
4. **Origin detection is already possible.** The deepest frame with a `Failed`
   outcome whose children all succeeded is the origin; `cpi_tree` has the
   structure to compute this today. This piece doesn't need to wait for any
   color decision.

## Smallest useful first step

Origin-vs-propagation distinction in the existing renderers, no color at all:
the origin frame's marker carries the error text (`✗ PluginAlreadyExists`),
ancestor failures render as bare `✗`. That implements the heart of the
feedback ("the eye should land on the root cause first") inside the current
no-color design, and it's where any later color work would attach.
