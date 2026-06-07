# Integrating `anchor-litesvm` (`class/ask` branch)

`class/ask` is the dogfood branch. The shape is stable enough to use in your tests; the API may shift before merge. Below is the integration path end to end, plus the gotchas that surfaced during real ports.

## What you get

- `AnchorLiteSVM::build_with_program(id, name, bytes)`: one-line setup, returns an `AnchorContext`. The `name` registers as a pubkey alias so structured logs read `<name>::<ix>`.
- `#[derive(Bundle)]` + `#[derive(BundledPubkeys)] + #[bundled_with(...)]`: proc-macro pair that emits `From<Bundle> for accounts::*` and `BuildableIx<Bundle> for instruction::*`. Build instructions with `ctx.program().build_ix(bundle, args)`.
- Send + assert shortcuts come in two flavours, both returning the same fluent `TransactionResult`:
  - **Context-owned aliases (recommended for Anchor programs)**: `ctx.alias(pk, "name")` accumulates into a context-internal `Aliases` table; `ctx.send_ok(ix, &[signer])` / `ctx.send_err_named(ix, &[signer], "ErrorName")` read from that table without any per-call `&Aliases` argument. The returned `TransactionResult` carries the table internally, so a chained `.print_logs_structured()` (no arg) just works.
  - **Bare LiteSVM (non-Anchor or external-table cases)**: `ctx.svm.send_ok(ix, &[signer], &aliases)` / `ctx.svm.send_err_named(ix, &[signer], &aliases, "ErrorName")` thread an external `Aliases` reference per call; same result type, same fluent chain.
- The fluent surface on `TransactionResult`: `.assert_success()` / `.assert_failure()` / `.assert_success_with(|r| ...)` / `.assert_failure_with(|r| ...)` / `.assert_error("Name")` / `.assert_error_code(6000)` / `.print_logs_structured()` / `.with_aliases(table)` / `.tap(|r| ...)` consume and re-emit `self`, so the chain ends in an owned binding you can keep using. Read-only data methods (`compute_units`, `error`, `logs`, `has_log`, `fee`) stay `&self -> T` and compose through `tap`, which borrows for the closure and hands ownership back.
- The alias table drives the structured CPI tree: per-frame `signer=X` on top-level instructions, friendly names for well-known programs (System, Token, AssociatedToken, etc.), and Solscan-style truncation (`<8>…<4>`) for unaliased pubkeys. `Aliases::default()` ships the well-known set; extend with `.with(pubkey, "name")` (consuming builder, good for the seed) or `Aliases::add(&mut self, pubkey, "name")` (in-place, good for scenario tables that grow).
- `TestHelpers` (on `LiteSVM`): `create_funded_account`, `create_token_mint`, `create_associated_token_account`, `mint_to`, `token_balance(&ata) -> Option<u64>`, time control (`warp_to_timestamp`, `advance_seconds`, ...).
- `ctx.load::<T>(&addr)` / `load_unchecked` for Anchor account reads. Panics with the address and underlying `AccountError` on missing/malformed.

## 1. Add the dep

In `programs/<your_program>/Cargo.toml`:

```toml
[target.'cfg(not(target_os = "solana"))'.dependencies]
anchor-litesvm = { git = "https://github.com/cds-rs/anchor-litesvm", branch = "class/ask" }
```

The `target.cfg` keeps it out of the BPF build; it's only present when you `cargo test` on the host.

> **`class/ask` may rebase.** If `cargo build` suddenly fails with `error: failed to fetch ... object not found in the database`, your `Cargo.lock` is pinned to a commit that's been squashed away. Fix with `cargo update -p anchor-litesvm` (surgical) or `rm Cargo.lock && cargo build` (nuclear).

> **Match your `litesvm-token` version.** The framework currently pins `litesvm = "0.11"`. If your `[dev-dependencies]` has `litesvm-token = "0.10"`, you'll get two versions of `LiteSVM` in scope and the types won't cross-call. Bump to `litesvm-token = "0.11"`.

## 2. Define a bundle, add the derive

Pick one `#[derive(Accounts)]` struct. Add the derive + bundle attribute, gated for non-Solana:

```rust
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(crate::test_helpers::MyBundle),
)]
#[derive(Accounts)]
pub struct Make<'info> { /* ... */ }
```

Define the bundle in a host-only module (e.g. `src/test_helpers.rs`):

```rust
#[cfg(not(target_os = "solana"))]
pub mod test_helpers {
    use anchor_lang::prelude::Pubkey;
    use anchor_litesvm::Bundle;

    #[derive(Bundle, Copy, Clone, Debug)]
    pub struct MyBundle {
        pub maker: Pubkey,
        pub mint_a: Pubkey,
        // every account in the struct that isn't Program<System>,
        // Program<AssociatedToken>, or Interface<TokenInterface>
    }
}
```

The derive auto-injects canonical IDs for those three program types, so don't put them in the bundle.

> **Don't add `Default` to the derive list.** `#[derive(Bundle)]` already emits one (every field gets `Pubkey::new_unique()`). Adding `Default` manually is a duplicate-impl error.

> **`pub` your state fields.** If your `#[account]` struct has private fields (e.g. `vault_bump: u8`), `ctx.load::<T>(&pda)` deserializes fine but you can't `.field` your way to assertions. Promote to `pub` (or add accessors) before writing post-state checks.

## 3. Write a test

```rust
use anchor_litesvm::{Aliases, AnchorLiteSVM, TestHelpers, TransactionHelpers};
use my_program::test_helpers::MyBundle;
use my_program::{instruction, MyState, ID};

#[test]
fn make_works() {
    let mut ctx = AnchorLiteSVM::build_with_program(
        ID,
        "my_program",
        include_bytes!("../../../target/deploy/my_program.so"),
    );
    let maker = ctx.svm.create_funded_account(10_000_000_000).unwrap();
    // ... mints, ATAs, PDAs ...

    let bundle = MyBundle { maker: maker.pubkey(), /* ... */ };
    let ix = ctx.program().build_ix(bundle, instruction::Make { /* args */ });

    // Register actor names on the context; the alias table is read
    // implicitly by ctx.send_ok / ctx.send_err_named below.
    ctx.alias(maker.pubkey(), "maker");

    // send_ok already asserts success; the chain below layers a compute-unit
    // bound (assert_success_with) and prints the structured tree on the way out.
    // print_logs_structured() takes no argument: ctx.send_ok stashed the alias
    // table on the returned result.
    ctx.send_ok(ix, &[&maker])
        .assert_success_with(|r| r.compute_units() < 200_000)
        .print_logs_structured();

    // Closed accounts read as `None`; existing-but-empty as `Some(0)`.
    assert_eq!(ctx.svm.token_balance(&vault), Some(deposit_amount));

    // `ctx.load` panics with the address + AccountError if missing/malformed.
    let state: MyState = ctx.load(&state_pda);
    assert_eq!(state.expiry_utc, Some(expiry));
}

#[test]
fn make_rejects_past_expiry() {
    // ... setup ...
    let ix = ctx.program().build_ix(bundle, instruction::Make { expiry_utc: Some(past), /* ... */ });
    // send_err_named asserts the failure mode and prints the CPI tree on
    // assertion-failure paths (wrong error / unexpected success). Returns
    // the wrapped result so further inspection can chain.
    ctx.send_err_named(ix, &[&maker], "ExpirationDateTooOld")
        .print_logs_structured();
}
```

Run with `cargo test -- --nocapture` to see the structured tree. The error-name argument matches the Anchor variant name as it's emitted in logs (substring match against logs + error field).

## Known sharp edges

**Blockhash isn't auto-expired between `send_ok` / `send_err_named` calls.** Two transactions with identical (ix-data, accounts, signer, blockhash) fail the second time with `AlreadyProcessed`. If your test deliberately submits the same ix twice (e.g. "this op failed while locked, then succeeds after unlock"), call `ctx.svm.expire_blockhash()` between them.

**`mint_supply` has no helper.** `token_balance` reads SPL Token *accounts*; for Mint *supply* you'll roll your own:

```rust
pub fn mint_supply(ctx: &AnchorContext, mint: &Pubkey) -> u64 {
    let acc = ctx.svm.get_account(mint).unwrap();
    spl_token::state::Mint::unpack(&acc.data).unwrap().supply
}
```

**`Interface<TokenInterface>` always injects classic SPL Token's program ID.** If you need Token-2022 mint behavior (transfer fees, transfer hooks, etc.) tested, you can't get there through the framework today; the mint-creation helpers are classic-only.

**Auto-inject is purely textual on the last type segment.** `Program<'info, System>`, `Program<'info, AssociatedToken>`, `Interface<'info, TokenInterface>` are recognised. `Box<Program<System>>` projects from the bundle (because the head is `Box`); add a `system_program: Pubkey` field if you wrap.

## Discovering the rest

```bash
cargo doc --open -p anchor-litesvm
```

Or read the trait sources; every method has a worked doc example:
- `crates/litesvm-utils/src/transaction.rs`: `TransactionHelpers`, `TransactionResult`
- `crates/litesvm-utils/src/test_helpers.rs`: `TestHelpers` (creation, reads, time)
- `docs/design/structured-logs.md`: end-to-end design of the renderer + the ergonomic decisions behind the fluent chain (consume-self, `tap` bridge, `_with` predicate variants).

## Feedback wanted

If you port a suite, please report:

1. Did the derive compile? Paste any error.
2. Field-name mismatches you hit (your bundle calls it `state`, the Anchor struct calls it `vault_state`).
3. Accounts that should have been auto-injected but weren't. (Currently: only `Program<System>`, `Program<AssociatedToken>`, `Interface<TokenInterface>`.)
4. Things you wished the bundle could derive (PDA addresses, ATA addresses, signer creation, ...).
5. Was the annotated CPI tree useful? Tree screenshots of a fail-path are gold. In particular: do the per-frame `signer=...` rows pull their weight, and did you find a use for extending `Aliases` to name your own actors/programs, or did the well-known defaults plus pubkey truncation suffice?
6. Did `send_ok` / `send_err_named` / `token_balance` / `ctx.load` cover your idioms, or did you end up writing a local wrapper anyway?

Open issues on `cds-rs/anchor-litesvm` or DM.
