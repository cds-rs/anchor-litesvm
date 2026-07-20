# For Agents

Machine-first documentation for AI agents writing or fixing tests with
anchor-litesvm. Every code block on these pages is included from a compiling,
CI-tested crate. Humans are welcome, but the book proper (Parts I through V)
tells the same story with more explanation.

If you arrived holding an error message, go straight to
[Failure Modes](failure-modes.md) and search for the exact string.

## Connecting an agent

These pages are published in agent-native forms alongside the book, with all
code includes resolved:

- **Claude Code** (one line; per-project, replace `~` with the project root):

  ```bash
  curl -fsSL https://cds-rs.github.io/anchor-litesvm/agent-skill.tar.gz | tar -xz -C ~/.claude/skills
  ```

- **Any agent**: `https://cds-rs.github.io/anchor-litesvm/llms.txt` indexes
  the corpus as fetchable markdown.

Do not point an agent at the raw `book/src/agents/*.md` files on GitHub: the
raw markdown still contains unresolved `{{#include}}` directives where the
code should be. The published forms above are assembled from these pages on
every deploy.

## Vocabulary

| term | meaning |
|---|---|
| world | everything a scenario needs, built by one setup function and returned as one struct |
| actor | a named signer: deterministic keypair, funded, aliased (`ctx.cast_actor("maker")`) |
| prop | a named non-signing account with fabricated state (`prop`, or `prop_mint` / `prop_token_account` for SPL state) |
| cast | derive + fund + alias a named account in one call (`actor`/`cast_actor`, `cast_actor_with_sol`, `cast_mint`, `fund_ata`); a duplicate cast name panics |
| alias | a `pubkey -> name` registration; every rendered output substitutes the name |
| cast list | the discipline: every account a test touches is named before the first send |
| bundle | a struct of pubkeys deriving `Bundle`/`BundledPubkeys`; converts into Anchor accounts structs, injecting any `Program<'info, T>` |
| scenario verb | a suite-owned function that performs one named step (`setup`, `open_session`) |
| Report | the narrative object: steps, snapshots, and `check` assertions, written to `target/md-reports/<slug>.md` |
| story | frood's substrate: mints a `Moment` per transaction, samples every registered observation into it |
| trajectory | the ordered moments; a frood report is this, projected once at world drop |
| observation | a labeled read registered once (`story.observe`), sampled at every moment; laws (`monotonic`/`latch`/`constant`) watch one |
| finally | a terminal fact settled at the story's conclusion (`count_actions`, law status) |
| report standard | `ReportConfig::of([cast(), sequence(Lifelines), ...])`, declared once beside the world; a test attenuates its own copy via `report_state().config_mut()` |
| event | a program event registered once (`register_events_from_idl` / `register_event::<E>()`), rendered by name and destructured, aliased fields in the structured views |
| TestSVM | the backend trait: one vocabulary, one engine per build |
| finding | an audit claim told as a byte-stable `Report` another auditor can reproduce |

## Decision: what are you writing?

| goal | go to |
|---|---|
| a feature test for a program | [Writing Tests](writing-tests.md) |
| a verifiable security finding / auditing existing code | [Auditing](auditing.md) |

## Decision: test shape

| condition | shape |
|---|---|
| default | narrative: thread a `Report`, `md.step` each step, `md.check` as assertions |
| quick unit-style check | plain Arrange // Act // Assert: same calls, no `Report` |
| documenting a state change / violated invariant | `md.transition(before, after, meaning)`, not a `check` checklist |
| program is a committed `.so` + Codama IDL (no crate) | frood story world: `when_ok`/`when_err` verbs, observations + laws, projection at drop (see [Writing Tests](writing-tests.md), "Narrative on frood") |

Both shapes use the identical execution surface; the `Report` is additive.
See [Writing Tests](writing-tests.md).

## Decision: dependencies

| program under test | dependency shape |
|---|---|
| Anchor program | git dep on `anchor-litesvm`, host-only via target cfg; no direct `litesvm` or `solana-*` test deps |
| Pinocchio program | optional dep + `testing` cargo feature; release/SBF graph stays clean |
| same contract, second engine | separate crate outside the workspace; engines never share a lockfile |

See [Dependencies](dependencies.md).

## Decision: backend

| situation | backend |
|---|---|
| default (Anchor or Pinocchio, in-memory) | litesvm (`AnchorLiteSVM` context or `LiteSvmBackend`) |
| instruction-level harness for a Pinocchio program | `testsvm-mollusk` (own build) |
| test against forked or live cluster state | `RpcBackend` + a surfnet endpoint (feature `rpc`) |

See [Backends](backends.md).

## Rules that prevent rework

- Name every account before the first send; rendered output is only as
  readable as the cast list.
- Actors and props are deterministic (derived from their name): committed
  snapshot files diff clean. Do not use `Keypair::new()` for a named role.
- Each helper-mediated `send` is its own transaction with a fresh blockhash;
  resend loops need no blockhash management.
- Keep `token_program` in account structs used for token CPIs, even where
  Anchor would let you drop it.
- PDAs are named by their role in the scenario ("SpendCap"), never by their
  slot ("policy_2").
