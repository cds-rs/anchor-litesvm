# BundledPubkeys: design

## Scope

This doc covers `#[derive(BundledPubkeys)]`: the proc-macro that lets a test
build an Anchor instruction from a "bundle" of pubkeys with one call instead of
hand-filling two parallel structs. It also covers the small family the bundle
work grew (`Bundle`, `BundleFrom`, `AliasMirror`), the `BuildableIx` trait the
derive plugs into, and the diagnostics around the macro (including one we built
and threw away).

Out of scope: the CPI renderers (`docs/design/cpi-rendering.md`) and the SVM
test helpers.

Definitions:

- **Bundle**: a host-only (`#[cfg(not(target_os = "solana"))]`) struct that
  holds the pubkeys a test will populate. Plain `Pubkey` fields, no Anchor
  machinery.
- **Accounts struct / instruction struct**: the two types Anchor generates per
  handler. `accounts::Foo` is the `#[derive(Accounts)]` struct's account list;
  `instruction::Foo` is the args struct (the handler's non-`Context`
  parameters, wrapped for the wire).

## The problem

Constructing an Anchor instruction by hand in a test is two parallel chores:
fill `accounts::Foo { maker, mint_a, vault, escrow, token_program,
system_program }`, fill `instruction::Foo { amount }`, then glue them into an
`Instruction`. The account list is mostly pubkeys the test already has in
variables, plus a few canonical program IDs that never change. Spelled out per
instruction, per test, it's a lot of ceremony around a little data, and it's
ceremony that drifts (rename an account, update every call site).

The bundle idea: name the pubkeys once in a struct, hand that struct to a
single builder.

```rust,ignore
let bundle = EscrowBundle { maker: maker.pubkey(), mint_a, vault, escrow };
let ix = ctx.program().build_ix(bundle, instruction::Make { amount: 1_000 });
```

## What the derive emits

`#[derive(BundledPubkeys)] #[bundled_with(EscrowBundle)]` on a
`#[derive(Accounts)]` struct emits two impls:

```rust,ignore
// 1. Project the bundle into the account list. Pubkey fields map by name;
//    canonical program IDs are auto-injected from the field's Anchor type.
impl From<EscrowBundle> for crate::accounts::Make {
    fn from(b: EscrowBundle) -> Self {
        Self {
            maker: b.maker,
            mint_a: b.mint_a,
            vault: b.vault,
            escrow: b.escrow,
            token_program: anchor_spl::token::ID,                          // injected
            system_program: anchor_lang::solana_program::system_program::ID, // injected
        }
    }
}

// 2. Pair the args struct with its accounts struct at the type level.
impl ::anchor_litesvm::BuildableIx<EscrowBundle> for crate::instruction::Make {
    type Accounts = crate::accounts::Make;
}
```

Two design choices carry the weight:

**Program IDs are auto-injected, not bundled.** Fields typed
`Program<'_, System>`, `Program<'_, AssociatedToken>`, and
`Interface<'_, TokenInterface>` have one canonical pubkey each, so the bundle
shouldn't have to carry them and every test shouldn't have to spell them. The
derive reads the field's Anchor type and fills the ID. The bundle holds only
the pubkeys a test actually varies.

**Pairing is type-level, so a mismatch is a compile error.**
`BuildableIx<B>` ties an args type to its `type Accounts`, and `build_ix`
consumes both:

```rust,ignore
// crates/anchor-litesvm/src/buildable.rs
pub trait BuildableIx<B>: InstructionData {
    type Accounts: ToAccountMetas;
    // ... project From<B> into Accounts, serialize the args, assemble metas ...
}

// crates/anchor-litesvm/src/program.rs
pub fn build_ix<B, A>(self, bundle: B, args: A) -> Instruction
where A: BuildableIx<B> { /* ... */ }
```

Passing `Withdraw` args with a `Deposit` bundle doesn't typecheck: there's no
`BuildableIx<DepositBundle> for instruction::Withdraw`. The error lands at the
call site at compile time, not as a malformed instruction at runtime.
`build_ix_with(bundle, args, |ix| ...)` is the escape hatch for the rare case
that needs to tweak the assembled `Instruction` (an extra account meta, say)
before it goes out.

## The naming precondition, and the override

The derive infers the two target paths from the struct's own name:
`crate::accounts::<StructName>` and `crate::instruction::<StructName>`. That
inference has a precondition that the macro can't see and can't enforce:

| Anchor item | Named from |
|---|---|
| `accounts::Foo` | the `Context<Foo>` type argument on the handler |
| `instruction::Foo` | `PascalCase(fn_name)` of the handler |

So the struct name has to equal `PascalCase(fn_name)`. The common case satisfies
it by construction (you name the struct after the instruction). The case that
breaks is a short handler paired with a longer struct: `fn initialize_poll`
produces `instruction::InitializePoll`, but the struct is `InitPoll`. The macro
emits `impl BuildableIx<...> for crate::instruction::InitPoll`, which doesn't
exist.

The fix is an explicit override on `bundled_with`:

```rust,ignore
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(
        crate::test_helpers::InitPollBundle,
        instruction = crate::instruction::InitializePoll,  // override the inferred path
        accounts = crate::accounts::InitPoll,              // rarely needed; here for symmetry
    )
)]
#[derive(Accounts)]
pub struct InitPoll<'info> { /* ... */ }
```

`accounts =` is rarely needed (Anchor pulls that name from `Context<Foo>`, so
it usually matches the struct by construction), but it's allowed for symmetry.

**N.B.** The override keys are bare (`instruction = ...`, not
`override_instruction = ...`), following `serde(rename = ...)` precedent: both
keys override macro-inferred paths, so a prefix wouldn't distinguish them, and
`bundled_with(...)` already namespaces them.

## Shape fixups: `#[bundle(unwrap)]` / `#[bundle(wrap_some)]`

One bundle is often shared across several accounts structs that disagree on a
field's optionality: an account that's `Option<Pubkey>` in one instruction and
required in another. Rather than force a separate bundle per shape, two
per-field attributes bridge the gap during projection:

- `#[bundle(unwrap)]` projects an `Option<T>` bundle field into a bare `T`
  account field: `b.field.expect("...")`, panicking with a pointed message if
  the bundle left it `None`.
- `#[bundle(wrap_some)]` does the reverse: a bare `T` bundle field into an
  `Option<T>` account field (`Some(b.field)`).

Without an annotation, a type mismatch between the bundle field and the account
field is a plain compile error, which is the right default: the fixup is opt-in
because silently coercing optionality is the kind of thing you want to be
explicit about.

## Diagnostics (and an avenue we abandoned)

The macro has two failure modes, and they resolve through different mechanisms.

**Path resolution (wrong name, no override).** When the inferred
`instruction::<StructName>` doesn't exist, rustc raises E0425 against the path
the macro emitted. The useful part is the span: `quote!`'s `#ident`
interpolation carries the struct ident's *original* span, so the resolution
failure is attributed to the struct definition site, with a "consider
importing" suggestion:

```text
error[E0425]: cannot find type `InitPoll` in module `crate::instruction`
   |
   | pub struct InitPoll<'info> {
   |            ^^^^^^^^ not found in `crate::instruction`
help: consider importing this struct
   | use crate::accounts::InitPoll;
```

**Remark (the probe we threw away).** The override design originally came
paired with a "diagnostic probe": a `const _: fn() = || { ... PhantomData<crate::instruction::#ident> ... }`
emitted alongside the impls, on the theory that it would give attribute messages
a stable anchor. We implemented it, tested it against a trybuild fixture, and
found it did nothing: the bare impl already produced exactly the E0425 above
(same span, same suggestion), because the span travels with the interpolated
path regardless. The probe just produced a second, identical error. Pure
duplication, dropped. The lesson worth keeping: `quote!` span propagation gives
you good diagnostics for free when you interpolate a user-provided ident into
the generated code; reach for a probe only when you've confirmed the bare
emission doesn't already point at the right place.

(One caveat: a real dogfood case in `~/sol/voting` reported an uglier
E0726 / E0603 lifetime-and-privacy cascade rather than the clean E0425, and we
couldn't reproduce it in trybuild. It's probably an interaction between
Anchor's `<'info>`-parameterized structs and the privacy of the generated
`accounts::` module that the fixture doesn't model. If it resurfaces, that's
when a probe or a richer diagnostic earns its place.)

**Bound not satisfied (`build_ix` with the wrong args type).** Calling
`program.build_ix(bundle, args)` where `args` has no `BuildableIx<Bundle>` impl
is a trait-bound failure, not a path failure, so it's covered by
`#[diagnostic::on_unimplemented]` on the trait:

```text
`instruction::Withdraw` can't be built with bundle `DepositBundle`:
no `BuildableIx<DepositBundle>` impl
note: Add `#[derive(BundledPubkeys)] #[bundled_with(DepositBundle)]` to the ...
```

## Usage gotcha: keep `bundled_with` inside the `cfg_attr`

`#[bundled_with(...)]` must live in the *same* `cfg_attr` that brings in the
derive. Pulled out into a bare `#[bundled_with(...)]` attribute, it either fires
without the derive present or reads as an unknown attribute under
`target_os = "solana"`, and the symptom is the same E0726 cascade as a naming
mismatch. The canonical form is the combined `cfg_attr` shown above: derive and
its configuration gated together, off for the on-chain BPF build.

## The bundle family

`BundledPubkeys` is the core; three smaller derives round it out (each is its
own concern, listed here for orientation):

- **`Bundle`** emits a `Default` impl that fills every `Pubkey` field with
  `Pubkey::new_unique()`, so a bundle is ready to populate from test setup
  without spelling out placeholders. Pair it with `BundledPubkeys`'s bundle
  struct.
- **`BundleFrom`** (`#[from_fixtures]` / `#[from]`) projects a bundle from
  multiple source structs, for tests that assemble pubkeys from several actor
  objects.
- **`AliasMirror`** (`#[alias]`) generates one-shot pubkey aliasing, wiring a
  struct's fields into the `Aliases` table so the rendered output (see
  `cpi-rendering.md`) reads in the test's own vocabulary.

## References

- Derive crate: `crates/anchor-litesvm-derive/src/` (`lib.rs` for the public
  macro docs, `parse.rs` for `bundled_with` parsing, `emit.rs` for the emitted
  impls).
- The trait: `crates/anchor-litesvm/src/buildable.rs` (`BuildableIx`,
  `on_unimplemented`); the builder: `crates/anchor-litesvm/src/program.rs`
  (`build_ix`, `build_ix_with`).
- Tests: `tests/derive_buildable.rs`, `tests/derive_buildable_override.rs`,
  and the `tests/compile_fail/` trybuild fixtures
  (`instruction_name_mismatch`, `build_ix_wrong_args_type`).
