# anchor-litesvm Workspace

> ⚠ **You are on the `compat/anchor-0.31` LTS branch.**
>
> This branch exists **solely** to support projects that depend on
> `mpl-core`'s `anchor` feature (still pinned to `anchor-lang = "0.31.1"`
> upstream). It is **bug-fixes only** — no forward features will be
> backported. The branch will be **deprecated and archived** as soon as
> upstream mpl-core ships an `anchor-lang = "1.0"` release.
>
> If you are *not* using mpl-core, use [`main`](../../tree/main) instead.
>
> See [`MIGRATING.md`](./MIGRATING.md) for the differences and the
> migration path back to `main`.

**Two powerful crates for Solana program testing with LiteSVM**

| Crate | Description | crates.io | docs.rs |
|-------|-------------|-----------|---------|
| **[anchor-litesvm](crates/anchor-litesvm)** | Anchor-specific testing with simplified syntax | [![Crates.io](https://img.shields.io/crates/v/anchor-litesvm.svg)](https://crates.io/crates/anchor-litesvm) | [![docs.rs](https://docs.rs/anchor-litesvm/badge.svg)](https://docs.rs/anchor-litesvm) |
| **[litesvm-utils](crates/litesvm-utils)** | Framework-agnostic testing utilities | [![Crates.io](https://img.shields.io/crates/v/litesvm-utils.svg)](https://crates.io/crates/litesvm-utils) | [![docs.rs](https://docs.rs/litesvm-utils/badge.svg)](https://docs.rs/litesvm-utils) |

## Which Crate Should I Use?

### Use `anchor-litesvm` if:
- You're testing **Anchor programs**
- You want simplified syntax similar to anchor-client
- You need Anchor account deserialization and event parsing

### Use `litesvm-utils` if:
- You're testing **Native Solana**, **SPL**, or **non-Anchor** programs
- You want framework-agnostic utilities
- You're building your own testing framework

> **Note:** `anchor-litesvm` includes all of `litesvm-utils`, so Anchor users get everything automatically.

## Crate Relationship

```
┌─────────────────────────────────────┐
│         anchor-litesvm              │
│  (Anchor-specific features)         │
│  • Simplified syntax                │
│  • Account deserialization          │
│  • Event parsing                    │
│  • Discriminator handling           │
└─────────────┬───────────────────────┘
              │ builds upon
              ▼
┌─────────────────────────────────────┐
│         litesvm-utils               │
│  (Framework-agnostic utilities)     │
│  • Account creation & funding       │
│  • Token operations                 │
│  • Transaction helpers              │
│  • Assertions                       │
│  • PDA derivation                   │
└─────────────┬───────────────────────┘
              │ uses
              ▼
┌─────────────────────────────────────┐
│           LiteSVM                   │
│  (Fast Solana VM for testing)       │
└─────────────────────────────────────┘
```

## Quick Start

### For Anchor Programs

```toml
[dev-dependencies]
anchor-litesvm = "0.4"
```

```rust
use anchor_litesvm::AnchorLiteSVM;
use litesvm_utils::{AssertionHelpers, TestHelpers};

anchor_lang::declare_program!(my_program);

#[test]
fn test_my_program() {
    // One-line setup
    let mut ctx = AnchorLiteSVM::build_with_program(
        my_program::ID,
        "my_program",
        include_bytes!("../target/deploy/my_program.so"),
    );

    // Create accounts
    let user = ctx.svm.create_funded_account(10_000_000_000).unwrap();

    // Build instruction with simplified syntax
    let ix = ctx.program()
        .accounts(my_program::client::accounts::Initialize { user: user.pubkey(), .. })
        .args(my_program::client::args::Initialize { amount: 100 })
        .instruction()
        .unwrap();

    // Execute and verify
    ctx.execute_instruction(ix, &[&user]).unwrap().assert_success();
}
```

### For Non-Anchor Programs

```toml
[dev-dependencies]
litesvm-utils = "0.4"
```

```rust
use litesvm_utils::{LiteSVMBuilder, TestHelpers, AssertionHelpers, TransactionHelpers};

#[test]
fn test_my_program() {
    // Setup
    let mut svm = LiteSVMBuilder::build_with_program(program_id, &program_bytes);

    // Create accounts and tokens
    let user = svm.create_funded_account(10_000_000_000).unwrap();
    let mint = svm.create_token_mint(&user, 9).unwrap();

    // Execute and verify
    let result = svm.send_instruction(ix, &[&user]).unwrap();
    result.assert_success();
    svm.assert_token_balance(&token_account, 1_000_000);
}
```

## Why These Crates?

| Metric | Raw LiteSVM | anchor-client | anchor-litesvm |
|--------|-------------|---------------|----------------|
| Lines of code | 493 | 279 | **106** |
| Setup lines | 20+ | 15+ | **1** |
| Token mint creation | 30+ lines | 20+ lines | **1 line** |
| Compilation | Fast | Slow | **Fast** |
| Mock RPC needed | No | Yes | **No** |

## Documentation

- **[anchor-litesvm README](crates/anchor-litesvm/README.md)** - Anchor-specific features
- **[litesvm-utils README](crates/litesvm-utils/README.md)** - Framework-agnostic utilities
- **[Quick Start Guide](docs/QUICK_START.md)** - 5-minute tutorial
- **[API Reference](docs/API_REFERENCE.md)** - Complete API docs
- **[Migration Guide](docs/MIGRATION.md)** - Migrate from raw LiteSVM

## Examples

```bash
# Run examples
cargo run -p anchor-litesvm --example basic_usage
cargo run -p anchor-litesvm --example advanced_features
```

## Testing

```bash
# Run all tests (63 total)
cargo test

# Run specific crate tests
cargo test -p anchor-litesvm    # 11 tests
cargo test -p litesvm-utils     # 52 tests
```

## License

MIT License - see [LICENSE](LICENSE) for details.

## Acknowledgments

Built on top of [LiteSVM](https://github.com/LiteSVM/litesvm), a fast and lightweight Solana VM for testing.
