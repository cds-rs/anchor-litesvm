# How a Pinocchio test evolves: the P-ATA progression

**Status:** design narrative. Records how a real Pinocchio test suite converges
on the testing vocabulary, stage by stage, each stage a real commit. Companion
to [`trait-boundaries.md`](trait-boundaries.md) (the vocabulary the stages
converge on) and [`../adr/0001-executor-owns-the-execution-vocabulary.md`](../adr/0001-executor-owns-the-execution-vocabulary.md).

## Scope

In scope: the arc from a raw, unrunnable corpus test to a drift-proof one, told
as a sequence of diffs, and the principle each stage establishes. The
through-line: each stage removes a category of hand-work by moving knowledge to
the layer that owns it, and the test's *output* improves in lockstep with its
*source*. Every stage is a real commit on the associated-token-account port (the
same arc repeats on the pinocchio-stake port); the excerpts are quoted from those
commits, not reconstructed.

Out of scope: the framework APIs themselves (covered in the user guide and the
trait-boundaries doc); this is the adoption story, not the reference.

The commit chain, oldest first:

```
the same contract through the TestSVM vocabulary on mollusk   (stage 1)
print the structured CPI tree from the model                   (stage 2a)
named trees on both sends, quiet runtime logs                  (stage 2b)
actors declared, snapshot promoted to committed baseline       (stage 2c)
register CreateIdempotent, frames render program::instruction  (stage 2d)
setup speaks the helpers: deploy_from_file + prop              (stage 3)
the instruction enum derives Discriminator, behind a feature   (stage 4)
```

## Stage 0: what the corpus had (the genuine "before")

P-ATA ships a `mollusk-svm` suite (`program/tests/mollusk.rs`). As committed, it
cannot run: it loads the token ELFs from another machine's absolute paths
(`/Users/<author>/...`), and it sends `data: vec![]`, which the program decodes
as `Create`, a literal `todo!()` panic (`CreateIdempotent` is `[1]`). Its
assertions are mollusk's `Check::*` builders: resulting-account checks, with no
vocabulary for the *shape* of execution. The pinocchio-stake "before" is 29
`solana-program-test` banks tests on the solana 2.x line.

The takeaway: the before is not a strawman; it is the corpus as found, including
the parts that could never have passed.

## Stage 1: the same contract through the vocabulary

The first port speaks the `TestSVM` trait, and everything is by hand:

```rust
let program_elf =
    std::fs::read("target/deploy/create_idempotent.so").expect("run `cargo build-sbf` first");
backend.deploy_program(PROGRAM_ID, &program_elf);

let funder = solana_keypair::Keypair::new();
let funder_pk = { use solana_signer::Signer; funder.pubkey() };
backend.fund_sol(&funder_pk, 10_000_000_000);
let wallet = Pubkey::new_unique();
backend.fund_sol(&wallet, 1_000_000_000);

let mint = Pubkey::new_unique();
// ... Mint::pack ... backend.set_account(&mint, Account { ... }) ...

data: vec![1], // CreateIdempotent (empty data is Create, a todo!() in this program)
```

What it bought, day one: the model's frame-shape assertion (the token CPI visible
under the program, which `Check::*` has no words for), and the backend-choice win
(this same create flow is blocked on litesvm by SIMD-0312; mollusk's Agave 4.0
runtime runs it: same test, different backend, rebuild). What it cost: every
actor is four lines, every deploy three, the mint fabrication is a paragraph, the
discriminator is a magic byte, and the trees print raw base58.

## Stage 2: the output learns to speak

Four short steps, each one a friction the dogfood surfaced and the framework
absorbed *at the vocabulary layer*, so every engine inherited it:

- the model renders its own frames (`pretty_cpi_tree`), because the rich
  renderers had been welded to the litesvm adapter and the mollusk graph held
  structured frames with no way to show them;
- the backend owns the alias table (`register_alias` once per actor, every send
  arrives named), retiring the thread-the-aliases-through-every-call tax;
- actors become declarations (`backend.actor("funder", ...)`): deterministic
  keypair + alias + funding in one call, and determinism is what promoted
  `snapshot.md` from a gitignored run artifact to a committed regression baseline;
- instruction names resolve at send time from a registry, because the wire
  carries no names but the message carries program + data.

The tree across this stage, same test:

```
before:  └── (3,497 CU) 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi
              ├── 11111111111111111111111111111111
              └── (235 CU) TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA

after:   └── CreateIdempotent (3,497 CU) create_idempotent
              ├── System
              └── (235 CU) Token
```

## Stage 3: setup becomes declarations

The helper rule was explicit: candidates marked at N=1, landed only when the
second target (pinocchio-stake) hit the same frictions. Both did, so:

```rust
backend.deploy_from_file(&PROGRAM_ID, "target/deploy/create_idempotent.so", "create_idempotent");
backend.deploy_from_file(&token_program_id(), "benches/programs/pinocchio_token_program.so", "Token");

let mint = backend.prop("mint", Account { /* the SPL packing stays here */ });
```

`deploy_from_file` folds read + deploy + alias and *diagnoses* the stub-ELF
failure the stake target taught (an entrypoint-less `.so` under 4 KiB fails as
`EntrypointOutOfBounds`; the panic message now says so). `prop` is the
fabricated-state counterpart of `actor`: deterministic named address + state +
alias, with format-specific packing staying caller-side.

## Stage 4: the program declares its map once

The last hand-maintained knowledge was the instruction map: a string and a magic
byte that could drift from the dispatch. The derive closes it:

```rust
// program (src/lib.rs): release and SBF builds see a plain enum.
#[cfg_attr(feature = "testing", derive(litesvm_pinocchio::Discriminator))]
pub enum AssociatedTokenAccountInstruction { Create, CreateIdempotent, RecoverNested }
```

```rust
// test: bulk-load the generated table; spell the tag as the generated const.
backend.register_program_instructions(
    &PROGRAM_ID,
    create_idempotent::AssociatedTokenAccountInstruction::instruction_names(),
);
data: vec![create_idempotent::discriminators::CreateIdempotent],
```

One declaration now drives three consumers: the on-chain dispatch, the test's
name table, and the IDL (the litesvm-pinocchio-idl extractor reads the same
enum). The name cannot drift from the tag because both *are* the variant.

### The adoption principle (now law)

**Tests must not alter how a Pinocchio program is written or shipped.** The
testing features ride the Serde-style feature-gated derive pattern:
`litesvm-pinocchio` is an *optional* dependency activated only by a `testing`
feature, the derive is `cfg_attr`-gated, and the proof is mechanical:
`cargo tree -e normal` on the release/SBF build shows **zero** testing crates,
and the `.so` builds unchanged. On pinocchio-stake the same pattern held even
though the program had no enum at all: the addition is a plain, inert,
IDL-visible declaration of the native wire order (consensus order, documented as
not-ours-to-reorder), and its derive exists only under the feature.

## The shape of the whole arc

| stage | actor | deploy | state | discriminator | tree |
|---|---|---|---|---|---|
| 0 (corpus) | hand `Account` literals | broken absolute paths | hand-built per ix | magic byte (wrong one) | none |
| 1 (vocabulary) | 4 lines each | `fs::read` + deploy | `set_account` paragraph | magic byte (right one) | raw base58 |
| 2 (idioms) | `actor("funder", …)` | unchanged | unchanged | registered string | named, committed baseline |
| 3 (helpers) | unchanged | `deploy_from_file(…)` | `prop("mint", …)` | unchanged | unchanged |
| 4 (derives) | unchanged | unchanged | unchanged | generated const + bulk table | unchanged, drift-proof |

Each column converges to a single declaration owned by whoever actually knows the
fact: the test knows its cast, the program knows its wire, the engine knows its
execution. That is the vocabulary thesis told as a sequence of diffs.

## What stage 5 looks like (not yet built)

The accounts and args columns are still hand-ordered `AccountMeta` vecs and
hand-packed bytes. The maps exist (`#[account(..)]` attrs, the extractor's
Anchor-spec IDL) and the consumer is proven (`declare_program!` ingests the
extracted IDL, single-byte discriminators honored). Stage 5 is wiring them: typed
account structs and args builders for a Pinocchio program, the same ones Anchor
enjoys, at the same zero release cost.
