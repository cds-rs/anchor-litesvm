# Reference

A curated "which tool and why," organized by the object you hold. This is
not a signature dump: docs.rs is the authoritative source for exact
signatures; this page tells you which method to reach for and points at the
example chapter that exercises it.

## `AnchorLiteSVM` (the builder)

The first line of every test: deploy the program(s), get back a ready
`AnchorContext`.

- **`build_with_program(id, name, &bytes)`**: deploy one program. `name` is
  registered as an alias, so a failing tree names the program instead of its
  raw pubkey. Used throughout [Vault](examples/vault.md) and
  [Escrow](examples/escrow.md).
- **`build_with_programs(&[(id, name, &bytes), ...])`**: deploy several
  programs in one call, aliasing each. Reach for this when the program under
  test CPIs into another one your test must also deploy, as
  [Stake](examples/stake.md) does for `mpl-core`. The first entry becomes the
  context's primary `program_id`.

## `AnchorContext` (the World)

The cast/setup surface. See [The World](concepts/world.md) and
[Setup](concepts/setup.md) for the narrative version.

- **`cast_actor(name)`**: a deterministic, 100-SOL-funded, aliased signer.
  The default way to bring an actor ("Alice", "Bob", "Mallory") into a
  scenario. See [Aliases & Actors](concepts/aliases.md).
- **`cast_actor_with_sol(name, lamports)`**: same as `cast_actor`, with an
  explicit lamport balance instead of the 100 SOL default. Reach for it when
  a scenario asserts on an exact SOL amount.
- **`cast_mint(name, &authority, decimals)`**: casts a token mint under
  `authority`, aliased `name`. Used throughout [Escrow](examples/escrow.md)
  (`MintA`, `MintB`).
- **`fund_ata(&owner, &mint, &authority, amount)`**: creates `owner`'s
  associated token account for `mint`, mints `amount` into it, and aliases
  the ATA `"<owner>/<mint>"`. The funded-holder setup Escrow uses for both
  sides of the trade.
- **`alias(pubkey, name)`** / **`alias_ata(&owner, &mint)`**: register a
  pubkey (or a derived ATA) under a name directly, for accounts that didn't
  come from a `cast_*` call.
- **`register_events_from_idl(idl_json)`**: registers a decoder for every
  event an Anchor IDL declares, so `emit!`ed logs decode into typed values and
  render as `🔔` badges. Vault's `Deposited` event uses this (see
  [Vault](examples/vault.md)).
- **`register_program_errors(program_id, &[(code, name), ...])`**: names a
  program's custom error codes by hand. The tool for programs without an IDL:
  Stake's `mpl-core`-dependent program can't feed `declare_program!`, so this
  is the only way a failing leaf reads `FreezePeriodNotElapsed` instead of
  `custom program error: 0x1770` (see [Stake](examples/stake.md)).
- **`tx(signers)`**: starts the fluent build-and-send chain. See
  [Sending](#sending) below.
- **`load(&address)`** / **`try_load(&address)`**: deserialize an Anchor
  account at `address`. `load` panics on failure (missing account, wrong
  discriminator, deser error), the idiomatic choice in a test where that
  failure is itself the test failing; `try_load` returns a `Result` for
  callers that want to handle it. `load_unchecked` / `try_load_unchecked`
  skip the discriminator check.

## Sending

Every send asserts something specific, so a glance at the call tells a
reader what the test expects:

| Method | Asserts | Reach for it when |
|---|---|---|
| `send_ok(ix, signers)` | transaction succeeds | the happy path |
| `send_err(ix, signers)` | transaction fails, any error | the outcome alone is the contract (an authorization check, a generic constraint trip) and pinning to a specific error name would over-constrain the test |
| `send_err_named(ix, signers, "Name")` | transaction fails *and* the failure resolves to (or its logs contain) `"Name"` | you know exactly which error should fire, e.g. `"EscrowExpired"`, `"ConstraintSeeds"` |

All three live on `AnchorContext` as one-shot calls
(`ctx.send_ok(ix, &[&signer])`) and as the fluent chain's terminators:

```rust
ctx.tx(&[&signer])
   .build(bundle, args)
   .send_ok()
   .print_logs();
```

The `Tx` chain (`ctx.tx(signers).build(bundle, args)`) is worth it once a
test builds and sends several instructions, since `build`/`build_with` share
the same terminators and `remaining_accounts` appends a dynamic account tail.
For a single one-off send, the one-shot `ctx.send_ok(ix, signers)` skips the
chain entirely. Both return the same `TransactionResult`. See
[Structured Logs](concepts/structured-logs.md#sending-send_ok--send_err--send_err_named).

## Building instructions

- **`program().build_ix(bundle, args)`**: the honest instruction. Derives
  every account from the bundle, no overrides. The path every happy-path call
  in this book uses.
- **`program().build_ix_with(bundle, args, |accounts| ...)`**: same
  derivation, plus a closure to override exactly one account before it's sent.
  The negative-path escape hatch: construct the instruction an attacker would
  submit (a valid-but-wrong account swapped in) without hand-rolling every
  other account yourself. See the [Vault escape hatch](examples/vault.md#the-escape-hatch)
  (`vault_state` swapped for Mallory's own) and the
  [Escrow escape hatch](examples/escrow.md#the-escape-hatch) (`vault` swapped
  for Mallory's ATA).

`bundles_from_idl!(program_name)` generates, from a committed IDL, one
`<Ix>Bundle` struct per instruction (a plain-pubkey field per account the
caller must supply), a `<account>_pda(...)` helper per derivable PDA, and a
module-level `injected_programs()` listing every fixed program address the
IDL pins. The bundle's `From` impl derives PDAs and injects fixed accounts, so
`build_ix` only needs the accounts that vary per call. See
[Setup](concepts/setup.md) and the [Quickstart](quickstart.md).

When the program's IDL can't feed `declare_program!` / `bundles_from_idl!` at
all (the anchor-version wall: [Stake](examples/stake.md)'s dependency on
`mpl-core` pins it to anchor 0.31, while the host workspace is anchor 1.0),
drop to hand-built `solana_instruction::Instruction`s: compute the 8-byte
discriminator as `sha256("global:<name>")[..8]`, list account metas in the
program's `#[derive(Accounts)]` order, and derive PDAs by hand with
`find_program_address`. This is exactly what `bundles_from_idl!` generates
for you when an IDL is available.

## Reading output

- **`print_logs()`**: prints the run's CPI tree to stdout, returns `self` so
  it chains at the end of a call.
- **`tree_string()`**: same rendering, returned as a `String` instead of
  printed. Every captured `.txt` fixture in this book is a `tree_string()`
  capture, verbatim. See [Structured Logs](concepts/structured-logs.md) for
  the full anatomy.
- **`EventHelpers::parse_event::<T>()`** / **`parse_events::<T>()`**:
  deserialize a specific Anchor event type out of the logs (the first one, or
  all of them). Vault's deposit test checks `ev.amount` this way (see
  [Vault](examples/vault.md)).
- **`assert_event_emitted::<T>()`** / **`assert_event_count::<T>(n)`**:
  assert an event of type `T` was (or wasn't, or was emitted exactly `n`
  times) without pulling the value out yourself.

## Clock & state helpers (`TestHelpers`)

Methods on `ctx.svm` (a `LiteSVM`), for time-locked program logic and token
state:

- **`advance_days(n)`** / **`advance_seconds(n)`**: move the Clock sysvar's
  `unix_timestamp` forward. The tool for time-locked constraints: Escrow's
  90-day expiry and Stake's 7-day freeze period both drive their negative and
  positive paths by advancing past (or short of) the deadline. See
  [Escrow's time-lock](examples/escrow.md#time-lock) and
  [Stake's freeze lock](examples/stake.md#freeze-lock).
- **`warp_to_timestamp(unix_timestamp)`**: set the Clock's `unix_timestamp`
  to an absolute value, when a test needs a deterministic wall-clock point
  rather than a relative jump.
- **`token_balance(&ata)`**: read an SPL Token account's amount; `None` if no
  account exists there (so a post-close assertion reads `is_none()` rather
  than a panic).
- **`create_token_mint(&authority, decimals)`**: create and initialize a
  token mint with a freshly generated keypair. `cast_mint` (on
  `AnchorContext`) is the aliased, deterministic-keypair wrapper most
  scenarios reach for instead; use this directly when you don't need either.
