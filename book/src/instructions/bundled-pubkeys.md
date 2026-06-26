# Bundled Pubkeys (the derive)

You've been using this derive since your [first test](../intro/first-test.md): the `#[cfg_attr(not(target_os = "solana"), derive(BundledPubkeys), bundled_with(...))]` line on the accounts struct, and the `#[derive(Bundle)]` struct it points at. This chapter is the full account of what that machinery does, the abstract picture first, then the corners (auto-injection, the naming precondition, optionality fixups, the rest of the family).

> This is the teaching tour. The *why* (the exact impls the macro emits, how span propagation gives good errors, the avenues abandoned) lives in [`docs/design/bundled-pubkeys.md`](https://github.com/cds-rs/anchor-litesvm/blob/turbin3/docs/design/bundled-pubkeys.md). Same subject, deeper altitude.

> **Pinocchio:** the whole bundle-derive idea is Anchor-shaped. It bridges your test bundle to the `accounts::Make` / `instruction::Make` types Anchor *generates*; Pinocchio generates no such namespaces, so there is nothing to project into and no `BundledPubkeys`. Pinocchio's derive, `#[derive(Discriminator)]` on the instruction enum, does a different job: it generates the dispatch discriminators and the host-side instruction names the renderers read. A Pinocchio test builds the raw `Instruction` directly rather than projecting a bundle. See [Testing Pinocchio Programs](../appendix/pinocchio.md).

## What the derive actually does

Your `#[derive(Accounts)]` struct is the instruction's on-chain context: the account set it needs ([Named Accounts](named-accounts.md) covers why that set is the instruction's identity). What the bundle derive leans on is what Anchor files *alongside* it. From the `#[program]` handler, Anchor generates two parallel namespaces, both filed under the instruction's name (here, `Make`):

- `accounts::Make` — the account list a client passes as metas.
- `instruction::Make` — the handler's args a client passes as data.

`BundledPubkeys` reuses that name. You write **one** derive, on **one** struct, and from that single site it emits **two** bridges, one to each namespace:

```text
                                        #[derive(BundledPubkeys)] on `Make`
                                                         │
                                              reuses the name "Make"
                                                         │
                                 ┌───────────────────────┴────────────────────────┐
                                 ▼                                                ▼
                          accounts::Make                                  instruction::Make
                        (the account list)                               (the handler args)
                                 │                                                │
                                 ▼                                                ▼
               impl From<Bundle> for accounts::Make        impl BuildableIx<Bundle> for instruction::Make
```

The crux is that middle line: the derive doesn't read your struct's *fields* to know what to wire up, it reuses your struct's *name*. It looks `Make` up in both namespaces and bridges your bundle to each, a `From<Bundle>` so the bundle becomes the account metas, and a `BuildableIx<Bundle>` so `build_ix` knows which args pair with which bundle. Name-based resolution across two generated namespaces, which is also why the [one precondition below](#the-one-precondition-that-bites) is about names.

## Two properties of the projection

The bundle names only the pubkeys your test varies; the derive fills in the rest:

```rust
let accs = EscrowBundle { maker: maker.pubkey(), mint_a, vault, escrow };
let ix = ctx.program().build_ix(accs, vix::Make { amount: 1_000 });
```

Two properties follow from this.

**Program IDs are auto-injected.** Fields typed `Program<'_, System>`, `Program<'_, AssociatedToken>`, or `Interface<'_, TokenInterface>` each have exactly one canonical pubkey, so the bundle doesn't carry them and your test doesn't spell them; the derive reads the field's Anchor type and fills the id. The bundle holds only the pubkeys you actually vary. (`Program<'_, Token>` is *not* in that set; if you're on the classic token program, declare it as `Interface<TokenInterface>`, which the derive recognizes and which still resolves to the SPL Token id.)

**Mismatches are compile errors.** Because the derive emits `BuildableIx<Bundle>` for exactly one args type (the `instruction::Make` it found by name), `build_ix` won't accept the wrong pairing. Passing `Withdraw` args with a `Deposit` bundle simply doesn't typecheck; the error lands at the call site at compile time, not as a malformed instruction at runtime.

## The bundle holds pubkeys, not signers

Every field in a bundle is a `Pubkey`. The bundle is the account-list view of an instruction, and projecting it into the metas is all `BundledPubkeys` does. Signing is a separate concern: `ctx.tx(&[&signer])` takes `Keypair`s, and a pubkey can't sign. So a scenario keeps the signer keypairs alongside the bundle, one carrying the secret key and one carrying the account meta:

```rust
pub struct Scenario {
    pub accs: TransferBundle,  // accs.authority : Pubkey   (build the ix)
    pub alice: Keypair,        // alice          : Keypair  (sign the tx)
}
```

Both come from the same identity, so `alice.pubkey() == accs.authority` by construction. Build with the bundle, sign with the keypair.

## The one precondition that bites

Since the derive resolves its two targets by name (the directory lookup above), the precondition follows directly: **your accounts struct's name must equal `PascalCase` of the handler function name.** Anchor files the args type under `PascalCase(handler_fn_name)`, so that's the entry the derive will look for in the `instruction` directory.

The common case satisfies this by construction (you name the `Accounts` struct after the instruction). It breaks when a short struct pairs with a longer handler: `fn initialize_poll` generates `instruction::InitializePoll`, but if your struct is `InitPoll`, the derive looks up `instruction::InitPoll`, which doesn't exist. You'll get an `E0425: cannot find type ... in module crate::instruction`, pointed at the struct definition.

The fix is to name the entry explicitly, overriding the inferred lookup:

```rust
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(
        InitPollBundle,
        instruction = crate::instruction::InitializePoll,  // override the inferred path
    )
)]
#[derive(Accounts)]
pub struct InitPoll<'info> { /* ... */ }
```

(`accounts = ...` is accepted for symmetry, but rarely needed; Anchor pulls that name from `Context<Foo>`, so it usually matches.)

## Optionality fixups

One bundle is often shared across several account structs that disagree on whether a field is optional. Two per-field attributes bridge the gap during projection:

- `#[bundle(unwrap)]`: an `Option<T>` bundle field projects into a bare `T` account field, panicking with a pointed message if the bundle left it `None`.
- `#[bundle(wrap_some)]`: a bare `T` bundle field projects into an `Option<T>` account field.

Without an annotation, a type mismatch is a plain compile error, which is the right default: coercing optionality is exactly the kind of thing you want to opt into explicitly.

## The gotcha that produces confusing errors

**Keep `bundled_with(...)` inside the same `cfg_attr` as the derive.** Pulled out into a bare `#[bundled_with(...)]`, it either fires without the derive present or reads as an unknown attribute under the on-chain target, and the symptom is an ugly lifetime-and-privacy error cascade that looks nothing like the real cause. The canonical form is the combined `cfg_attr` shown throughout: derive and its configuration gated together, off for BPF.

## The rest of the family

`BundledPubkeys` is the core; three smaller derives round it out (each its own concern, here for orientation):

- **`Bundle`**: a `Default` impl that fills every `Pubkey` field with `Pubkey::new_unique()`, so a bundle is ready to populate from setup, binding only the keys you care about with `..Bundle::default()` and letting the rest fall to throwaway placeholders.
- **`BundleFrom`**: projects a bundle from several source structs, for tests that assemble pubkeys from multiple actor objects (`SwapBundle::from((&pool, &user))`).
- **`AliasMirror`**: wires a struct's fields into the [`Aliases`](../running/accounts-as-actors.md) table in one shot, so the bundle you build *is* the cast your rendered output reads in.

`AliasMirror` closes the loop: the struct that names your accounts for building also names them for the [rendering views](../inspect/cpi-tree.md), so the cast list and the instruction inputs stay one source of truth.
