# anchor-litesvm

**Simplified Anchor testing with LiteSVM**: bundle-based instruction building, far less code, no mock RPC needed.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

> Part of the `compat/anchor-0.31` LTS branch, distributed via git only (not crates.io).

> A fork of [anchor-litesvm](https://github.com/brimigs/anchor-litesvm) by [@brimigs](https://github.com/brimigs).

## Overview

`anchor-litesvm` provides a streamlined testing experience for Anchor programs. It pairs bundle-based instruction building with the speed of LiteSVM, plus comprehensive testing utilities.

**Key Benefits:**
- **Far less code** than raw LiteSVM
- **Fast compilation**: no network dependencies
- **No mock RPC**: zero configuration needed
- **Named bundles**: order-independent account building, checked at compile time

## Installation

```toml
# Host-only: the test machinery, never compiled into the on-chain binary.
[target.'cfg(not(target_os = "solana"))'.dependencies]
anchor-litesvm = { git = "https://github.com/cds-rs/anchor-litesvm", branch = "compat/anchor-0.31" }
```

## Quick Start

```rust
use anchor_litesvm::{AnchorLiteSVM, TestHelpers};
use solana_sdk::signer::Signer;
use my_program::{instruction as vix, test_helpers::InitializeBundle};

#[test]
fn test_my_anchor_program() {
    // 1. One-line setup, no mock RPC. The name registers as a pubkey alias so
    //    structured logs read `my_program::Initialize`, not the raw program id.
    let mut ctx = AnchorLiteSVM::build_with_program(
        my_program::ID,
        "my_program",
        include_bytes!("../target/deploy/my_program.so"),
    );

    // 2. Create accounts with built-in helpers
    let user = ctx.svm.create_funded_account(10_000_000_000).unwrap();
    let mint = ctx.svm.create_token_mint(&user, 9).unwrap();

    // 3. Build + send + assert in one chain. The bundle names the accounts; the
    //    BundledPubkeys derive on the program orders them (no Vec<AccountMeta>,
    //    no client codegen), and canonical program ids are auto-injected.
    ctx.tx(&[&user])
        .build(
            InitializeBundle { user: user.pubkey(), mint: mint.pubkey() },
            vix::Initialize { amount: 1_000_000 },
        )
        .send_ok(); // builds, sends, asserts success

    // 4. Read an Anchor account back, discriminator-checked.
    let account: MyAccount = ctx.try_load(&pda).unwrap();
}
```

`InitializeBundle` is the one-time program-side setup shown in
[Features](#one-call-instruction-building-with-bundledpubkeys) below.

## Why anchor-litesvm?

| Feature | anchor-client + LiteSVM | anchor-litesvm |
|---------|------------------------|----------------|
| Lines of Code | 279 lines | **106 lines** |
| Compilation | Slow (network deps) | **40% faster** |
| Setup | Mock RPC needed | **No config** |
| Token Operations | Manual (30+ lines) | **1 line** |
| Syntax | anchor-client | **Similar** |

### No More Account Ordering Bugs

The #1 pain point in Solana testing, eliminated:

```rust
// Raw LiteSVM: order matters, easy to get wrong
let instruction = Instruction {
    accounts: vec![
        AccountMeta::new(maker.pubkey(), true),   // Must be position 0
        AccountMeta::new(escrow_pda, false),       // Must be position 1
        // Wrong order = silent failure or wrong accounts
    ],
    ..
};

// anchor-litesvm: a named bundle, order doesn't matter
ctx.tx(&[&maker])
    .build(
        MakeBundle { escrow: escrow_pda, maker: maker.pubkey() }, // any order
        vix::Make { seed, amount },
    )
    .send_ok();
```

## Features

### One-Call Instruction Building with `BundledPubkeys`

A **bundle** is a small struct of the pubkeys a test varies. The `BundledPubkeys` derive projects it into the program's `accounts::*` list and pairs it with the `instruction::*` args at compile time, so `ctx.tx(..).build(bundle, args)` (or `ctx.program().build_ix(bundle, args)`) replaces the `.accounts().args().instruction()` chain. The pairing is type-checked: passing `Deposit` args with a `Withdraw` bundle is a compile error, not a runtime failure.

One-time setup in your program crate. The bundle is host-only, and the `cfg_attr` gates the derive off the on-chain build:

```rust
// src/test_helpers.rs
#[derive(Copy, Clone, anchor_litesvm::Bundle)]
pub struct DepositBundle {
    pub user: Pubkey,
    pub vault_state: Pubkey,
    pub vault: Pubkey,
}
```

```rust
// on the instruction's #[derive(Accounts)] struct
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(crate::test_helpers::DepositBundle),
)]
#[derive(Accounts)]
pub struct Deposit<'info> { /* ... */ }
```

Canonical program ids (`Program<System>`, `Program<AssociatedToken>`, `Interface<TokenInterface>`) are auto-injected, so the bundle carries only the pubkeys you vary.

In your tests:

```rust
let bundle = DepositBundle { user, vault_state, vault };

// Happy path: build + send + assert in one chain.
ctx.tx(&[&user]).build(bundle, vix::Deposit { amount: 1_000_000 }).send_ok();

// Or get the raw Instruction:
let ix = ctx.program().build_ix(bundle, vix::Deposit { amount: 1_000_000 });

// Negative path: inject a deliberately-wrong account via a closure.
ctx.tx(&[&user])
    .build_with(bundle, vix::Deposit { amount: 1_000_000 }, |a| a.vault_state = wrong_pda)
    .send_err_named("ConstraintSeeds");
```

A bundle can also be projected from several fixtures with `#[derive(BundleFrom)]` (a pool plus a user, say), and `#[derive(Bundle)]` gives it a `Default` so a test binds only the fields it varies (`..DepositBundle::default()`). See [`EVALUATING.md`](../../EVALUATING.md) for the full tour.

### Manual Instruction Building (escape hatch)

The `.accounts(...).args(...).instruction()` chain stays available for full control over the accounts struct, or for a one-off test that skips the bundle setup:

```rust
let ix = ctx.program()
    .accounts(my_program::accounts::Transfer {
        from: from_account,
        to: to_account,
        authority: user.pubkey(),
    })
    .args(vix::Transfer { amount: 100 })
    .instruction()?;

ctx.execute_instruction(ix, &[&user])?.assert_success();
```

### Anchor Account Deserialization

```rust
// Deserialize with discriminator check
let account: MyAccount = ctx.try_load(&pda)?;

// Deserialize without check (for PDAs with custom layouts)
let account: MyAccount = ctx.try_load_unchecked(&pda)?;
```

### Event Parsing

```rust
use anchor_litesvm::EventHelpers;

let result = ctx.execute_instruction(ix, &[&user])?;

// Parse all events of a type
let events: Vec<TransferEvent> = result.parse_events()?;

// Parse first event
let event: TransferEvent = result.parse_event()?;

// Check if event was emitted
assert!(result.has_event::<TransferEvent>());
result.assert_event_emitted::<TransferEvent>();
```

### All litesvm-utils Features Included

Since `anchor-litesvm` builds on `litesvm-utils`, you get all utilities:

```rust
// Account creation
let user = ctx.svm.create_funded_account(10_000_000_000)?;

// Token operations
let mint = ctx.svm.create_token_mint(&user, 9)?;
let ata = ctx.svm.create_associated_token_account(&mint.pubkey(), &user)?;
ctx.svm.mint_to(&mint.pubkey(), &ata, &user, 1_000_000)?;

// Assertions
ctx.svm.assert_token_balance(&ata, 1_000_000);
ctx.svm.assert_account_exists(&pda);

// PDA derivation
let (pda, bump) = ctx.svm.get_pda_with_bump(&[b"seed"], &program_id);

// Transaction analysis
result.print_logs();
let cu = result.compute_units();
```

## Common Patterns

### Token Testing

```rust
let mint = ctx.svm.create_token_mint(&authority, 9)?;
let token_account = ctx.svm.create_associated_token_account(&mint.pubkey(), &owner)?;
ctx.svm.mint_to(&mint.pubkey(), &token_account, &authority, 1_000_000)?;

// After your instruction
ctx.svm.assert_token_balance(&token_account, expected_balance);
```

### PDA Usage

```rust
let (pda, bump) = ctx.svm.get_pda_with_bump(
    &[b"escrow", maker.pubkey().as_ref(), &seed.to_le_bytes()],
    &my_program::ID,
);

let ix = ctx.program().build_ix(
    InitializeBundle { escrow: pda, /* ... */ },
    vix::Initialize { seed, bump },
);
```

### Error Testing

```rust
let result = ctx.execute_instruction(ix, &[&user])?;

if !result.is_success() {
    result.print_logs();
    println!("Error: {:?}", result.error());
}

// Or assert specific errors (substring match in logs or error field).
result.assert_error("InsufficientFunds");
```

## Testing

```bash
cargo test -p anchor-litesvm
```

## Related Crates

- [`litesvm-utils`](../litesvm-utils): framework-agnostic utilities (included)
- [`litesvm`](https://crates.io/crates/litesvm): the underlying fast Solana VM
- [`anchor-lang`](https://crates.io/crates/anchor-lang): Anchor framework

## License

MIT License - see [LICENSE](../../LICENSE) for details.
