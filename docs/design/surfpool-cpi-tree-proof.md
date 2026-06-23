# Proof artifact: AMM CPI tree from a live surfnet via RpcBackend

**Status:** captured artifact. The Task-3 proof, and the runtime validation of the
endpoint-agnostic `RpcBackend` (see `docs/design/endpoint-agnostic-architecture.md`).

## What this shows

The deployed `02-amm` program's `initialize` instruction, sent to a running
surfnet (`surfpool start --no-tui`) over JSON-RPC by anchor-litesvm's new
`RpcBackend` (`litesvm-utils/examples/rpc_amm.rs`). surfpool returns the logs;
the SAME `litesvm::cpi_tree` parser that renders in-memory trees renders these.
No special integration: one `ExecutionBackend` adapter, a real instruction with
deep CPIs.

It also exercises surfpool's mainnet fork: `mint_x`/`mint_y` are real USDC/USDT,
fetched from the datasource; the program `init`s the LP-mint PDA and three vault
ATAs, each a nested CPI to the token + ATA programs.

## The tree

```
AMM initialize (config DWm4MrK6YroDTYNXjcpjh7k5TLbriQDfkQjePWuAmLVc)
└── Initialize (87,865 / 200,000 CU) 5aDxxnPDGeVEuLXerisV4GF5f8tcwTjbMK7Bn1dYUXSi
    ├── 11111111111111111111111111111111
    ├── (201 / 185,666 CU) TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
    ├── (13,517 / 181,012 CU) ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL
    │   │ >> log:  Create
    │   │ >> log:  Initialize the associated token account
    │   ├── (183 / 175,591 CU) TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
    │   │     >> log:  Program return: Tokenkeg... pQAAAAAAAAA=
    │   ├── 11111111111111111111111111111111
    │   ├── (38 / 170,498 CU) TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
    │   └── (235 / 168,034 CU) TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
    ├── (16,517 / 159,878 CU) ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL
    │   └── ... nested token CPIs (vault_y ATA) ...
    ├── (16,517 / 135,745 CU) ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL
    │   └── ... nested token CPIs (lp_vault ATA) ...
    └── 11111111111111111111111111111111
```

`error: None`, `compute_units: 87,865`, `trace present: false` (no per-frame
trace over stock RPC; flips to present once surfpool surfaces the record).

## The same record, rendered aliased (consumer augmentation)

The `RpcBackend` record lifted into `TransactionResult`
(`impl From<ExecutionRecord> for TransactionResult`) and run through
anchor-litesvm's aliased renderer. Same execution, executor's raw frames above,
consumer's naming here:

```
Transaction  signers=[2MjToHk9…j91h]
└── AMM::Initialize [1] ✓ 77365cu  signer=2MjToHk9…j91h
    ├── System [2] ✓ (no cu)
    ├── Token [2] ✓ 201cu
    ├── AssociatedToken [2] ✓ 15017cu
    │   ├── Token [3] ✓ 183cu
    │   ├── System [3] ✓ (no cu)
    │   ├── Token [3] ✓ 38cu
    │   └── Token [3] ✓ 235cu
    ├── AssociatedToken [2] ✓ 13517cu
    │   └── … (vault_y) …
    ├── AssociatedToken [2] ✓ 13517cu
    │   └── … (lp_vault) …
    └── System [2] ✓ (no cu)
Legend (1):
  AMM = 5aDxxnPDGeVEuLXerisV4GF5f8tcwTjbMK7Bn1dYUXSi
```

Well-known program names (`System`/`Token`/`AssociatedToken`) come for free; the
program alias (`AMM`) is registered. This is the boundary principle live: litesvm
emits the base payload; the consumer (anchor-litesvm) names it, over RPC, exactly
as it would in memory.

## Capstone justification

- The litesvm CPI-tree capability, landed once in litesvm, is consumed by BOTH
  surfpool (its `--no-tui` render) and anchor-litesvm (this `RpcBackend`), sharing
  zero code. The litesvm-reach thesis, demonstrated end to end.
- anchor-litesvm scenarios can now target a real surfnet, not just in-process
  litesvm. The endpoint-agnostic direction is real, not hypothetical.
- The one fidelity gap (the per-frame trace) is exactly what a surfpool
  execution-record endpoint would close (the next collaborative seam).

## The round-trip: the test's aliases in surfpool's OWN render

The final piece. `ctx.alias("AMM")` (etc.) over a surfnet pushes
`surfnet_registerAlias` cheatcodes; surfpool accumulates them and renders its
*own* `--no-tui` CPI tree named by the client's aliases. Same execution, surfpool's
native stdout, labeled by the test's domain terms, **no raw pubkeys**:

```
└── Initialize (80,365 / 200,000 CU) AMM
    ├── System
    ├── (201 / 190,166 CU) Token
    ├── (13,517 / 185,512 CU) AssociatedToken
    │   ├── (183 CU) Token   ├── System   ├── (38 CU) Token   └── (235 CU) Token
    ├── (13,517 CU) AssociatedToken
    │   └── … Token/System/Token/Token …
    ├── (15,017 CU) AssociatedToken
    │   └── … Token/System/Token/Token …
    └── System
```

Three renderers now alias the same litesvm frames: anchor-litesvm (its `Aliases`),
surfpool (the client-pushed map via `surfnet_registerAlias`), and the in-memory
path. Structure lives once at the executor; naming lives in each consumer. Proven
live 2026-06-09.

## Running it yourself

This spans three repos, all on branch `feat/surfpool`: litesvm (the
`cds-rs/litesvm` fork), anchor-litesvm, and surfpool. The dogfood project is
`~/sol/02-amm` (an Anchor AMM) which consumes anchor-litesvm via a path dep, so
local changes are picked up with no publish step.

### Prerequisites

- All three repos on `feat/surfpool`. surfpool's `[patch.crates-io]` points
  `litesvm` at the fork branch (carries `cpi_tree` + `format_cpi_tree_with`).
- `~/sol/02-amm` built (`anchor build`, so `target/idl/amm.json` and
  `target/deploy/amm.so` exist), with `programs/amm/Cargo.toml` on
  `edition = "2021"` (the Anchor 1.0.2 IDL builder rejects `edition = "2024"`).

### In-memory only (no surfnet needed)

The `ExecutionBackend` port, the `From<ExecutionRecord>` bridge, and the
`AnchorContext` rewire are exercised in-process:

```sh
cd ~/oss/upstream/anchor-litesvm
cargo test -p litesvm-utils --lib backend       # the port + bridge unit tests
cargo test -p litesvm-utils -p anchor-litesvm   # full suites (in-memory path unchanged)
```

### Against a live surfnet

1. Build + install the surfpool binary (must be the `feat/surfpool` one, with
   `surfnet_registerAlias` + the legend):
   ```sh
   cd ~/oss/upstream/surfpool && cargo install --path crates/cli --locked --force
   ```
2. Start the surfnet **in the AMM project** (so the deploy runs and the program
   exists), with `--no-tui` (the CPI-tree render only lives in that path):
   ```sh
   cd ~/sol/02-amm && surfpool start --no-tui   # wait for "Runbook 'deployment' execution completed"
   ```
3. From anchor-litesvm, run an example (all need `--features rpc`):
   ```sh
   cd ~/oss/upstream/anchor-litesvm
   cargo run -p litesvm-utils  --features rpc --example rpc_smoke    # plumbing sanity (System transfer)
   cargo run -p litesvm-utils  --features rpc --example rpc_amm      # AMM tree: bare + aliased (consumer side)
   cargo run -p anchor-litesvm --features rpc --example ctx_rpc_amm  # ctx-over-RPC; pushes aliases to surfpool's render
   ```
   The example prints the anchor-litesvm-side render; the surfpool-side render
   (with the pushed aliases + the legend) appears in the `surfpool start --no-tui`
   terminal.

### Gotchas (each one we actually hit)

1. **`--features rpc` is required.** `RpcBackend` is feature-gated; without it the
   examples print "rebuild with `--features rpc`".
2. **Start surfpool *in* `~/sol/02-amm`**, not the surfpool repo dir. Elsewhere
   there is no `txtx.yml`/AMM, so nothing deploys and the examples have no program
   to invoke.
3. **Restart surfpool after rebuilding.** A running surfnet is still the *old*
   binary; `cargo install --force` only affects the next `surfpool start`.
4. **Airdrop can be rate-limited.** `rpc_amm`/`ctx_rpc_amm` use the surfnet's
   pre-funded deploy payer (`~/.config/solana/id.json`) rather than
   `request_airdrop`, for that reason.
5. **On surfpool's side, only the ids you pushed get named.** `ctx.alias(...)`
   forwards to `surfnet_registerAlias`; well-known programs are named for free on
   the anchor-litesvm side but stay raw on surfpool's unless pushed.
