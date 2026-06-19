# Backends

One trait (`TestSVM`), one engine per build. A test written in the trait's
verbs runs on any engine; switching engines is a manifest change and a
rebuild, never a runtime branch.

## The matrix

| backend | crate (feature) | engine | reset | fees | structured CPI |
|---|---|---|---|---|---|
| `LiteSvmBackend` | `litesvm-utils` / `anchor-litesvm` | in-memory litesvm | instant | yes | litesvm's own parser |
| `RpcBackend` | `litesvm-utils` (feature `rpc`) | surfnet over JSON-RPC | endpoint-dependent | no | via canonical log parse |
| `MolluskBackend` | `testsvm-mollusk` (own build) | mollusk-svm | instant | no | via canonical log parse |

A backend declares what it can populate, and reports annotate degraded output
instead of silently rendering partial diagrams:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Whether `model::Transaction::trace` is populated (the authority
    /// diagram needs it).
    pub per_frame_trace: bool,
    /// Whether `frames` came from structured facts (vs the canonical log parse).
    pub structured_cpi: bool,
    /// Whether a multi-ix `send` is one atomic, budget-shared transaction.
    pub atomic_send: bool,
    /// Whether the engine models fees (`fee: Some(..)`).
    pub fees: bool,
    /// Whether a full per-test reset is cheap (rebuild the VM) vs requiring
    /// namespacing on a shared endpoint.
    pub instant_reset: bool,
    /// Whether the endpoint forks live cluster state.
    pub fork: bool,
}
```

## The trait

The required verbs are the trait-core below. Default methods build the cast
vocabulary on top of them: `actor`, `prop` / `prop_at`, `deploy_from_file`,
`label`, the cast-name guard (a duplicate cast name panics on every engine), and
the `register_*` naming sockets. The token extension `TokenTestSVM`
(blanket-implemented for every `TestSVM`) adds `prop_mint`, `prop_token_account`,
and `alias_ata`, hand-packing the stable SPL layouts with no token-crate
dependency.

```rust
pub trait TestSVM {
    /// Build a transaction from `ixs` (`signers[0]` is the fee payer), send
    /// it, and return what the engine witnessed. One atomic transaction: all
    /// ixs succeed or none persist, shared CU budget; an engine that cannot
    /// honor atomicity must say so in `capabilities()`. A program-level
    /// failure is carried in [`model::Transaction::error`]; only a malformed
    /// build panics.
    fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> model::Transaction;

    /// Fund an address with lamports (airdrop in memory; a faucet/cheatcode
    /// or real transfer over RPC).
    fn fund_sol(&mut self, address: &Pubkey, lamports: u64);

    /// Write an account's full state (lamports, data, owner): the state
    /// fabrication lever. Every engine has it natively (litesvm
    /// `set_account`, mollusk's account store, surfpool's
    /// `surfnet_setAccount` cheatcode); tests use it to fabricate state a
    /// real flow would have built elsewhere (a mint, a foreign PDA).
    fn set_account(&mut self, address: &Pubkey, account: Account);

    /// Post-execution owner of an account (drives the ownership graph).
    fn account_owner(&self, pubkey: &Pubkey) -> Option<Pubkey>;

    /// Read an account's full state.
    fn get_account(&self, pubkey: &Pubkey) -> Option<Account>;

    /// Make a program available at `program_id`.
    fn deploy_program(&mut self, program_id: Pubkey, bytes: &[u8]);

    /// Move the clock so the next execution sees at least this slot. On
    /// litesvm/mollusk the clock moves only when warped; surfpool's ticks in
    /// real time, so this is a floor, not a freeze.
    fn warp_to_slot(&mut self, slot: u64);

    /// Set the Clock sysvar's `unix_timestamp` (seconds); other Clock fields
    /// unchanged.
    fn warp_to_timestamp(&mut self, unix_timestamp: i64);

    /// The clock the engine will present to the next execution.
    fn clock(&self) -> Clock;

    fn capabilities(&self) -> Capabilities;

    /// The engine's alias table, read access. The socket the naming default
    /// methods build on ([`label`](Self::label), and the token aliasing in
    /// [`TokenTestSVM`](crate::token::TokenTestSVM)): every adapter already
    /// holds an [`Aliases`](crate::aliases::Aliases) for `register_alias`, so it
    /// returns a borrow and inherits the naming surface for free.
    fn aliases(&self) -> &crate::aliases::Aliases;
}
```

`AnchorContext` is itself a `TestSVM` engine (Anchor-flavored, over in-memory
litesvm): it inherits this whole vocabulary as default methods and is usable
anywhere a `&mut impl TestSVM` is expected, with its Anchor-specific sugar
(`cast_actor`, `cast_mint`, `fund_ata`, `try_load` / `load`) layered on top.

## Recipes

**litesvm, Anchor suite** (the default; the whole book runs on this):

```rust
let mut ctx = AnchorLiteSVM::build_with_program(program::ID, "program", PROGRAM_SO);
```

**litesvm, trait-level** (framework-agnostic suites):

```rust
let mut svm = LiteSvmBackend::new(LiteSVM::new());
svm.deploy_from_file(&PROGRAM_ID, "target/deploy/program.so", "program");
let payer = svm.actor("payer", 10_000_000_000);
```

**mollusk, Pinocchio suite** (in the excluded crate's own build):

```rust
let mut svm = MolluskBackend::new();
svm.deploy_from_file(&PROGRAM_ID, "target/deploy/program.so", "program");
svm.register_program_instructions(&PROGRAM_ID, program::Instruction::instruction_names());
```

**surfnet over RPC** (feature `rpc`; the endpoint must be running):

```rust
let mut svm = RpcBackend::new("http://127.0.0.1:8899");
```

**token fabrication, any engine** (`use testsvm::token::TokenTestSVM;`): fabricate
token state a real flow would have built elsewhere, instead of hand-packing it:

```rust
let mint = svm.prop_mint("USDC", 6, &authority);
let holder = svm.prop_token_account("alice.usdc", &mint, &alice, 1_000_000);
```

`prop_mint` (82-byte SPL mint) and `prop_token_account` (165-byte token account
at the canonical `(owner, mint)` ATA) write the bytes directly with no CPI, so
they work on mollusk and litesvm alike. A Pinocchio token suite reaches for these
instead of `Mint::pack` / `Account::pack` + `prop`.

## Asserting failure at the trait level

`send` never panics on a program failure; it returns the transaction with
`error: Some(message)`. The `send_err_named` sugar belongs to the Anchor
context and does not exist here; assert on the model:

```rust
let tx = svm.send(&[ix], &[&funder]);
let err = tx.error.expect("must be rejected");
assert!(err.contains("Provided seeds do not result in a valid address"));
assert!(svm.get_account(&addr).is_none(), "failed sends persist nothing");
```

A failed send commits no state on any engine: an address the transaction
would have created reads back as `None`.

## Choosing

- Default to litesvm. It is the fastest reset and the only engine the
  higher-level `AnchorContext` sugar targets.
- Use mollusk when the suite must run where mollusk already runs
  (instruction-level Pinocchio harnesses); the vocabulary is identical, fees
  are absent (`fee: None`, `capabilities().fees == false`).
- Use `RpcBackend` to exercise forked or live cluster state; the clock ticks
  in real time there, so `warp_to_slot` is a floor, not a freeze.
