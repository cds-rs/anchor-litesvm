# Stake

<details>
<summary>Your starting point</summary>

The staking program's full source, a standard Anchor program with no tests, at
`examples/staking/`. It CPIs into mpl-core, so that program's `.so` is committed
alongside. The built fixtures are committed too, so a fresh clone runs this
chapter's test without building anything:

```bash
git clone -b feat/buildable-ix https://github.com/cds-rs/anchor-litesvm
cd anchor-litesvm
cargo test -p anchor-litesvm --test book_stake
```

```text
examples/staking/                                the program source (no tests)
crates/anchor-litesvm/tests/fixtures/staking.so  the built program
crates/anchor-litesvm/tests/fixtures/mpl_core.so the mpl-core CPI callee
crates/anchor-litesvm/tests/book_stake.rs        this chapter's test
```

Changed the program? Rebuild the fixture with `cd examples/staking && anchor build`.

</details>

The staking program lets a holder stake an mpl-core NFT into a collection
and earn rewards. `create_collection` and `mint_asset` set up the NFT side
of things; `initialize` opens a `config` PDA on the collection with a
rewards rate and a freeze period, in days; `stake` freezes an asset in place
and records when; `unstake`, once that freeze period elapses, unfreezes it
again and mints the rewards.

This is the deepest CPI tree in the book: mpl-core assets are only mutable
through CPIs into the mpl-core program itself, so nearly everything `stake`
and `unstake` do to the NFT shows up as a nested frame rather than as a
direct account write in `staking`'s own frame.

The program depends on `mpl-core`, which is pinned to anchor 0.31. The host
workspace here is anchor 1.0, and that mismatch is exactly why the
program's IDL can't feed `declare_program!` or `bundles_from_idl!` the way
vault's and escrow's do: those macros generate code against the workspace's
own anchor 1.0 typings, and an anchor 0.31 IDL doesn't speak that dialect.

So this chapter drives the program with raw `solana_instruction::Instruction`s
instead, hand-built the same way those macros build them internally when
they can. Think of this as the escape hatch at the whole-program level:
where vault's and escrow's escape hatches swap a single account inside an
otherwise-generated instruction, here nothing is generated at all, and the
framework supports that for any program, IDL or not.

## The raw client

Every Anchor instruction's data starts with an 8-byte discriminator: a
hash-derived tag identifying which instruction the rest of the bytes belong
to, since the wire format carries no instruction name, just data.
`bundles_from_idl!` normally computes this for you, reading it straight off
the IDL. With no IDL here, `disc` computes it directly instead:

```rust
// crates/anchor-litesvm/tests/book_stake.rs
/// Anchor 0.31 8-byte instruction discriminator: `sha256("global:<name>")[..8]`.
fn disc(name: &str) -> [u8; 8] {
    let h = Sha256::digest(format!("global:{name}").as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h[..8]);
    d
}
```

```rust
// crates/anchor-litesvm/tests/book_stake.rs
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

`disc` computes that discriminator with the exact formula Anchor itself
uses, `sha256("global:<instruction_name>")` truncated to its first 8 bytes,
so it's the same value a generated IDL would have handed you, just computed
instead of looked up.

The account metas in `ix_stake` mirror the program's `#[derive(Accounts)]`
struct for `Stake`, field for field, in the same order the struct declares
them. Get that order wrong and the program reads the wrong account into the
wrong slot: `Instruction`'s `accounts` field is just a positional list, with
no name attached to catch a mistake the way a bundle's named struct fields
do. Each PDA, `config` and the update-authority account `ua`, is derived by
hand with `find_program_address`, the same derivation `bundles_from_idl!`
would generate for you if this program's IDL could feed it.

## Two-program boot

```rust
// crates/anchor-litesvm/tests/book_stake.rs
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

`build_with_programs` deploys the staking program alongside `mpl_core`:
staking CPIs into it for every NFT operation, so both programs need to be
live on the SVM for any of this to run.

With no IDL to source error names from, `register_program_errors` supplies
the mapping by hand, read straight off staking's own `error.rs`. That's what
turns a failing leaf into `FreezePeriodNotElapsed` instead of the far less
readable `custom program error: 0x1770`.

## Happy path

```rust
// crates/anchor-litesvm/tests/book_stake.rs
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

`create_collection` mints a fresh mpl-core collection asset: think of it as
the container `stake` will later attach individual NFTs to. `initialize`
opens the `config` PDA on that collection with a 500bps rewards rate and a
7-day freeze period. `mint_asset` mints an NFT into the collection, and
`stake` freezes it in place.

`result.tree_string()` renders the last of those four calls, `stake`:

```text
{{#include ../captured/stake.txt}}
```

`staking::Stake` CPIs into `mpl_core::AddPlugin` twice, once per plugin it
attaches: first the Attributes plugin, which records `staked` and
`staked_at` as data on the asset, then the FreezeDelegate plugin, which is
what actually freezes the asset in place. Both effects show up as nested
frames rather than as writes inside `staking`'s own frame, because that's
the only way `staking` is allowed to touch someone else's mpl-core asset.

Each `AddPlugin` call, in turn, touches `System` to resize the asset
account: attaching a plugin grows the account's stored data, and the extra
rent that growth requires gets funded through a `System` transfer CPI.

## Freeze lock

A staker who tries to unstake before the freeze period elapses gets turned
away. `unstake` reads the current clock, works out how many days have
passed since `stake` recorded `staked_at`, and requires that count to reach
`initialize`'s 7-day freeze period before it will touch the asset at all:

```rust
// crates/anchor-litesvm/tests/book_stake.rs
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

The `AssociatedToken` frame still runs and succeeds: `unstake` creates the
staker's rewards ATA before it ever checks the freeze period, the same
create-first, guard-second ordering the escrow chapter's expiry check
showed. Then the `✗ FreezePeriodNotElapsed` leaf stops the transaction,
with 6 of the 7 freeze-period days still owed.

Give `unstake` the days it's owed, and the very same call succeeds:

```rust
// crates/anchor-litesvm/tests/book_stake.rs
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

Past the freeze period, `unstake` runs to completion. The first
`mpl_core::UpdatePlugin` call resets the Attributes plugin's `staked` /
`staked_at` values, undoing what `stake`'s first `AddPlugin` call recorded.
The second `UpdatePlugin` call sets `FreezeDelegate.frozen` back to `false`,
unfreezing the asset. The final `Token` call is the payoff: it mints the
staking rewards to the staker's ATA, at the rewards rate `initialize` set
back at the start of the chapter.

The full test is `crates/anchor-litesvm/tests/book_stake.rs`.
