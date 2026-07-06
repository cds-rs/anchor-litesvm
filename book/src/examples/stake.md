# Stake

The staking program lets a holder stake an mpl-core NFT into a collection
and earn rewards: `create_collection` and `mint_asset` set up the NFT side,
`initialize` opens a `config` PDA on the collection with a rewards rate and a
freeze period (in days), `stake` freezes an asset in place and records when,
and `unstake` (after the freeze period elapses) unfreezes it and mints
rewards. This is the deepest CPI tree in the book.

The program depends on `mpl-core`, which is pinned to anchor 0.31; the host
workspace is anchor 1.0, so the program's IDL can't feed `declare_program!` or
`bundles_from_idl!` the way vault's and escrow's do. Instead this chapter
drives it with raw `solana_instruction::Instruction`s, hand-built the same way
those macros build them internally. It's the escape hatch at the
whole-program level, and the framework supports it for any program, IDL or
not.

## The raw client

```rust
/// Anchor 0.31 8-byte instruction discriminator: `sha256("global:<name>")[..8]`.
fn disc(name: &str) -> [u8; 8] {
    let h = Sha256::digest(format!("global:{name}").as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h[..8]);
    d
}
```

```rust
/// Mirrors `instructions/stake.rs::Stake`.
fn ix_stake(owner: &Pubkey, asset: &Pubkey, collection: &Pubkey) -> Instruction {
    let (config, _) = config_pda(collection);
    let (ua, _) = update_authority_pda(collection);
    Instruction {
        program_id: STAKING_ID,
        accounts: vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(config, false),
            AccountMeta::new(*asset, false),
            AccountMeta::new(*collection, false),
            AccountMeta::new_readonly(ua, false),
            AccountMeta::new_readonly(SYSTEM_ID, false),
            AccountMeta::new_readonly(MPL_CORE_ID, false),
        ],
        data: disc("stake").to_vec(),
    }
}
```

An anchor discriminator is always `sha256("global:<instruction_name>")`,
truncated to its first 8 bytes; `disc` computes it directly instead of
reading it off a generated IDL. The account metas mirror the program's
`#[derive(Accounts)]` struct field-for-field, in the same order, each PDA
derived by hand with `find_program_address`. This is exactly what
`bundles_from_idl!` generates for you when an IDL is available; here you
write it out.

## Two-program boot

```rust
/// Deploys both vendored programs and names the staking custom errors (no
/// IDL for this anchor-0.31 program, so `register_program_errors` is the
/// only way a failing leaf reads as `InvalidOwner` instead of `custom
/// program error: 0x1770`). Codes are declaration order from 6000, per
/// `error.rs`.
fn boot() -> anchor_litesvm::AnchorContext {
    let mut ctx = AnchorLiteSVM::build_with_programs(&[
        (STAKING_ID, "staking", &common::fixture_bytes("staking")),
        (MPL_CORE_ID, "mpl_core", &common::fixture_bytes("mpl_core")),
    ]);
    ctx.register_program_errors(
        STAKING_ID,
        &[
            (6000, "InvalidOwner"),
            (6001, "InvalidUpdateAuthority"),
            (6002, "AlreadyStaked"),
            (6003, "AssetNotStaked"),
            (6004, "InvalidTimestamp"),
            (6005, "FreezePeriodNotElapsed"),
            (6006, "InvalidRewardsBps"),
            (6007, "NothingToClaim"),
        ],
    );
    ctx
}
```

`build_with_programs` deploys the staking program alongside `mpl_core`, since
staking CPIs into it for every NFT operation. With no IDL to source error
names from, `register_program_errors` supplies the mapping by hand so a
failing leaf renders as `FreezePeriodNotElapsed` instead of `custom program
error: 0x1770`.

## Happy path

```rust
let mut ctx = boot();
let admin = ctx.cast_actor("Alice");

let collection = deterministic_keypair(&STAKING_ID.to_string(), "Collection");
let asset = deterministic_keypair(&STAKING_ID.to_string(), "Asset");
ctx.alias(collection.pubkey(), "Collection");
ctx.alias(asset.pubkey(), "Asset");

ctx.send_ok(
    ix_create_collection(
        &admin.pubkey(),
        &collection.pubkey(),
        "Stake Collection",
        "https://example.com/collection.json",
    ),
    &[&admin, &collection],
);
ctx.send_ok(
    ix_initialize(&admin.pubkey(), &collection.pubkey(), 500, 7),
    &[&admin],
);
ctx.send_ok(
    ix_mint_asset(
        &admin.pubkey(),
        &asset.pubkey(),
        &collection.pubkey(),
        "Stake Asset",
        "https://example.com/asset.json",
    ),
    &[&admin, &asset],
);

let result = ctx.send_ok(
    ix_stake(&admin.pubkey(), &asset.pubkey(), &collection.pubkey()),
    &[&admin],
);
```

`create_collection` mints a fresh mpl-core collection asset, `initialize`
opens the `config` PDA on it with a 500bps rewards rate and a 7-day freeze
period, `mint_asset` mints an NFT into that collection, and `stake` freezes
it. `result.tree_string()` renders the last call:

```text
{{#include ../captured/stake.txt}}
```

`staking::Stake` CPIs into `mpl_core::AddPlugin` twice: once to attach the
Attributes plugin (recording `staked` and `staked_at`), once for the
FreezeDelegate plugin (freezing the asset in place). Each `AddPlugin` in turn
touches `System` to resize the asset account.

## Freeze lock

A staker who tries to unstake before the freeze period elapses is turned
away. `unstake` reads the clock, computes the staked days, and requires them
to reach `initialize`'s 7-day freeze period before it will touch the asset:

```rust
// Only 1 of the 7 freeze-period days has elapsed.
ctx.svm.advance_days(1);
let result = ctx.send_err_named(
    ix_unstake(&admin.pubkey(), &asset.pubkey(), &collection.pubkey()),
    &[&admin],
    "FreezePeriodNotElapsed",
);
```

```text
{{#include ../captured/stake_freeze_locked.txt}}
```

The `AssociatedToken` frame still runs and succeeds (it creates the rewards
ATA before the freeze check), then the `✗ FreezePeriodNotElapsed` leaf stops
the transaction with 6 of the 7 days still owed.

Give it the days it's owed and the same call succeeds:

```rust
// 8 of the 7 freeze-period days have elapsed.
ctx.svm.advance_days(8);
let result = ctx.send_ok(
    ix_unstake(&admin.pubkey(), &asset.pubkey(), &collection.pubkey()),
    &[&admin],
);
```

```text
{{#include ../captured/stake_unstake_ok.txt}}
```

Past the freeze period, `unstake` runs to completion: the first
`mpl_core::UpdatePlugin` call resets the Attributes plugin's `staked` /
`staked_at` values, the second sets `FreezeDelegate.frozen` back to `false`,
and the final `Token` call mints the staking rewards to the staker's ATA.

The full test is `crates/anchor-litesvm/tests/book_stake.rs`.
