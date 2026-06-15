# Per-ix bundles + per-field opt-out (needs scale survey)

The `Bundle` model assumes one union across every ix. Fine for small programs (the escrow dogfood is 3 ixs / 10 unique accounts); suspected to not scale, but unverified.

## Action: survey real Anchor programs first

Before picking a design, sample 5-10 deployed programs (Jupiter, Drift, Marinade, Phoenix, Squads, Solend, ...) and measure:

- Instructions per program.
- Distinct accounts across all `#[derive(Accounts)]` structs (= union bundle field count).
- Accounts per ix (mean, max).
- Overlap between ixs (how much would per-ix bundles duplicate?).

Decides whether the union actually breaks in practice and which shape below is worth building.

## Candidate shapes, in order of macro complexity

### A. `#[bundle(skip)]` on Accounts fields

Opt-out per field; macro emits a `build_ix_with(bundle, overrides, args)` variant. Use case: spoofing tests where the value should vary, not live on the bundle.

### B. Per-ix bundles

Each `BundledPubkeys` derive names its own bundle. Smaller, program-shaped bundles; cross-ix tests populate multiple with overlapping data.

### C. Embedded core (`#[bundle(embed)]`)

Per-ix bundles compose a shared core. Cleanest, hardest to implement.

### D. Per-ix bundle traits

Each `BundledPubkeys` derive emits a trait, not a struct. The test author defines a fixture and impls one trait per ix the fixture supports; "union" becomes trait composition.

```rust,ignore
// macro emits, per ix:
trait InitPollBundle { fn auth(&self) -> Pubkey; fn poll(&self) -> Pubkey; }
trait CastVoteBundle { fn poll(&self) -> Pubkey; fn voter(&self) -> Pubkey; }
impl<B: InitPollBundle> BuildableIx<B> for instruction::InitPoll { /* ... */ }

// test author:
struct Fixture { auth: Pubkey, poll: Pubkey, voter: Pubkey }
impl InitPollBundle for Fixture { /* getters */ }
impl CastVoteBundle for Fixture { /* getters */ }
fn flow<B: InitPollBundle + CastVoteBundle>(svm: &mut LiteSVM, b: &B) { /* drives both */ }
```

Properties:

- Shared accounts cost zero extra fields on the fixture (one field, two impl methods returning it). (B) and (C) pay for shared accounts in struct shape; (D) pays in trait impls, which collapse for shared fields.
- Per-field overrides fall out of newtype-wrapping the fixture and re-impling one accessor; no `_with_overrides` builder.
- Subsumes (A): "skip" means "no trait method generated"; the user passes the field to `build_ix` directly.
- Backwards-compat is clean: today's monolithic bundle keeps working via a blanket impl of every trait, driven by the `#[derive(Bundle)]` companion.

Costs:

- Verbosity at the fixture, unless a scaffolding derive (`#[derive(BundleScaffold)]`) auto-impls the obvious case.
- No reflection over "all fields in the bundle"; matters only if tooling enumerates bundle shape.
- More macro machinery than (B); arguably less than (C) since there's no embedded-core layer.

Relationship to (B), (C): same "per-ix" instinct, but reframes the composition primitive from struct embedding (B/C) to trait bounds. The framing shift is from "what fields go in the bundle struct?" to "what ixs does this fixture support?". Survey results decide whether the trait approach is worth the macro complexity over (B).

## Deferred until the survey

1. Does "the bundle is the test suite's account index" property survive (B)? If not, what replaces it?
2. (A) seems strictly additive — ship independently regardless?

## Context

Surfaced while dogfooding `class/ask` against an Anchor escrow program at `cds-amal/web3-escrow:dogfood-bundled-pubkeys`. Union worked great at that size; the question is what happens when the program is 5-10x bigger.
