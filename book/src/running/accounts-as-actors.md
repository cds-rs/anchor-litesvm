# Accounts as Actors

This is the idea the rest of the book leans on, so it gets its own short chapter before the worked examples put it to work.

A Solana transaction is, mechanically, a list of pubkeys and the roles they play (signer, writable, program). In a raw test those pubkeys are base58 noise: you generate a keypair, you pass its `.pubkey()` around, and when something fails you're staring at `4Nd1mB...` in a log and trying to remember which account that was. The test knows the *roles* (this one's the maker, that one's the vault), but that knowledge lives only in your head and your variable names; the tooling can't see it.

The premise here is to make that knowledge first-class. Every account a test touches is an **actor** with a name and a role, declared up front, and the tooling renders its work back to you in those names.

## Two mechanisms

**Aliases** give a pubkey a name. You register them during setup:

```rust
ctx.alias(maker.pubkey(), "maker");
ctx.alias(escrow_pda, "escrow");
ctx.alias(vault, "vault");
```

From then on, anything that renders the transaction (the CPI tree, the Mermaid diagram, the authority and ownership graphs in [Part IV](../inspect/cpi-tree.md)) substitutes the name for the base58. The account you named in setup is the account you see in the output. That round trip, name in, name out, turns debugging from a pubkey-matching exercise into reading.

**The cast verbs** fuse the whole declaration into one line. `cast_actor` gives you a signer that is deterministic (derived from the program id and the name, so [committed output diffs clean](../intro/determinism.md)), funded, and aliased:

```rust
let maker = ctx.cast_actor("maker");      // deterministic keypair, 100 SOL, aliased "maker"
let recipient = ctx.cast_account("recipient"); // passive pubkey, rent-funded, aliased
```

`cast_actor` funds with **100 SOL**; when a scenario asserts exact balances (a spend cap, a fee split), treat that as the float and set the precise stake explicitly afterwards. `cast_account` is for the non-signers: a recipient, a target, anything that only needs to exist and have a name.

Derived accounts join the cast the same way, named **by their role in this scenario** rather than by what they structurally are. The third policy PDA is not "policy_2"; it is whatever it is *here*:

```rust
let spend_cap = derive_policy(session, 0);
ctx.alias(spend_cap, "SpendCap");          // the report reads by role
```

**The cast list** is the discipline that makes aliases pay off: declare *every* account a test touches as a named actor at the top of the scenario, before any instruction runs. Not just the interesting ones; the system program and the token program get names too, so the rendered output has no anonymous nodes. A reader who scans the top of the test learns the entire cast before the first instruction runs.

## Why bother

Two payoffs. The immediate one is debugging: a failing transaction renders as a tree of named actors doing named things, so you read the failure instead of decoding it. The structural one is that a scenario built around a cast describes *who did what to whom*, which is usually the thing under test; the instructions become verbs over nouns you already introduced, and that alone surfaces test-logic bugs (a missing signer, the wrong writable account) by making the cast visible.

<div class="callout spotlight">

**N.B.** The cast-list discipline has a house style for how output gets labeled; that's the domain of [Test-Output Conventions](../appendix/conventions.md) in the appendix. This chapter is the "why"; that one is the "exactly how it's spelled".

</div>

## Where it shows up

Every worked example in [Part V](../examples/vault.md) opens by introducing its cast, then builds the scenario from it, then renders the result in those names. Vault has a `depositor` and a `vault`; Escrow has a `maker`, a `taker`, an `escrow`, and a `vault`; CPAMM adds an `lp`, a `trader`, a `pool`, and the two mints. Read those three in order and the pattern becomes muscle memory.
