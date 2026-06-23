# The Mechanism, End to End

The four chapters before this one each took the bundle from one side. This one is all of them at once, on the smallest program that exercises the whole mechanism: a counter. One PDA, two instructions (`initialize` and `increment`), no tokens. Every block below is included from [`book/listings/counter`](https://github.com/cds-rs/anchor-litesvm/tree/turbin3/book/listings/counter), a crate CI builds and tests; clone it and `cargo test` to run it yourself.

It's a counter on purpose. Tokens, mints, ATAs, mainnet forks are *specializations* you layer on once the shape is in your hands; the spine stays bare so nothing competes with the macro and the bundle for your attention.

> **Pinocchio:** the counter is an Anchor program, and so is everything this chapter wires up. The same harness, observability, and assertions drive a Pinocchio program; what differs is the bundle and derive sugar. [Testing Pinocchio Programs](../appendix/pinocchio.md) is the counterpart.

## The source layout

A bundle-driven test spreads across four files, and exactly one of them carries the macro. Hold the map before the code:

```
programs/counter/src/
  state.rs                      the account the program stores
  instructions/initialize.rs    #[derive(Accounts)] + the cfg_attr   <- the macro lives here
  lib.rs                        #[program], the handlers
  test_helpers.rs               #[derive(Bundle)], the pubkey bundle  <- host-only
tests/
  test_counter.rs               build, send, assert
```

## Where the instruction is defined, and how the bundle ties in

The account the program stores is plain Anchor:

```rust
{{#include ../../listings/counter/programs/counter/src/state.rs:state}}
```

The instruction's context is an `#[derive(Accounts)]` struct. The `cfg_attr` stacked above it is the tie-in, and the one line in this chapter you most need to place correctly: it puts `BundledPubkeys` on the struct, and `bundled_with(...)` names the bundle to bridge to.

```rust
{{#include ../../listings/counter/programs/counter/src/instructions/initialize.rs:accounts}}
```

`system_program` is a field here (Anchor needs it to create the PDA), but watch for it to be *absent* from the bundle two sections down: it auto-injects. The handler is ordinary Anchor, and the `#[program]` module wires it to its context:

```rust
{{#include ../../listings/counter/programs/counter/src/instructions/initialize.rs:handler}}
```

```rust
{{#include ../../listings/counter/programs/counter/src/lib.rs:program}}
```

## Where the helper file goes

The bundle is a flat struct of pubkeys, one named field per account you vary. It lives in a host-only `test_helpers` module, alongside the program rather than inside it:

```rust
{{#include ../../listings/counter/programs/counter/src/test_helpers.rs:bundle}}
```

Two things keep it off the chain: it derives `Bundle` (a host-only macro), and `lib.rs` declares the module under the same target gate, so it vanishes on the BPF build.

```rust
{{#include ../../listings/counter/programs/counter/src/lib.rs:wire}}
```

The path in `bundled_with(crate::test_helpers::InitializeBundle)` points at exactly this struct, at exactly this location. That path is the source-location contract: name the bundle, name where it lives, and the macro does the rest. The second instruction, `increment`, repeats the whole shape, its own `Accounts` struct, its own bundle (above), and no `system_program`, since it creates nothing:

```rust
{{#include ../../listings/counter/programs/counter/src/instructions/increment.rs:increment}}
```

## The test

With the program and the bundles in place, the test is short:

```rust
{{#include ../../listings/counter/programs/counter/tests/test_counter.rs:test}}
```

`cast_actor_with_sol` derives Alice from the program id and her name, funds her, and aliases her; `get_pda` derives the counter the same way the program will; `build` projects the bundle and serializes the args; `send_ok` sends and asserts success; `load` reads the account back. `system_program` never appears in either bundle, yet `initialize` creates the PDA: that is the auto-injection, doing its job invisibly.

## Mechanics, not scenario

This is a **mechanics** test, and the distinction runs through the whole book.

| | mechanics | scenario |
|---|---|---|
| shape | arrange, act, assert | a cast of actors, narrated through a `Report` |
| answers | does it work, did it regress | who did what to whom, and was that allowed |
| output | deterministic, so it diffs clean | a CPI tree, a sequence diagram, an audit you can hand to someone |
| home | this chapter | [the worked examples](../examples/escrow.md) and [Auditing](../agents/auditing.md) |

Because every identity here is cast (derived, not random), the test's output is byte-stable across runs. That is what makes it a mechanics test in the useful sense: commit the output and CI fails the moment behavior drifts (the [snapshot gate](../intro/determinism.md#in-ci-the-snapshot-is-a-regression-gate)). When you want the other mode, a transaction read as a story for an audit or a shared document, you reach for a scenario; the [worked examples](../examples/vault.md) build them. And the test above ran on litesvm, but nothing in it names an engine: the same source feeds an RPC backend or mollusk unchanged (see [Backends](../agents/backends.md)).

## How it works

The mechanism is name resolution, and this example shows it without an override, the common case. `BundledPubkeys` sits on `Initialize`, so the derive looks up two entries Anchor filed under that same name: `accounts::Initialize` (the account list) and `instruction::Initialize` (the args). It bridges the bundle to each, a `From<InitializeBundle>` for the accounts and a `BuildableIx` for the args, which is why `build(accs, vix::Initialize { .. })` typechecks and a mismatched pairing would not. The struct is named `Initialize` and the handler is `fn initialize`, so `PascalCase(initialize) == Initialize`, the lookup lands, and no `instruction = ...` override is needed.

Auto-injection is the other half: the derive reads `system_program`'s Anchor type, `Program<'info, System>`, recognizes its one canonical pubkey, and fills it, which is why the bundle doesn't carry it.

That is the whole contract, witnessed on one compiling crate. The general rules, what to do when the names *don't* match, how to share one bundle across structs that disagree on optionality, and the rest of the derive family, are in [Bundled Pubkeys](bundled-pubkeys.md).
