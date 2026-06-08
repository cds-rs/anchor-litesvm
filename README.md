# anchor-litesvm Workspace

*A fork of [anchor-litesvm](https://github.com/brimigs/anchor-litesvm) by [@brimigs](https://github.com/brimigs), maintained for the anchor-0.31 ecosystem.*

> ⚠ **You are on the `compat/anchor-0.31` LTS branch.**
>
> This branch exists **solely** to support projects that depend on
> `mpl-core`'s `anchor` feature (still pinned to `anchor-lang = "0.31.1"`
> upstream). It is **bug-fixes only**: no forward features will be
> backported. The branch will be **deprecated and archived** as soon as
> upstream mpl-core ships an `anchor-lang = "1.0"` release.
>
> If you are *not* using mpl-core, use [`main`](../../tree/main) instead.
>
> See [`MIGRATING.md`](./MIGRATING.md) for the differences and the
> migration path back to `main`.

**Two powerful crates for Solana program testing with LiteSVM**

| Crate | Description |
|-------|-------------|
| **[anchor-litesvm](crates/anchor-litesvm)** | Anchor-specific testing with simplified syntax |
| **[litesvm-utils](crates/litesvm-utils)** | Framework-agnostic testing utilities |

> This branch is distributed **via git only** (not published to crates.io); see [Quick Start](#quick-start) for the dependency form.

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
# Host-only: the test machinery, never compiled into the on-chain binary.
[target.'cfg(not(target_os = "solana"))'.dependencies]
anchor-litesvm = { git = "https://github.com/cds-rs/anchor-litesvm", branch = "compat/anchor-0.31" }
```

```rust
use anchor_litesvm::AnchorLiteSVM;
use litesvm_utils::TestHelpers;
use my_program::{instruction as vix, test_helpers::InitializeBundle};

#[test]
fn test_my_program() {
    // One-line setup: deploy the program. The name registers as a pubkey alias,
    // so structured logs read `my_program::Initialize`, not the raw program id.
    let mut ctx = AnchorLiteSVM::build_with_program(
        my_program::ID,
        "my_program",
        include_bytes!("../target/deploy/my_program.so"),
    );

    let user = ctx.svm.create_funded_account(10_000_000_000).unwrap();

    // Build, send, and assert in one chain. The bundle names the accounts; the
    // BundledPubkeys derive on the program orders them, so there is no
    // hand-built Vec<AccountMeta> and no client codegen.
    ctx.tx(&[&user])
        .build(
            InitializeBundle { user: user.pubkey() },
            vix::Initialize { amount: 100 },
        )
        .send_ok(); // builds, sends, asserts success

    // Then assert on-chain state with ctx.svm.assert_* or ctx.get_account::<T>().
}
```

`InitializeBundle` is a small `#[derive(Bundle)]` struct of pubkeys in your
program's host-only `test_helpers` module, bound to the instruction's
`#[derive(Accounts)]` struct with
`#[cfg_attr(not(target_os = "solana"), derive(BundledPubkeys), bundled_with(InitializeBundle))]`.
See [EVALUATING.md](./EVALUATING.md) for the complete program-side setup.

### For Non-Anchor Programs

```toml
[dev-dependencies]
litesvm-utils = { git = "https://github.com/cds-rs/anchor-litesvm", branch = "compat/anchor-0.31" }
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
# Fabricate a complete NFT (mint + Token Metadata + holder), no minting tx:
cargo run -p litesvm-utils --example fabricate_nft
```

## Example programs

Full programs tested with these crates, each pinned to the branch matching its
Anchor version:

| Program | Branch (Anchor) | What it shows |
|---------|-----------------|---------------|
| [`cds-turbin3/builder-03-nft-stake`](https://github.com/cds-turbin3/builder-03-nft-stake) | `compat/anchor-0.31` (0.31) | mpl-core NFT staking on this branch: the framework against a real Metaplex consumer. |
| [`cds-turbin3/builder-01-vault`](https://github.com/cds-turbin3/builder-01-vault) | `turbin3` (1.0) | Vault deposit / withdraw. |
| [`cds-turbin3/builder-01-escrow`](https://github.com/cds-turbin3/builder-01-escrow) | `turbin3` (1.0) | Escrow make / take / refund. |
| [`cds-turbin3/builder-02-amm`](https://github.com/cds-turbin3/builder-02-amm) | `turbin3` (1.0) | A constant-product AMM. |
| [`cds-rs/anchor-escrow-with-litesvm`](https://github.com/cds-rs/anchor-escrow-with-litesvm) | `turbin3` (1.0) | Escrow migrated to the bundle API; a generated `TESTRUN.md` with authority / ownership / sequence diagrams. |

The `compat/anchor-0.31` consumer is **builder-03-nft-stake** (mpl-core pins anchor
0.31); the others target the `turbin3` branch (anchor 1.0). Each commits a
deterministic test report, so a diff in that report is a behavior change worth
scrutinizing.

## Feedback

Tried it on your program? Open a [**Dogfood feedback**](https://github.com/cds-rs/anchor-litesvm/issues/new/choose)
issue (it lands under the `🐶 dogfood` label) and tell us what helped and what got in
your way. The full call is in [docs/dogfooding-call.md](docs/dogfooding-call.md).

## Testing

```bash
# The compatibility check: full workspace suite + the fabrication example
just test-compat

# Or directly:
cargo test --workspace
```

## License

MIT License - see [LICENSE](LICENSE) for details.

## Acknowledgments

This project is a fork of [anchor-litesvm](https://github.com/brimigs/anchor-litesvm)
by [@brimigs](https://github.com/brimigs); the `compat/anchor-0.31` branch extends her
original crates for the anchor-0.31 / mpl-core ecosystem.

Built on top of [LiteSVM](https://github.com/LiteSVM/litesvm), a fast and lightweight Solana VM for testing.
