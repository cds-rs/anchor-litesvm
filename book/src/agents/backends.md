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
{{#include ../../../crates/testsvm/src/lib.rs:capabilities}}
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
{{#include ../../../crates/testsvm/src/lib.rs:trait-core}}
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
