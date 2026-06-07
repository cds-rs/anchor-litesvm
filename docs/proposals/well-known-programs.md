# Well-known programs: replace the injection table with the type system

`BundledPubkeys` decides which accounts a bundle must carry and which it
injects as constants. Today that decision is a hardcoded table of three
textual type matches:

| Field type | Injected constant |
|---|---|
| `Program<'info, System>` | `anchor_lang::solana_program::system_program::ID` |
| `Program<'info, AssociatedToken>` | `anchor_spl::associated_token::ID` |
| `Interface<'info, TokenInterface>` | `anchor_spl::token::ID` |

Everything else projects from the bundle by field name. This proposal replaces
the table with a structural rule, adds one field-level escape hatch, and adds
the per-field `Default` override the `Bundle` derive has been missing.

## Where this came from

The nft-staking project (the compat-branch dogfood) is the first consumer
whose program depends on a non-SPL external program. mpl-core's program
account *looks* like it should be well-known and is not:

```rust
#[account(address = MPL_CORE_ID)]
pub mpl_core_program: UncheckedAccount<'info>,
```

The derive cannot recognise this (the program's identity lives in the
constraint expression, not the type), so `mpl_core_program` rides in the
bundle, and because `#[derive(Bundle)]`'s generated `Default` fills every field
with `Pubkey::new_unique()`, the project hand-rolls `Default` to pin the one
field that must be real. The marketplace project (the other mpl-core consumer)
hits the same wall even though it declares the account *correctly* as
`Program<'info, MplCore>`: the derive's table simply doesn't have an MplCore
row.

Both workarounds are documented in the staking README as a measured limit of
the derive. This proposal removes the limit.

## The design

Three pieces, ordered by how much they carry.

### 1. The classification rule: any `Program<'info, T>` injects `T::id()`

Anchor already has the abstraction the table was approximating:
`Program<'info, T>` requires `T: anchor_lang::Id`, and `Id::id()` is the
program's address. So the derive's rule becomes:

> A field of type `Program<'info, T>`, for any `T`, is well-known. Its
> projection is `<T as anchor_lang::Id>::id()`, and it never appears in the
> bundle.

The generated `From` impl emits the type path exactly as the field declares it
(`<MplCore as anchor_lang::Id>::id()`), so it resolves wherever the accounts
struct compiles; no path registry, no constants table. `System` and
`AssociatedToken` both implement `Id`, so two of the three table rows fall out
of the general rule rather than being special.

The one special case that remains is `Interface<'info, TokenInterface>`:
`TokenInterface` implements the plural `Ids` (classic Token and Token-2022),
so there is no single `id()` to call. The derive keeps its existing opinion
(inject classic `anchor_spl::token::ID`), and Token-2022 tests keep using
`build_with` to override, exactly as documented today.

What this asks of program code: declare program accounts as
`Program<'info, T>` rather than `UncheckedAccount` + `address =`. That is
already the better Anchor idiom (ownership and executable checks come with the
type; `address =` only pins the key), and where the external crate doesn't
ship an `Id` type, it is five lines:

```rust
pub struct MplCore;
impl anchor_lang::Id for MplCore {
    fn id() -> Pubkey { mpl_core::ID }
}
```

### 2. The hatch: `#[bundle(inject = expr)]`

For accounts that genuinely cannot be a typed `Program<T>` (no meaningful type
to hang `Id` on, or a deliberately unchecked account), a field-level attribute
in the existing `#[bundle(...)]` grammar, next to `unwrap` / `wrap_some`:

```rust
#[cfg_attr(not(target_os = "solana"), bundle(inject = mpl_core::ID))]
#[account(address = MPL_CORE_ID)]
pub mpl_core_program: UncheckedAccount<'info>,
```

Projection becomes the given expression instead of `b.field`; the field stops
being part of the bundle's required vocabulary. Same precedence rule as the
existing field attributes: an explicit `#[bundle(...)]` beats the structural
classification, and the derive honours it rather than silently ignoring it.

### 3. The orthogonal piece: `#[bundle(default = expr)]` on the `Bundle` derive

Even with 1 and 2, some bundles will carry a field that wants a meaningful
default rather than a random placeholder (a known mint, a fee account, a
program that stays overridable on purpose). The `Bundle` derive grows a
per-field override:

```rust
#[derive(Bundle, AliasMirror)]
pub struct StakingBundle {
    pub owner: Pubkey,                       // Pubkey::new_unique(), as today
    #[bundle(default = mpl_core::ID)]
    pub mpl_core_program: Pubkey,            // the real ID
}
```

This is what deletes the hand-rolled `Default` impls downstream. Fields
without the attribute keep the fail-loudly placeholder semantics.

## Bundles describe the world; `build_with` describes deviations

Injection removes a field from the bundle, which raises the question of how a
negative test passes a *wrong* program (the staking suite does exactly this:
`create_collection_rejects_wrong_mpl_core_program`). The answer is the
division of labour the framework already has:

- the bundle describes the scenario's world: every account as it should be;
- `build_with(bundle, args, |a| ...)` describes one transaction's deliberate
  deviation from that world, after projection.

So the wrong-program test sets `a.mpl_core_program = fake` in the closure.
That is not a workaround; it is the negative-test idiom working as designed,
and it makes the test read as what it is: "the world is fine, this one call
lies about the program."

## Downstream changes (same wave, no shims)

The crate is unreleased; downstream is the dogfood projects, and they change
in the same wave as the derive:

- **nft-staking**: declare `Program<'info, MplCore>` (with the five-line `Id`
  wrapper), drop `mpl_core_program` from `StakingBundle`, delete the
  hand-rolled `Default` (replaced by `#[derive(Bundle)]`), rewrite the
  wrong-program negative test on `build_with`, rewrite the README's
  "where the derive meets its limits" section to describe the rule instead of
  the limit.
- **web3-nft-marketplace**: already declares `Program<'info, MplCore>`; drops
  the field from `MarketplaceBundle` and most of its hand-rolled `Default`.
- **vault / escrow / amm**: no source changes; their three well-known programs
  reclassify from "table rows" to "instances of the general rule" with
  identical emitted values.

## Implementation notes

All in `anchor-litesvm-derive`:

- `parse.rs` `classify_field_type`: replace the three-entry match with (a) the
  general `Program<'info, T>` arm that captures `T`'s path, (b) the
  `Interface<'info, TokenInterface>` special case, kept. The function's return
  type changes from "constant path" to "injection expression".
- `parse.rs` `extract_bundle_attr`: accept `inject = <expr>` alongside
  `unwrap` / `wrap_some`; unknown keys stay compile errors.
- The `Bundle` derive's `Default` emission: accept `default = <expr>` per
  field.
- `emit.rs`: emit `<#ty as anchor_lang::Id>::id()` for rule (a); emit the
  user expression for `inject` and `default`.
- trybuild tests: `Program<CustomType>` injection, `inject` on an
  `UncheckedAccount`, `default` on a bundle field, and the precedence case
  (explicit `#[bundle(...)]` on a `Program<T>` field beats the rule).

HEAD first, compat backport, parity ledger row; the usual loop.

## Open questions

1. **Should `inject` and `default` take arbitrary expressions or just paths?**
   Arbitrary expressions are more powerful and no harder to emit (the derive
   pastes tokens); the risk is unreadable bundles. Leaning toward arbitrary,
   documented with restraint.
2. **Should the `Interface` opinion be overridable per field?**
   `#[bundle(inject = anchor_spl::token_2022::ID)]` on an
   `Interface<TokenInterface>` field already covers it once the hatch exists;
   no extra mechanism needed. Worth a doc example, nothing more.
3. **Does anything need the reverse: a `Program<T>` field that should project
   from the bundle?** The precedence rule (explicit attribute wins) covers it:
   `#[bundle(project)]` could force projection. Defer until a real consumer
   needs it; do not build it speculatively.
