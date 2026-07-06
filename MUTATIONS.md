# External mutations log

The agent works freely inside this repo but never mutates anything outside it
(`.claude/settings.local.json` denies writes to the external source trees it reads
from, e.g. `/Users/amal/sol/capstone` and `/Users/amal/oss/mine`). Anything the
agent would otherwise change outside the repo is recorded here for you to action
later, rather than done silently.

## Pending external mutations

- **avm (global anchor toolchain), transient + restored.** Building `examples/staking` (anchor 0.31.1) required `avm use 0.31.1`; the agent restored `avm use 1.0.2` afterward. Net-neutral (the active version is back to 1.0.2). Reproducible without manual action because `examples/staking/Anchor.toml` pins `[toolchain] anchor_version = "0.31.1"`, so `anchor build` auto-switches via avm. Nothing for you to action; recorded because avm state lives outside the repo.

## In-repo additions sourced from outside (provenance)

These are writes **into** this repo (allowed), recorded so the fixtures are
regenerable and their origin is traceable.

- `crates/anchor-litesvm/tests/fixtures/mpl_core.so` (810,504 bytes, sha256
  `4a5cc50d…`) copied from `/Users/amal/oss/mine/vogon-registry/mpl-core/fixtures/mpl_core.so`,
  itself a mainnet dump of program `CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d`
  (`solana program dump -u m`). Needed as the mpl-core CPI callee for the stake
  example.
