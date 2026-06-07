# ADR-0001: `BundledPubkeys` instruction path: explicit override + diagnostic probe

- **Status:** Accepted (override shipped; probe tried and dropped, see [Outcome](#outcome))
- **Date:** 2026-05-23
- **Implemented:** 2026-05-24 (commit `2df0406`; related diagnostic in commit `b840570`)
- **Source:** dogfooding `#[derive(BundledPubkeys)]` against `~/sol/voting`

## Context

The `#[derive(BundledPubkeys)]` proc-macro at `crates/anchor-litesvm-derive/`
emits two impls per Accounts struct:

```rust,ignore
// emit.rs (current)
impl ::core::convert::From<#bundle> for crate::accounts::#accounts_ident { /* ... */ }
impl ::anchor_litesvm::BuildableIx<#bundle> for crate::instruction::#accounts_ident { /* ... */ }
```

`#accounts_ident` is taken directly from the struct the derive is attached to.
That single identifier is reused for both `accounts::Foo` and
`instruction::Foo`. But Anchor names those two types from different sources:

| Anchor item        | Named from                                   |
|--------------------|----------------------------------------------|
| `accounts::Foo`    | the `Context<Foo>` type argument on the handler |
| `instruction::Foo` | `PascalCase(fn_name)` of the handler         |

So `BundledPubkeys` has a silent precondition: the Accounts struct must be
named to match `PascalCase(fn_name)`. Mismatches don't surface as the
underlying naming problem; they surface as a misleading lifetime error.

### Reproduction

`programs/voting/programs/voting/src/`:

```rust,ignore
// lib.rs
#[program]
pub mod voting {
    use super::*;
    pub fn initialize_poll(ctx: Context<InitPoll>, /* ... */) -> Result<()> { /* ... */ }
}

// instructions/initialize_poll.rs
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(crate::test_helpers::InitPollBundle)
)]
#[derive(Accounts)]
pub struct InitPoll<'info> { /* ... */ }
```

`fn initialize_poll` produces `instruction::InitializePoll`, but the struct is
`InitPoll`. The macro emits `impl BuildableIx<...> for crate::instruction::InitPoll`,
which doesn't exist. rustc reports:

```
error[E0726]: implicit elided lifetime not allowed here
 --> .../initialize_poll.rs:8:12
  |
8 |     derive(anchor_litesvm::BundledPubkeys),
  |            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected lifetime parameter

error[E0603]: struct import `InitPoll` is private
  --> .../initialize_poll.rs:13:12
```

Neither message mentions `instruction::`, `PascalCase`, or the handler. The
investigator's natural reaction is to add lifetime annotations to the derive
(no such option) or to the struct (already there). Resolution required reading
the macro's emit code.

## Considered options

### Option 1: Infer the instruction name from the source file path

`initialize_poll.rs` → `InitializePoll`. Rejected on three grounds:

1. **Unstable.** `proc_macro::Span::source_file()` requires the
   `proc_macro_span` feature, which is unstable.
2. **Couples to the wrong thing.** File path isn't authoritative; the Anchor
   handler's function name is. A user who keeps multiple Accounts structs in
   one file, or names files differently from their handlers, would break.
3. **Hides the dependency.** Filename-based inference would replace one silent
   coupling (struct name = `PascalCase(fn_name)`) with another (struct name =
   `PascalCase(file_name)`). The coupling is no less silent for being
   different.

### Option 2: Emit a probe + improve the diagnostic, but no override

Better error message; still forces the user to rename the struct or the
handler to bring them into alignment. Doesn't unlock the case where a short
handler name (`fn init_poll`) is paired with a longer file/struct name
(`initialize_poll.rs`, `struct InitializePoll`).

### Option 3: Accept an explicit `instruction = ...` override, paired with a diagnostic probe

Composable: the override solves the case where rename isn't desirable; the
probe makes the failure mode legible whether or not the override is in use.

## Decision

Adopt **Option 3** (both halves).

### Diagnostic probe

Emit a small const probe whose only job is to resolve the referenced path:

```rust,ignore
// emit.rs
quote! {
    // Probe: if `crate::instruction::#accounts_ident` doesn't resolve,
    // the user gets *this* error site, which we can annotate.
    #[doc(hidden)]
    const _: fn() = || {
        const _ASSERT_INSTRUCTION_EXISTS:
            ::core::marker::PhantomData<crate::instruction::#accounts_ident>
            = ::core::marker::PhantomData;
    };

    impl ::anchor_litesvm::BuildableIx<#bundle> for crate::instruction::#accounts_ident { /* ... */ }
    impl ::core::convert::From<#bundle> for crate::accounts::#accounts_ident { /* ... */ }
}
```

The probe doesn't fix anything by itself; it gives a stable anchor for
attribute messages. A more involved version uses `trybuild`-style assertions
or wraps the failing path with `compile_error!` text. The win is that the
*first* failure the user sees points at the right place and names the right
fix.

If we integrate
[`#[diagnostic::on_unimplemented]`](https://doc.rust-lang.org/reference/attributes/diagnostics.html),
the message can read directly:

```
note: `BundledPubkeys` expected to find `crate::instruction::InitPoll`.
      Anchor names that type from `PascalCase(fn_name)` of the handler.
      Either rename this struct to match, or set the path explicitly:

          #[bundled_with(InitPollBundle, instruction = crate::instruction::InitializePoll)]
```

### Explicit override

Accept optional `instruction = ...` (and, for symmetry, `accounts = ...`)
arguments on `bundled_with`. When present, use them verbatim instead of
`#accounts_ident`:

```rust,ignore
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(
        crate::test_helpers::InitPollBundle,
        instruction = crate::instruction::InitializePoll,
    )
)]
#[derive(Accounts)]
pub struct InitPoll<'info> { /* ... */ }
```

The `accounts =` override is rarely needed in practice (Anchor pulls that
name from `Context<Foo>`, so the names usually match by construction), but
worth allowing for symmetry:

```rust,ignore
bundled_with(
    InitPollBundle,
    accounts = crate::accounts::InitPoll,
    instruction = crate::instruction::InitializePoll,
)
```

**N.B.** Keys are bare (`instruction = ...`, not `override_instruction = ...`),
following `serde(rename = "...")` precedent. Both keys override
macro-inferred paths, so a prefix wouldn't distinguish them from anything;
the attribute name `bundled_with(...)` already namespaces them. If we later
add a knob that *isn't* an override (a hypothetical `signer_seed = ...`,
say), we can rename at that point and keep the bare form as a deprecated
alias.

The probe runs against the *final* resolved path (override if given,
inferred name otherwise) so the diagnostic stays useful in both modes.

## Outcome

Shipped in commit `2df0406` as the **override only**. The probe was
implemented, tested empirically, and dropped.

**Override (shipped as designed):** `parse.rs` grew a custom `Parse` impl
on a new `BundledWith` struct that reads the bundle path followed by
optional order-independent `key = path` pairs (trailing-comma-tolerant,
with dup-key and unknown-key errors). `Spec` gained
`instruction_path: Option<syn::Path>` and `accounts_path: Option<syn::Path>`.
`emit.rs` got two small helpers (`accounts_target` / `instruction_target`)
that prefer the override when set and otherwise interpolate
`crate::{accounts,instruction}::#accounts_ident`. New integration test
(`tests/derive_buildable_override.rs`) exercises the end-to-end path with
the dogfood mismatch (`struct InitPoll` paired with
`instruction::InitializePoll`).

**Probe (not adopted):** the trybuild fixture
(`tests/compile_fail/instruction_name_mismatch.rs`) showed that the bare
impl alone already produces the diagnostic we wanted:

```text
error[E0425]: cannot find type `InitPoll` in module `crate::instruction`
  --> tests/compile_fail/instruction_name_mismatch.rs:53:12
   |
53 | pub struct InitPoll<'info> {
   |            ^^^^^^^^ not found in `crate::instruction`
help: consider importing this struct
   |
12 + use crate::accounts::InitPoll;
```

The good span attribution (the struct name, not the derive call) that
the ADR attributed to the probe actually comes from `quote!`'s
`#accounts_ident` interpolation: the path emitted into the impl carries
the struct ident's original span, so the resolution failure rustc raises
against the path is automatically attributed to the struct definition
site. Adding the probe just produced a *second* error of the same shape,
same span, same suggestion. No diagnostic improvement; pure duplication.
Dropped.

The ADR's reported E0726 / E0603 cascade (lifetime + privacy errors
instead of the cleaner E0425) couldn't be reproduced in trybuild. The
real-world case probably exercises some interaction between Anchor's
`<'info>`-parameterized accounts structs and the privacy of the generated
`accounts::` module that our fixture doesn't model. If a real-world
reproduction surfaces the cascade, revisit with a fixture that exercises
it; the probe (or a more elaborate diagnostic) could be reintroduced at
that point.

**Adjacent improvement (separate, but related):** commit `b840570` added
`#[diagnostic::on_unimplemented]` to the `BuildableIx` trait in
`crates/anchor-litesvm/src/buildable.rs`. That covers the *other* macro-
adjacent failure mode (user calls `program.build_ix(bundle, args)` where
`args` lacks the `BuildableIx<Bundle>` impl), which is a trait-bound
failure rather than a path-resolution failure. Different mechanism,
different fixture (`tests/compile_fail/build_ix_wrong_args_type.rs`),
out of scope for this ADR but cited for the record.

## Consequences

- Users with mismatched handler/struct names (`fn initialize_poll` paired
  with `struct InitPoll`) get a single legible E0425 pointing at the
  struct name with a "consider importing" suggestion (e.g. for the
  `accounts::*` path). The `instruction = ...` override is the actual
  fix; the diagnostic surfaces the symptom clearly enough that the user
  reaches for it.
- Users who want short handler names paired with auto-derived snake-case
  file layout (`fn init_poll` in `initialize_poll.rs`) can wire it up
  without renaming either the file or the handler.
- The macro stops dictating naming conventions; it only asks the user to
  wire the dependency it can't see.

## Adjacent gotcha (worth a doc note, not in scope here)

`#[bundled_with(...)]` outside the `cfg_attr` that brings in the derive
causes the macro to fire without the attribute (or the attribute to be an
"unknown attribute" under `target_os = "solana"`), producing the same E0726
cascade as the naming mismatch. A README example showing the canonical
combined-`cfg_attr` form, paired with the diagnostic fix above, would catch
this for new users.

## Implementation pointers

- Emit site: `crates/anchor-litesvm-derive/src/emit.rs::emit_buildable_impl`
  (uses `accounts_target` / `instruction_target` helpers).
- Attribute parse: `crates/anchor-litesvm-derive/src/parse.rs::extract_bundled_with`
  (delegates to `BundledWith::parse`).
- Trybuild fixture for the path-resolution diagnostic:
  `crates/anchor-litesvm-derive/tests/compile_fail/instruction_name_mismatch.rs`
  + `.stderr`.
- Trybuild fixture for the bound-not-satisfied diagnostic (from the
  adjacent `on_unimplemented` work in commit `b840570`):
  `crates/anchor-litesvm-derive/tests/compile_fail/build_ix_wrong_args_type.rs`
  + `.stderr`.
- Override integration test:
  `crates/anchor-litesvm-derive/tests/derive_buildable_override.rs`.

## References

- Example project that hit this: `~/sol/voting/programs/voting/`. The
  naming-mismatch case is gotcha #1 in that program's `gotchas.md`.
