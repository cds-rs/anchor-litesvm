# Vault

The vault program has four instructions: `initialize` creates a per-user
`vault_state` PDA and its companion `vault` PDA, `deposit` moves lamports in
and emits a `Deposited` event, `withdraw` moves them back out, and `close`
returns the rent. This chapter drives `initialize` and `deposit` through
`anchor-litesvm`, then uses the escape hatch to show what happens when an
attacker substitutes a valid-but-wrong account.

## Boot and deposit

```rust
anchor_lang::declare_program!(vault);
anchor_litesvm::bundles_from_idl!(vault);

fn boot() -> anchor_litesvm::AnchorContext {
    let mut ctx =
        AnchorLiteSVM::build_with_program(vault::ID, "vault", &common::fixture_bytes("vault"));
    // Decode `Deposited` badges from the committed IDL.
    ctx.register_events_from_idl(include_str!("../idls/vault.json"));
    ctx
}
```

`declare_program!` generates the typed client from the vault IDL;
`bundles_from_idl!` generates an account bundle (`InitializeBundle`,
`DepositBundle`, ...) for each instruction, deriving PDAs so you only supply
the accounts that vary per call. `register_events_from_idl` teaches the
context how to decode the program's events from its logs.

```rust
let mut ctx = boot();
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

// deposit 1 SOL; capture the rendered CPI tree (system transfer + Deposited badge).
let result = ctx
    .tx(&[&alice])
    .build(
        DepositBundle {
            user: alice.pubkey(),
        },
        vault::client::args::Deposit {
            amount: 1_000_000_000,
        },
    )
    .send_ok();

let ev: vault::events::Deposited = result.parse_event().expect("Deposited event present");
assert_eq!(ev.amount, 1_000_000_000);
```

`result.tree_string()` renders the transaction as a CPI tree:

```text
{{#include ../captured/vault_deposit.txt}}
```

The `System [2]` child is the lamport transfer `deposit` makes via CPI; the
🔔 line is the decoded `Deposited` event, with `user` alias-resolved back to
`Alice`.

## The escape hatch

`build_ix` derives every account from the bundle honestly. `build_ix_with`
does the same, then hands you a closure to override exactly one slot, so you
can construct the instruction an attacker would submit without hand-rolling
every other account yourself.

Mallory wants Alice's deposit. She initializes her own vault first, so a
real, program-owned, correctly-discriminated `VaultState` account exists at
*her* PDA, then submits a deposit into Alice's vault with the `vault_state`
slot swapped for her own:

```rust
let (mallory_state, _) = vault_state_pda(&mallory.pubkey());
let ix = ctx.program().build_ix_with(
    DepositBundle {
        user: alice.pubkey(),
    },
    vault::client::args::Deposit {
        amount: 1_000_000_000,
    },
    |accounts| accounts.vault_state = mallory_state,
);

let result = ctx.send_err_named(ix, &[&alice], "ConstraintSeeds");
```

```text
{{#include ../captured/vault_wrong_state.txt}}
```

Anchor loads Mallory's account fine (right owner, right discriminator), but
the `✗` leaf is `ConstraintSeeds`: a seeds check derived from Alice's key
rejects the mismatch. This is the confused-deputy story: a substituted
account that's valid in every way except who it belongs to, caught by the
one constraint that checks.

The full test is `crates/anchor-litesvm/tests/book_vault.rs`.
