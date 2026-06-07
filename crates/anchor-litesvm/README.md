# anchor-litesvm

**Simplified Anchor testing with LiteSVM** - Similar syntax to anchor-client, 78% less code, no mock RPC needed.

[![Crates.io](https://img.shields.io/crates/v/anchor-litesvm.svg)](https://crates.io/crates/anchor-litesvm)
[![Documentation](https://docs.rs/anchor-litesvm/badge.svg)](https://docs.rs/anchor-litesvm)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Overview

`anchor-litesvm` provides a streamlined testing experience for Anchor programs. It combines the familiar syntax of anchor-client with the speed of LiteSVM, plus comprehensive testing utilities.

**Key Benefits:**
- **78% less code** compared to raw LiteSVM
- **40% faster compilation** than anchor-client (no network dependencies)
- **No mock RPC** - zero configuration needed
- **Familiar syntax** - similar to anchor-client, transferable knowledge

## Installation

```toml
[dev-dependencies]
anchor-litesvm = "0.4"
```

## Quick Start

```rust
use anchor_litesvm::AnchorLiteSVM;
use litesvm_utils::{AssertionHelpers, TestHelpers};
use solana_signer::Signer;

// Generate client types from your program
anchor_lang::declare_program!(my_program);

#[test]
fn test_my_anchor_program() {
    // 1. One-line setup - no mock RPC needed. The name registers as a
    //    pubkey alias so structured logs read `my_program::Transfer`
    //    instead of the raw program ID.
    let mut ctx = AnchorLiteSVM::build_with_program(
        my_program::ID,
        "my_program",
        include_bytes!("../target/deploy/my_program.so"),
    );

    // 2. Create accounts with built-in helpers
    let user = ctx.svm.create_funded_account(10_000_000_000).unwrap();
    let mint = ctx.svm.create_token_mint(&user, 9).unwrap();

    // 3. Build instruction with simplified syntax (similar to anchor-client)
    let ix = ctx.program()
        .accounts(my_program::client::accounts::Initialize {
            user: user.pubkey(),
            mint: mint.pubkey(),
            system_program: solana_system_interface::program::id(),
        })
        .args(my_program::client::args::Initialize { amount: 1_000_000 })
        .instruction()
        .unwrap();

    // 4. Execute and verify
    ctx.execute_instruction(ix, &[&user])
        .unwrap()
        .assert_success();

    // 5. Deserialize Anchor accounts
    let account_data: MyAccount = ctx.get_account(&pda).unwrap();
}
```

## Why anchor-litesvm?

| Feature | anchor-client + LiteSVM | anchor-litesvm |
|---------|------------------------|----------------|
| Lines of Code | 279 lines | **106 lines** |
| Compilation | Slow (network deps) | **40% faster** |
| Setup | Mock RPC needed | **No config** |
| Token Operations | Manual (30+ lines) | **1 line** |
| Syntax | anchor-client | **Similar** |

### No More Account Ordering Bugs

The #1 pain point in Solana testing - eliminated:

```rust
// Raw LiteSVM - order matters, easy to get wrong
let instruction = Instruction {
    accounts: vec![
        AccountMeta::new(maker.pubkey(), true),   // Must be position 0
        AccountMeta::new(escrow_pda, false),       // Must be position 1
        // Wrong order = silent failure or wrong accounts
    ],
    ..
};

// anchor-litesvm - named fields, order doesn't matter
let ix = ctx.program()
    .accounts(my_program::client::accounts::Make {
        escrow: escrow_pda,  // Any order works
        maker: maker.pubkey(),
        // Compiler ensures all fields present
    })
    .args(...)
    .instruction()?;
```

## Features

### One-Call Instruction Building with `BuildableIx`

If your program's Accounts structs share most of their pubkeys across instructions, you can implement `BuildableIx` once per instruction and collapse the `.accounts().args().instruction()` chain into a single call. The args/accounts pairing is checked at compile time, so passing `Deposit` args with `Withdraw` accounts is a type error instead of a runtime failure.

In your program crate (one-time setup):

```rust
use anchor_litesvm::BuildableIx;
use anchor_lang::prelude::Pubkey;

#[derive(Copy, Clone)]
pub struct BundledPubkeys {
    pub user: Pubkey,
    pub vault_state: Pubkey,
    pub vault: Pubkey,
}

impl From<BundledPubkeys> for accounts::Deposit {
    fn from(b: BundledPubkeys) -> Self {
        Self {
            user: b.user,
            vault_state: b.vault_state,
            vault: b.vault,
            system_program: solana_program::system_program::ID,
        }
    }
}

impl BuildableIx<BundledPubkeys> for instruction::Deposit {
    type Accounts = accounts::Deposit;
}
```

In your tests:

```rust
let bundle = BundledPubkeys { user, vault_state, vault };

// Happy path - one call.
let ix = ctx.program().build_ix(bundle, instruction::Deposit { amount: 1_000_000 });

// Negative path - pass a deliberately-wrong account via a closure.
let ix = ctx.program().build_ix_with(
    bundle,
    instruction::Deposit { amount: 1_000_000 },
    |a| a.vault_state = wrong_pda,
);
```

You can also implement `BuildableIx` for the same args struct against multiple bundle types (the bundle is a trait parameter, not an associated type). For example, an alternate bundle that routes a delegated signing authority into the `user` field, for tests that exercise a different signing path:

```rust
pub struct DelegatedBundle {
    pub delegate: Pubkey,
    pub vault_state: Pubkey,
    pub vault: Pubkey,
}

impl From<DelegatedBundle> for accounts::Deposit {
    fn from(b: DelegatedBundle) -> Self {
        Self {
            user: b.delegate, // delegate signs in place of the user
            vault_state: b.vault_state,
            vault: b.vault,
            system_program: solana_program::system_program::ID,
        }
    }
}

impl BuildableIx<DelegatedBundle> for instruction::Deposit {
    type Accounts = accounts::Deposit;
}

// Same args struct, dispatched by the bundle's concrete type:
let user_ix = ctx.program().build_ix(
    BundledPubkeys { user, vault_state, vault },
    instruction::Deposit { amount: 1_000_000 },
);
let delegate_ix = ctx.program().build_ix(
    DelegatedBundle { delegate, vault_state, vault },
    instruction::Deposit { amount: 1_000_000 },
);
```

The bundle's type tells the compiler (and the test reader) which scenario is being exercised; the accounts struct it produces is the same shape either way. Add as many bundle types as your test suite needs, each with its own `From` impl, and the call site picks the right one by the bundle value's type.

A caveat in the interest of honesty: the vault example threaded through this section doesn't actually have a separate authority/delegation distinction in its accounts shape (just a single `user: Signer`), so the `DelegatedBundle` above is mostly illustrative for *this* program; a test could get the same effect by constructing `BundledPubkeys` with a different pubkey in the `user` slot. Programs with richer account shapes (separate authority/owner/delegate fields, instructions that accept several potential signers, multi-step approval flows) get more concrete value from the multi-bundle pattern. The design is a quality-of-life affordance: nice when your program's shape rewards it, easy to ignore when it doesn't.

### Manual Instruction Building (escape hatch)

The chain stays available if you need full control over the accounts struct or want to skip `BuildableIx` setup for a one-off test:

```rust
let ix = ctx.program()
    .accounts(my_program::client::accounts::Transfer {
        from: from_account,
        to: to_account,
        authority: user.pubkey(),
    })
    .args(my_program::client::args::Transfer { amount: 100 })
    .instruction()?;

ctx.execute_instruction(ix, &[&user])?.assert_success();
```

### Anchor Account Deserialization

```rust
// Deserialize with discriminator check
let account: MyAccount = ctx.get_account(&pda)?;

// Deserialize without check (for PDAs with custom layouts)
let account: MyAccount = ctx.get_account_unchecked(&pda)?;
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

let ix = ctx.program()
    .accounts(my_program::client::accounts::Initialize {
        escrow: pda,
        // ...
    })
    .args(my_program::client::args::Initialize { seed, bump })
    .instruction()?;
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
cargo test -p anchor-litesvm    # 11 tests
```

## Related Crates

- [`litesvm-utils`](https://crates.io/crates/litesvm-utils) - Framework-agnostic utilities (included)
- [`litesvm`](https://crates.io/crates/litesvm) - The underlying fast Solana VM
- [`anchor-lang`](https://crates.io/crates/anchor-lang) - Anchor framework

## License

MIT License - see [LICENSE](../../LICENSE) for details.
