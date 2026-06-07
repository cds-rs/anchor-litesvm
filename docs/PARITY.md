# Feature parity: `main`/HEAD vs `compat/anchor-0.31`

Until we cut a release, the premise is that the LTS compatibility branch
(`compat/anchor-0.31`) holds **feature parity** with the main line (currently
`feat/euler`). After release, `compat/anchor-0.31` becomes bug-fixes-only and
this invariant lapses; see [the LTS branch note](#scope) below.

This document is the parity ledger: it's both a worklist (what still has to be
backported) and a glance-test (are we at parity right now?). Update it in the
same PR as any feature that touches one branch, so the ledger never lies.

## What "parity" means here

Parity is about the **feature surface**, not identical source. The two branches
target different ecosystems on purpose:

| | `feat/euler` (HEAD) | `compat/anchor-0.31` |
|---|---|---|
| anchor-lang | 1.0.0 | =0.31.1 |
| solana | 3.x (split crates) | =2.2.1 |
| litesvm | path dep (unreleased 0.12) | =0.6.1 |
| spl-token / -ata | 9.0 / 8.0 | =7.0 / =6.0 |
| CPI tree source | `litesvm::cpi_tree` (upstream) | `crate::cpi_tree` (local port of 0.12's parser) |

So there is an irreducible **adaptation layer** that *should* differ and is not
a parity violation: the local `crate::cpi_tree` port, the dep pins, the import
redirects (`litesvm::cpi_tree` -> `crate::cpi_tree`), and the solana/spl/anchor
API adaptations that follow. Parity is satisfied when, modulo that layer, every
feature has a behaviorally-equivalent counterpart on the other branch, proven by
the snapshot and `trybuild` suites.

**Legend:** ✓ present  ·  ✗ absent  ·  ≈ present but divergent (needs converging)
·  n/a adaptation layer (divergence is expected, not a gap)

## Derive macros (`anchor-litesvm-derive`)

| Feature | HEAD | compat | Parity | Action |
|---|:---:|:---:|---|---|
| `#[derive(Bundle)]` | ✓ | ✓ | ✓ | shared (pre-fork) |
| `#[derive(BundledPubkeys)]` | ✓ | ✓ | ✓ | shared; see attr note below |
| `#[derive(BundleFrom)]` (`from_fixtures`, `from`) | ✓ | ✗ | gap | backport `175f84e` |
| `#[derive(AliasMirror)]` (`alias`) | ✓ | ✗ | gap | backport `69bf029` |
| `#[derive(BundleSetters)]` | ✗ | ✗ | ✓ | removed from compat: redundant with `Bundle` + struct-update, no boilerplate saving over HEAD ([compat ⊆ HEAD](#keeping-parity-cheap)) |
| `#[bundle(unwrap)]` / `#[bundle(wrap_some)]` field attrs | ✓ | ✓ | ✓ | forward-ported compat→HEAD: a real `BundledPubkeys` shape-fixup HEAD lacked, not subsumed by `BundleFrom`. Both branches' `BundledPubkeys` now match |

> **N.B. (BundledPubkeys attribute skew).** HEAD's `BundledPubkeys` accepts only
> `bundled_with`; compat's also accepts `bundle` (from `d148890`). That extra
> attribute is the `unwrap`/`wrap_some` machinery, which converges into
> `BundleFrom`'s `from` expression. After the backport, compat's `BundledPubkeys`
> should match HEAD's attribute set.

## Transaction rendering (`litesvm-utils`)

| Feature | HEAD | compat | Parity | Action |
|---|:---:|:---:|---|---|
| `TransactionResult` core (`assert_*`, `logs`, `compute_units`, ...) | ✓ | ✓ | ✓ | shared |
| `print_logs_structured()` + Legend + alias substitution | ✓ | ✓ | ✓ | compat ported via `772fa01` |
| Structured CPI tree | ✓ | ✓ | n/a | HEAD: `litesvm::cpi_tree`; compat: `crate::cpi_tree` port (`e8f4e9b`) |
| Tree ANSI styling + failure-rendered-last (`style.rs`) | ✓ | ✓ | ✓ | backported `e959b2d` (HEAD `tree.rs` adopted, `cpi_tree` import redirected) |
| Mermaid `sequenceDiagram` (`mermaid.rs`) | ✓ | ✓ | ✓ | backported `099f6a0` (`cpi_tree` import redirected) |
| Anchor error-name surfacing | ✓ | ✓ | ✓ | converged on HEAD (`a6862ba`): `extract_anchor_error_name` from Anchor's log line replaces compat's hardcoded table; `send_err_named`'s `expected_error_name` display plumbing retired |
| `print_markdown_pair()` | ✓ | ✓ | ✓ | backported `dbb5f7a` (after the rendering group it depends on) |

## Runtime (`anchor-litesvm`)

| Feature | HEAD | compat | Parity | Action |
|---|:---:|:---:|---|---|
| `Program` builder / deploy | ✓ | ✓ | ✓ | shared |
| Require a program name on every deploy entry point | ✓ | ✓ | ✓ | backported `870662b`; conflict resolved keeping litesvm 0.6's `add_program`-returns-`()` form |
| `Tx<'a>` fluent transaction builder (`tx.rs`) | ✓ | ✓ | ✓ | backported `ac66f6e` + the BuildableIx bound relaxation; only `tx.rs` Keypair/Signer imports adapted to `solana_sdk` |
| Context-owned aliases (`Aliases::add`) | ✓ | ✓ | ✓ | shared (`c593391`, pre-fork) |

## Test helpers / fixtures (`litesvm-utils`)

| Feature | HEAD | compat | Parity | Action |
|---|:---:|:---:|---|---|
| `test_helpers` core | ✓ | ✓ | ✓ | shared |
| `Report` (`report.rs`) | ✓ | ✓ | ✓ | backported `d2517a7` (pure std, verbatim) |
| Report ergonomics: `label()`, `md_kv!` / `md_table!` | ✓ | ✓ | ✓ | added to both (`156b2f7`): alias-name resolver + MarkdownBlock macros; ecosystem-agnostic, cherry-picked clean |
| Report abort honesty: `expect_panic()`, `ABORTED` / `RED (expected)` statuses | ✓ | ✓ | ✓ | added on HEAD (`b744a1c`), cherry-picked clean to compat; pure std |
| Deterministic keypairs + `actors.rs` | ✓ | ✓ | ✓ | backported `d2517a7`; `Keypair::new_from_array` -> `keypair_from_seed` for solana 2.x |
| `create_token_mint_at()` | ✓ | ✓ | ✓ | backported `d2517a7`; cherry-pick relocated compat's own spl-7 mint body, so no spl-9 leak |

## Docs

| Feature | HEAD | compat | Parity | Action |
|---|:---:|:---:|---|---|
| `docs/CONVENTIONS.md` | ✓ | ✓ | ✓ | backported `eef5fb3` after `d2517a7` (its referenced APIs now exist on compat) |
| Rustdoc warning cleanup (`transaction.rs`) | ✓ | ✓ | ✓ | folded into the `print_markdown_pair` + color-doc port (`a40207a` text taken pre-fixed, no warnings introduced) |
| `docs/PARITY.md` (this file) | ✓ | ✓ | ✓ | committed to both branches |

## Adaptation layer (expected divergence, not gaps)

These exist on `compat` only and have no HEAD counterpart by design:

| Item | Why it diverges |
|---|---|
| `crate::cpi_tree` (`cpi_tree.rs`) | litesvm 0.6.1 has no `cpi_tree` module; this is a local port of 0.12's parser. HEAD imports the upstream module instead. |
| `=`-pinned deps (anchor 0.31, solana 2.x, spl 7/6, litesvm 0.6) | Lockfile reproducibility against the narrow anchor-0.31 ecosystem. |

## Backport worklist (ordered)

Bottom-up by dependency; easiest first to surface ecosystem seams early.

1. **Derive crate** [done] — `69bf029` (AliasMirror) and `175f84e` (BundleFrom)
   ported to compat; `BundleSetters` removed (redundant with `Bundle` +
   struct-update); `#[bundle(unwrap/wrap_some)]` forward-ported compat→HEAD (a
   real `BundledPubkeys` shape-fixup HEAD lacked, not subsumed by `BundleFrom`).
2. **Tree / mermaid / style + error names** [done] — `e959b2d` + `099f6a0` +
   `a6862ba`. HEAD's `tree.rs` / `mermaid.rs` / `style.rs` adopted wholesale with
   the one `litesvm::cpi_tree` -> `crate::cpi_tree` import redirected (the
   `crate::compat` seam was dropped: the skew is a single import line, so the
   seam was overkill). `transaction.rs` hand-merged: compat's `solana_sdk`
   imports and `fee()`-returns-0 (litesvm 0.6 has no fee field) preserved; error
   names converged on HEAD; `send_err_named`'s `expected_error_name` display
   retired. Had to come *before* docs/presentation: that group depends on these.
3. **Docs / presentation (partial)** — `dbb5f7a` (`print_markdown_pair`) +
   `a40207a` (rustdoc fixes) [done]. `eef5fb3` (CONVENTIONS.md) is **deferred to
   after step 4's `d2517a7`**: it documents `Report` / `deterministic_keypair` /
   `create_token_mint_at` as live APIs, which don't exist on compat until the
   runtime group lands.
4. **Runtime** [done] — `870662b` (require program name; kept litesvm 0.6's
   `add_program` form) -> `ac66f6e` (`Tx` + bound relaxation; `tx.rs` imports
   adapted) -> `d2517a7` (Report / actors / `create_token_mint_at`; `keypair_from_seed`
   for solana 2.x, compat's spl-7 mint body preserved).
5. **Docs capstone** [done] — `eef5fb3` (CONVENTIONS.md), ported once `d2517a7`
   made its referenced APIs live on compat.
6. **Verify** — `cargo build && cargo test` on the compat toolchain after each
   group; `trybuild` + snapshot suites catch derive/render regressions;
   recompile examples (`870662b` changes their signatures). All groups green.

With steps 1-5 done, the backport is **feature-complete**: every euler-only
commit is on compat, adapted to anchor 0.31 / solana 2.x / spl 7 / litesvm 0.6.

## Keeping parity cheap

Until release, every feature on the main line has to be reproduced here. These
techniques keep that from being a tax. They're ordered by leverage for this
repo; the worked example throughout is the `AliasMirror` backport, where the new
files checked over clean and 100% of the friction was in shared wiring.

1. **New file per feature.** A feature whose payload lives in its own module
   backports with `git checkout <main> -- path/to/new_file.rs` (and its test),
   byte-for-byte, no conflict. The residual friction is always the *wiring* (the
   `mod foo;`, the `pub use`, a match arm) and any *runtime coupling* the feature
   needs; see techniques 4 and 5.

2. **Concentrate ecosystem skew behind one seam.** The dominant tax is the
   dependency divergence (`litesvm::cpi_tree` here is `crate::cpi_tree`; solana
   3.x vs 2.x types). Route it through an internal re-export module
   (`crate::compat`) whose *body* differs per branch but whose *path* is
   identical on both:

   ```rust
   // main:   pub use litesvm::cpi_tree;
   // compat: pub use crate::cpi_tree;
   ```

   Feature code then writes `use crate::compat::cpi_tree;` identically on both
   branches and checks over cleanly. Tradeoff: this is indirection on `main`
   that exists only to subsidize `compat`, so apply it at genuine hotspots (the
   cpi_tree import), not everywhere. The seam file itself is adaptation-layer:
   never backported, marked n/a above.

3. **Atomic feature commits, quarantined from refactors.** A feature commit that
   also carries a rustfmt pass, a dep bump, or an unrelated rename can't be
   `cherry-pick -x`'d cleanly. One concern per commit makes cherry-pick the
   default and hand-porting the exception. Corollary: commit the new-file payload
   separately from the wiring line, so a wiring conflict stays a one-liner.

4. **Shrink unavoidable wiring conflicts.** The registration lines that can't
   move into a new file are conflict magnets when combined. One `pub use` per
   line (rather than a grouped `{A, B, C}`) lets 3-way merge resolve features
   independently. Soft win: rustfmt may recombine, so don't over-invest.

5. **Design features against the already-public surface.** When a feature leans
   only on API that's already `pub` on both branches, its backport needs zero
   shared-file edits. (`AliasMirror` missed this by one line: it forced
   `Aliases::resolve_by_pubkey` from `pub(crate)` to `pub`.)

6. **Back the ledger with a mechanical gate.** This document is the human
   record; a checked-in public-API snapshot (e.g. `cargo public-api`) diffed in
   CI would fail loudly when a feature lands on one branch and not the other.
   Release-gate nice-to-have, not yet wired up.

**Provenance habit:** every compat-side commit names the source as `Port of
HEAD's <hash>`, so `git log` is auditable against this ledger.

## Scope

The "bug-fixes-only" freeze is a **post-release** policy, not the current state;
nothing has shipped yet, so features are actively backported. See the
[LTS branch context](../README.md) and the team's MIGRATING.md for the
deprecation timeline (deprecated once upstream mpl-core ships an anchor-lang 1.0
release).
