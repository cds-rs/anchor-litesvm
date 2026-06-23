# Migrating from Raw LiteSVM

This appendix helps you migrate existing raw-LiteSVM tests to `anchor-litesvm`. If you're starting fresh, you want [Part I](../intro/why.md) instead; this is for the case where you already have hand-rolled tests and want to see, pattern by pattern, what each one becomes.

## Quick migration checklist

- [ ] Add `anchor-litesvm` as a host-only (target-cfg'd) dependency; drop the raw `litesvm` dev-dependency
- [ ] Add a host-only `test_helpers` module to your program with a `#[derive(Bundle)]` struct per instruction (or one shared bundle)
- [ ] Decorate each `#[derive(Accounts)]` struct with the `#[cfg_attr(..., derive(BundledPubkeys), bundled_with(...))]` line
- [ ] Replace `LiteSVM::new()` with `AnchorLiteSVM::build_with_program()`
- [ ] Replace manual discriminator calculation and `Vec<AccountMeta>` building with `ctx.tx(...).build(bundle, args)`
- [ ] Replace manual token operations with `ctx.svm` helper methods
- [ ] Replace manual assertions with `ctx.svm.assert_*` helpers
- [ ] Drop direct `solana-*` dev-dependencies; reach for them through the `anchor-litesvm` facade re-exports

Most of this is test-side. The one step that touches your *program* (not just your tests) is the second and third: the bundle module and the `cfg_attr` derives. They're gated to the host build, so nothing changes on chain.

## Step by step

### Step 1: Update dependencies

**Before:**

```toml
[dev-dependencies]
litesvm = "0.1"
solana-signer = "3.0.0"
```

**After:**

```toml
[dependencies]
anchor-lang = "1.0.2"
anchor-spl = "1.0.2"   # only if your program makes token CPIs

# Host-only: compiled for `cargo test`, never into the BPF binary. It's a
# normal dependency (not a dev-dependency) so the bundle derives, whose impls
# must live in your crate by the orphan rule, can see it from `src/`.
[target.'cfg(not(target_os = "solana"))'.dependencies]
anchor-litesvm = { git = "https://github.com/cds-rs/anchor-litesvm", branch = "turbin3" }
```

Note there's no `[dev-dependencies]` block anymore: the test reaches `Keypair`, `Pubkey`, `Signer`, the harness, and the helpers through the `anchor-litesvm` facade. (The why behind the target-cfg dance is in [Installation & Setup](../intro/installation.md).)

### Step 2: Add a bundle and wire the derive

This is the program-side step. For each instruction, name its varying pubkeys in a bundle, in a host-only module:

```rust
// src/test_helpers.rs
use anchor_lang::prelude::Pubkey;
use anchor_litesvm::Bundle;

#[derive(Copy, Clone, Debug, Bundle)]
pub struct MakeBundle {
    pub maker: Pubkey,
    pub escrow: Pubkey,
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
    pub maker_ata_a: Pubkey,
    pub vault: Pubkey,
}
```

```rust
// src/lib.rs
#[cfg(not(target_os = "solana"))]
pub mod test_helpers;
```

Then decorate the matching `#[derive(Accounts)]` struct so the derive can bridge the bundle to it:

```rust
#[cfg_attr(
    not(target_os = "solana"),
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(crate::test_helpers::MakeBundle),
)]
#[derive(Accounts)]
pub struct Make<'info> { /* ... */ }
```

The bundle omits the program accounts (`token_program`, `system_program`, `associated_token_program`); those are auto-injected from their Anchor field types. The full mechanism is [Bundled Pubkeys](../instructions/bundled-pubkeys.md). This replaces the old `declare_program!` + generated-client step entirely: tests use your program's *own* types, so there's no IDL to generate or keep in an `idls/` directory.

### Step 3: Update test setup

**Before (raw LiteSVM):**

```rust
use litesvm::LiteSVM;

let mut svm = LiteSVM::new();
svm.add_program(program_id, include_bytes!("../target/deploy/my_program.so"));
```

**After:**

```rust
use anchor_litesvm::AnchorLiteSVM;

let mut ctx = AnchorLiteSVM::build_with_program(
    my_program::ID,
    "my_program",
    include_bytes!("../target/deploy/my_program.so"),
);
```

**Savings:** 3 lines → 1 line.

### Step 4: Replace manual token operations

**Before (30+ lines):**

```rust
let mint = Keypair::new();
let rent = svm.minimum_balance_for_rent_exemption(82);
let create_mint_ix = system_instruction::create_account(
    &payer.pubkey(), &mint.pubkey(), rent, 82, &spl_token::id(),
);
let init_mint_ix = spl_token::instruction::initialize_mint(
    &spl_token::id(), &mint.pubkey(), &authority.pubkey(), None, 9,
)?;
let tx = Transaction::new_signed_with_payer(
    &[create_mint_ix, init_mint_ix], Some(&payer.pubkey()),
    &[&payer, &mint], svm.latest_blockhash(),
);
svm.send_transaction(tx)?;
// create token account (another 20+ lines), mint tokens (another 15+)
```

**After (3 lines):**

```rust
let mint = ctx.svm.create_token_mint(&authority, 9)?;
let token_account = ctx.svm.create_associated_token_account(&mint.pubkey(), &owner)?;
ctx.svm.mint_to(&mint.pubkey(), &token_account, &authority, 1_000_000)?;
```

**Savings:** 65+ lines → 3 lines. (See [PDAs & Token Helpers](../instructions/pdas-and-tokens.md).)

### Step 5: Replace manual discriminator and instruction building

The two old chores, computing the 8-byte discriminator by hand and assembling an ordered `Vec<AccountMeta>`, collapse into a single `build`/`send` chain.

**Before (25+ lines):**

```rust
use sha2::{Digest, Sha256};

fn get_discriminator(name: &str) -> [u8; 8] {
    let mut hasher = Sha256::new();
    hasher.update(format!("global:{}", name));
    let result = hasher.finalize();
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&result[..8]);
    disc
}

let accounts = vec![
    AccountMeta::new(data_account, false),
    AccountMeta::new(user.pubkey(), true),
    AccountMeta::new_readonly(system_program::id(), false),
];
let mut data = get_discriminator("initialize").to_vec();
data.extend_from_slice(&borsh::to_vec(&InitializeArgs { value: 42 })?);
let ix = Instruction { program_id, accounts, data };
```

**After (one chain):**

```rust
use my_program::instruction as vix;

let accs = InitializeBundle { data_account, user: user.pubkey() };
ctx.tx(&[&user])
    .build(accs, vix::Initialize { value: 42 })
    .send_ok();
```

The discriminator is computed for you, the accounts are ordered by the program's own `ToAccountMetas`, and the compiler now catches a missing account or a mismatched args type at the call site. The ordered-`Vec` failure modes this removes are the subject of [Named Accounts](../instructions/named-accounts.md). (When you need the raw `Instruction` rather than an immediate send, `ctx.program().build_ix(accs, vix::Initialize { value: 42 })` returns it.)

### Step 6: Replace manual assertions

**Before:**

```rust
let account = svm.get_account(&pubkey).expect("Account should exist");

let account_data = svm.get_account(&token_account).unwrap();
let token_data = spl_token::state::Account::unpack(&account_data.data)?;
assert_eq!(token_data.amount, 1_000_000, "Wrong balance");

let account = svm.get_account(&user_pubkey).unwrap();
assert_eq!(account.lamports, 10_000_000_000, "Wrong SOL balance");
```

**After:**

```rust
ctx.svm.assert_account_exists(&pubkey);
ctx.svm.assert_token_balance(&token_account, 1_000_000);
ctx.svm.assert_sol_balance(&user_pubkey, 10_000_000_000);
```

**Savings:** 10+ → 3, with better failure messages. (See [Assertion Helpers](../running/assertions.md).)

## Pattern-by-pattern comparison

### Account creation

| Operation | Raw LiteSVM | anchor-litesvm |
| --- | --- | --- |
| **Funded account** | Manual airdrop + keypair | `ctx.svm.create_funded_account(lamports)` |
| **Token mint** | 30+ lines | `ctx.svm.create_token_mint(&auth, decimals)` |
| **Token account** | 20+ lines | `ctx.svm.create_associated_token_account(&mint, &owner)` |
| **Mint tokens** | 15+ lines | `ctx.svm.mint_to(&mint, &account, &auth, amount)` |

### PDA operations

**Before:**

```rust
let (pda, bump) = Pubkey::find_program_address(&[b"vault", user.pubkey().as_ref()], &program_id);
```

**After:**

```rust
let pda = ctx.svm.get_pda(&[b"vault", user.pubkey().as_ref()], &program_id);
let (pda, bump) = ctx.svm.get_pda_with_bump(&[b"vault", user.pubkey().as_ref()], &program_id);
```

### Transaction execution

**Before:**

```rust
let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[&payer], svm.latest_blockhash());
match svm.send_transaction(tx) {
    Ok(_) => println!("Success"),
    Err(e) => panic!("Transaction failed: {:?}", e),
}
```

**After:**

```rust
ctx.tx(&[&payer])
    .build(accs, vix::Initialize { value: 42 })
    .send_ok();   // sends, asserts success, carries your aliases forward
```

(See [Executing Transactions](../running/executing.md).)

### Error testing

**Before:**

```rust
let result = svm.send_transaction(tx);
assert!(result.is_err(), "Should have failed");
let error_string = format!("{:?}", result.unwrap_err());
assert!(error_string.contains("InsufficientFunds"));
```

**After:**

```rust
ctx.tx(&[&payer])
    .build(accs, vix::Withdraw { amount: too_much })
    .send_err_named("InsufficientFunds");   // asserts failure AND the named error
```

`send_err_named` checks both halves: that it failed, and that *this* error fired. A bare `send_err()` asserts only the first.

## Complete example

### Before: raw LiteSVM (~493 lines)

```rust
#[cfg(test)]
mod tests {
    // ... 10 lines of manual get_discriminator() ...
    #[test]
    fn test_escrow() {
        let mut svm = LiteSVM::new();
        svm.add_program(/* ... */);
        let maker = Keypair::new();
        svm.airdrop(&maker.pubkey(), 10_000_000_000).unwrap();
        // create mint (30+ lines), token accounts (40+ lines/account)
        // build MAKE instruction by hand (25+ lines of AccountMeta + discriminator)
        // execute (10+ lines), verify by unpacking raw account data (20+ lines)
        // ... 300+ more lines for TAKE and REFUND ...
    }
}
```

### After: anchor-litesvm (~106 lines)

```rust
use anchor_litesvm::{AnchorLiteSVM, AssertionHelpers, Signer, TestHelpers};
use anchor_escrow::{instruction as vix, test_helpers::EscrowBundle};

#[test]
fn test_escrow() {
    let mut ctx = AnchorLiteSVM::build_with_program(
        anchor_escrow::ID,
        "anchor_escrow",
        include_bytes!("../../target/deploy/anchor_escrow.so"),
    );

    let maker = ctx.svm.create_funded_account(10_000_000_000).unwrap();
    let mint_a = ctx.svm.create_token_mint(&maker, 9).unwrap();
    let mint_b = ctx.svm.create_token_mint(&maker, 9).unwrap();
    let maker_ata_a = ctx.svm.create_associated_token_account(&mint_a.pubkey(), &maker).unwrap();
    ctx.svm.mint_to(&mint_a.pubkey(), &maker_ata_a, &maker, 1_000_000_000).unwrap();

    let seed = 42u64;
    let escrow = ctx.svm.get_pda(
        &[b"escrow", maker.pubkey().as_ref(), &seed.to_le_bytes()],
        &anchor_escrow::ID,
    );
    let vault = ctx.svm.create_associated_token_account(&mint_a.pubkey(), &escrow).unwrap();

    // One shared bundle names every account in the trade; `token_program`,
    // `system_program`, and `associated_token_program` are auto-injected.
    let accs = EscrowBundle {
        maker: maker.pubkey(),
        escrow,
        mint_a: mint_a.pubkey(),
        mint_b: mint_b.pubkey(),
        maker_ata_a,
        vault,
    };

    ctx.tx(&[&maker])
        .build(accs, vix::Make { seed, receive: 500_000_000, amount: 1_000_000_000 })
        .send_ok();

    ctx.svm.assert_token_balance(&vault, 1_000_000_000);
}
```

**Result:** 493 → 106 lines. The fully built-out version of this is the [Escrow worked example](../examples/escrow.md).

## Common issues

**Forgetting the `cfg_attr` derive** (`build` won't accept your bundle): the `From<Bundle>` projection comes from `#[cfg_attr(not(target_os = "solana"), derive(BundledPubkeys), bundled_with(...))]` on the accounts struct. Keep `bundled_with` *inside* the `cfg_attr`, not as a bare attribute. (See [Bundled Pubkeys](../instructions/bundled-pubkeys.md).)

**Struct name doesn't match the handler** (`E0425: cannot find type ... in module crate::instruction`): the derive resolves its args type from `PascalCase(handler_fn_name)`. If your struct is `InitPoll` but the handler is `initialize_poll`, add `instruction = crate::instruction::InitializePoll` to `bundled_with(...)`.

**`Program<'_, Token>` isn't auto-injected:** only `Program<System>`, `Program<AssociatedToken>`, and `Interface<TokenInterface>` are. For the classic token program, declare the field as `Interface<'info, TokenInterface>` (it still resolves to the SPL Token id).

**Accessing LiteSVM:** raw `svm.get_account(&pk)` becomes `ctx.svm.get_account(&pk)`; the `LiteSVM` lives at `ctx.svm`.

## Benefits summary

| Aspect | Before | After |
| --- | --- | --- |
| **Lines of code** | 493 | 106 |
| **Setup** | 20+ lines | 1 line |
| **Token operations** | 65+ lines | 3 lines |
| **Instruction building** | 25+ lines | one `build`/`send` chain |
| **Account ordering** | Manual Vec (error-prone) | Named bundle (order-independent) |
| **Assertions** | 10+ lines | 1 line |
| **Type safety** | Manual | Compile-time |
