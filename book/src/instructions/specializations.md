# Specializations

The [counter](end-to-end.md) is the whole mechanism in miniature: a bundle, the `cfg_attr`, a deterministic test. Real programs add to that shape, tokens, forked mainnet state, accounts passed at runtime, and each addition is a *specialization*: a delta you layer on once the shape is in your hands, not a new framework to learn. This chapter collects them, each section showing only what it adds, all on the same counter crate.

## Tokens

The counter touched only its own state. A token instruction reaches into the SPL Token program, so it pulls in two things: a CPI in the program, and a mint plus token accounts in the test. Both are small.

### The instruction

`donate` moves tokens from a donor to a recipient. Its `Accounts` struct is ordinary Anchor, and it carries `token_program` the same way the counter carried `system_program`:

```rust
{{#include ../../listings/counter/programs/counter/src/instructions/donate.rs:accounts}}
```

The handler is one `transfer_checked` CPI:

```rust
{{#include ../../listings/counter/programs/counter/src/instructions/donate.rs:handler}}
```

> **N.B.** In anchor-lang 1.0.2, `CpiContext::new(program_id, accounts)` takes the program *id* as a `Pubkey`, which is why the handler passes `self.token_program.key()`. Reaching for `self.token_program.to_account_info()` is the natural mistake, and it won't typecheck: the program goes in as a key, the accounts as infos.

The bundle is where the lesson is. Four fields, and `token_program` is not one of them:

```rust
{{#include ../../listings/counter/programs/counter/src/test_helpers.rs:donatebundle}}
```

`token_program` auto-injects. It's in the `Accounts` struct, the CPI needs it, but the derive reads its Anchor type (`Interface<'info, TokenInterface>`), recognizes the one canonical pubkey, and fills it. That is the same auto-injection the counter relied on for `system_program`, one rung up: the bundle carries only the accounts you vary (the donor, the mint, the two token accounts), and the program-typed accounts fill themselves. Tokens didn't introduce a new rule; they exercised the rule you already had.

### Modeling tokens in the test

A token test needs a mint and accounts to hold it. The cast vocabulary builds them deterministically, so the test stays byte-stable:

```rust
{{#include ../../listings/counter/programs/counter/tests/test_donate.rs:test}}
```

`cast_mint` derives a mint from its name and creates it under Alice's authority; `fund_ata` creates an associated token account and mints into it (or leaves it empty at amount `0`, as Bob's is). Both alias their accounts, so a rendered tree reads `Alice` and `USDC`, not base58. The full catalog, the raw `create_token_mint` / `create_associated_token_account` / `mint_to` and their cast counterparts, is in [PDAs & Token Helpers](pdas-and-tokens.md).

This is still a [mechanics](end-to-end.md#mechanics-not-scenario) test: deterministic, snapshot-able, a CI gate. Tokens added accounts to model, not a new kind of test.

## Mainnet state (surfpool)

The counter ran in the in-memory engine. The same test runs against a live [surfnet](https://github.com/txtx/surfpool), a local validator that forks mainnet, and the change is one line.

```rust
{{#include ../../listings/counter/programs/counter/examples/surfpool.rs:example}}
```

The load-bearing line is the backend:

```rust
let mut ctx = AnchorContext::new(LiteSVM::new(), counter::ID)
    .with_backend(Box::new(RpcBackend::new(url)));
```

Drop it and the identical `tx().build().send_ok()` calls run in-memory; keep it and they route over JSON-RPC to the surfnet. The test never names an engine; the backend is a configuration, not a rewrite. That is the engine-independence the [spine chapter](end-to-end.md#mechanics-not-scenario) pointed at, made concrete: [`TestSVM`](../agents/backends.md) is the seam, `RpcBackend` is one implementation, and the in-memory engine is another. Three small things differ from the in-memory test, all visible above: the program is deployed out of process, the payer is airdropped over RPC rather than cast on a local engine, and the context is handed a backend.

It's an example, not a test, because a surfnet has to be running. Start one, deploy the program, run it:

```sh
surfpool start --no-tui                  # in book/listings/counter
solana program deploy target/deploy/counter.so \
  --program-id target/deploy/counter-keypair.json
cargo run --example surfpool --features rpc
```

Unlike the in-memory test, this one is not a deterministic snapshot: it airdrops a fresh payer each run (so a re-run gets a fresh PDA) and reads real forked state from the cluster. That is the trade a live environment makes, and the reason surfpool is a specialization you reach for to check against mainnet, not the default you snapshot in CI.
