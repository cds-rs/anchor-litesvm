# Quickstart

Deploy an Anchor program, deposit into it, and print the structured logs.
This is the fastest path to a passing test; see the [Vault chapter](examples/vault.md)
for the same program driven in depth.

## Add the dependency

```toml
[dev-dependencies]
anchor-litesvm = "0.4"
```

## The test

```rust
#![allow(unexpected_cfgs)]

use anchor_lang::{self};
use anchor_litesvm::AnchorLiteSVM;
use solana_signer::Signer;

anchor_lang::declare_program!(vault);
anchor_litesvm::bundles_from_idl!(vault);

#[test]
fn deposit_happy_path() {
    // In your own crate, point this at your build output:
    //   include_bytes!("../target/deploy/vault.so")
    let mut ctx = AnchorLiteSVM::build_with_program(
        vault::ID,
        "vault",
        include_bytes!("../target/deploy/vault.so"),
    );
    let alice = ctx.cast_actor("Alice");

    // initialize creates the vault_state + vault PDAs for Alice.
    ctx.tx(&[&alice])
        .build(
            InitializeBundle {
                user: alice.pubkey(),
            },
            vault::client::args::Initialize {},
        )
        .send_ok();

    // deposit 1 SOL.
    ctx.tx(&[&alice])
        .build(
            DepositBundle {
                user: alice.pubkey(),
            },
            vault::client::args::Deposit {
                amount: 1_000_000_000,
            },
        )
        .send_ok()
        .print_logs();
}
```

`declare_program!` generates the typed client from the vault IDL.
`bundles_from_idl!` generates an account bundle per instruction
(`InitializeBundle`, `DepositBundle`, ...), deriving PDAs so you only supply
the accounts that vary per call. `print_logs()` renders the transaction as a
CPI tree, program logs and all.

Next: the [Vault chapter](examples/vault.md) drives the same program deeper
(events, the escape hatch, a rejected transaction), and [Concepts](concepts/aliases.md)
covers aliases, the World, and structured logs.
