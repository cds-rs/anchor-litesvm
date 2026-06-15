# Account state diff: before/after snapshots for `Report` (needs a friction survey)

A `Report` (see [`report.rs`](../../crates/litesvm-utils/src/report.rs)) records a
scenario as prose + checks. What it can't yet do cheaply is show *what moved*:
the before/after state of the accounts a transaction touched. This proposal
sketches a layer for that, and records why the obvious "listen to the SVM"
shape doesn't pay off on the LTS toolchain.

## Where this came from

Dogfooding `Report` in `web3-nft-marketplace` (the BuyWithSol defect demo and
the AcceptTokenOffer settlement), the most valuable rows turned out to be a
hand-built before/after table:

```text
| item                | before               | after      |
| NFT owner           | listing escrow (PDA) | taker      |
| taker lamports      | 5000000000           | 4897955720 |
| taker payment-tokens| 0                    | 0          |
```

Two snags showed up immediately, and both are general:

1. **Reading a balance can panic.** An ATA the instruction *creates* is absent
   *before*; one it *closes* (an offer vault) is a drained husk *after*. The
   raw token reader unpacks unconditionally and panics on a non-token account.
   The stopgap is `token_balance_or(ctx, ata, default)`: guard on the owning
   program, fall back to a known initial. That works for token balances
   because the absent-state is definitionally `0`, not a guess.
2. **The caller hand-lists every account and reads it twice.** Fine for two
   tests; tedious and error-prone as a pattern.

This proposal is about (2): a reusable before/after diff. (1) is already solved
for the token case and is a constraint any diff layer inherits (never panic on
absent/closed/foreign accounts).

## Scope

In scope: a typed snapshot of a chosen set of accounts at an instant, a diff of
two snapshots, and a `Report`-friendly rendering of the diff. Out of scope: the
on-chain program, and anything that needs a real validator.

## The litesvm constraint (why "register a listener" is a dead end on 0.6)

litesvm 0.6.1 (the LTS pin) exposes no observation hook: grepping its surface,
the only relevant entry points are `get_account` (pull), `send_transaction`,
and `simulate_transaction`. There is no `subscribe` / `on_account_change` /
geyser-style callback. So "use the SVM as the source of truth via a listener
that catches each account's first change" is not a switch you flip; it's
infrastructure you build on top of `send_transaction`.

It also has an ambiguity even if you built it: an account created mid-test goes
from absent to *funded* inside a single transaction, so "first change" doesn't
cleanly mean "initial zero state." A snapshot taken at a chosen instant (right
before the action) has no such ambiguity. So the listener shape is both harder
*and* fuzzier than pull-based snapshots here.

**N.B.** Solana's static account model is what makes pull-based snapshots
complete: a transaction (including every CPI it makes) can only touch accounts
that appear in its message's account list. So "the set of accounts this tx
touched" is knowable *before* execution from the message keys; we don't need a
runtime listener to discover it.

## Candidate shapes, in order of coupling

### A. Explicit snapshot + diff (pull-based)

```rust,ignore
let before = Snapshot::of(&ctx, &[acct(vault, Token), acct(maker, Lamports), ...]);
ctx.send_ok(ix, &[signer]);
let after = Snapshot::of(&ctx, &[/* same set */]);
report.block("Assets exchanged", before.diff(&after).to_markdown());
```

`Snapshot::of` reads each account via `get_account` and decodes it per a
caller-declared kind (`Token`, `Lamports`, or a custom `Decode`). Absent /
closed / foreign accounts decode to a typed "absent" rather than panicking
(generalizing the `token_balance_or` lesson). Pros: unambiguous instant; zero
SVM coupling; trivial to reason about. Cons: the caller still lists the
accounts and remembers to snapshot twice.

### B. Send-wrapping auto-diff ("SVM as SOT", without a listener)

```rust,ignore
let (result, diff) = ctx.send_observed(ix, &[signer]);   // snapshots msg account keys
report.block("Assets exchanged", diff.to_markdown());
```

A wrapper around the send path snapshots every account in the transaction
message *before* execution and re-reads the same set *after*, emitting a diff.
This is the "source of truth" idea done with the API litesvm actually has.
Pros: no manual account list; before-state comes straight from the SVM. Cons:
needs a decode/render strategy per account type (so the engine stays
domain-agnostic, the way `Report` does via `ToMarkdown`); a diff over *all*
touched accounts is noisy unless filtered (sysvars, programs, the fee payer's
dust).

### C. First-change listener (the rejected one)

Record each account's first observed state across the whole test and diff
against it. Rejected for the LTS target: litesvm 0.6 has no hook (so it
collapses into B plus history retention), and "first change" is ambiguous as
above. If a future litesvm grows a real account-change callback, revisit; until
then B is strictly simpler and answers the same question.

## Recommendation

Build **A** first, because it's unambiguous and SVM-coupling-free, and it
directly retires the hand-rolled tables in the marketplace dogfood. Reach for
**B** only if the survey below shows the manual account list is the actual
friction. Skip **C** on litesvm 0.6.

Whichever ships:
- Lives in `litesvm-utils` next to `report.rs`, so both `AnchorContext` and a
  bare `LiteSVM` get it.
- Renders through the same alias resolution `Report` uses: pubkeys become role
  names, values are plain integers/enums, no base58 or timestamps, so committed
  reports stay byte-stable and diffable (the determinism contract in
  [`report.rs`](../../crates/litesvm-utils/src/report.rs)).
- Decoders are caller-supplied (a `Decode`/`ToMarkdown`-style trait), so the
  engine knows nothing about a consumer's account types, the way `Report`
  already stays domain-agnostic.

## Action: survey the friction first

Before building, look at how the snapshot pattern actually gets used as more
tests adopt `Report`:

- How many distinct accounts does a typical before/after touch? (If it's ~3-5,
  shape A's manual list is no burden and B's auto-diff is mostly noise to
  filter.)
- Do authors want *all* touched accounts, or a curated few? (Decides A vs B.)
- Which account kinds recur beyond token + lamports? (Decides the starter set
  of built-in decoders.)

That keeps this from over-fitting to the two marketplace tests that motivated
it. See [bundle-scaling.md](bundle-scaling.md) for the same "survey real usage
before picking a shape" discipline.
