# Installation & Setup

This book assumes you already have an Anchor workspace. If you're starting from nothing, `anchor init my_program` scaffolds one (program crate, `Anchor.toml`, the works); everything below then happens inside that program crate. We'll use `my_program` as the program name throughout; substitute your own.

## 1. Add the dependencies

Two dependencies, and the second one is the interesting one.

```toml
[dependencies]
anchor-lang = "1.0.2"
anchor-spl = "1.0.2"   # only if your program makes token CPIs

# Host-only: pulled in for `cargo test` builds, never compiled into the
# on-chain (BPF) binary.
[target.'cfg(not(target_os = "solana"))'.dependencies]
anchor-litesvm = { git = "https://github.com/cds-rs/anchor-litesvm", branch = "turbin3" }
```

Two things deserve explanation.

First, `anchor-litesvm` is a **normal dependency**, not a `dev-dependency`, but scoped with `[target.'cfg(not(target_os = "solana"))'.dependencies]` so it only compiles for host (test) builds, not the BPF program. 

<details> <summary>Why not a plain <strong>dev-dependency</strong>? </summary>

> Because the derives that make tests pleasant (`Bundle`, `BundledPubkeys`) generate trait impls that, by Rust's orphan rule, have to live in your program crate, next to your account structs. A `dev-dependency` isn't visible to `src/`; a target-scoped normal dependency is, while still staying out of the deployed binary. The next chapter shows exactly what lands in `src/`.
</details>

Second, there's no `[dev-dependencies]` section at all.

<details> <summary>Where are the <strong>solana-*</strong> crates, then? </summary>

> The test reaches everything it needs (`Keypair`, `Pubkey`, `Signer`, the harness, the assertion and token helpers) through the `anchor-litesvm` facade re-exports, so you don't list `solana-*` crates separately. One `use anchor_litesvm::...` and you're done.
</details>

<details> <summary>Which <strong>litesvm</strong> runs your tests? </summary>

> The one the facade pins: `anchor-litesvm` brings its own `litesvm` (the fork the framework is built against), and `ctx.svm` is that instance. Don't add a second `litesvm` to your own `[dev-dependencies]`: a crates.io copy is a *different package version* from the framework's, so it neither shares types with `ctx.svm` nor turns features on for it; it just sits in your tree confusing future readers. If you need a litesvm capability the framework doesn't expose yet, that's a framework issue to file, not a dependency to add.

</details>

<details> <summary>Does your program make <strong>token CPIs</strong>? </summary>

If your program makes token CPIs (the [Tokens specialization](../instructions/specializations.md) builds one), extend the `idl-build` feature so the IDL generator can see the SPL types:

```toml
[features]
idl-build = ["anchor-lang/idl-build", "anchor-spl/idl-build"]
```

(`anchor-spl` tracks `anchor-lang`'s version: with `anchor-lang 1.0.x`, use `anchor-spl 1.0.x`.)

</details>

## 2. Build the program

```bash
anchor build
```

This compiles the program to `target/deploy/my_program.so`, which your test loads with `include_bytes!`. Rebuild whenever you change the program: the test embeds the bytes at compile time, so a stale `.so` means you're testing yesterday's program.

That's the whole setup. Notice what's **not** here: no `declare_program!`, no `idls/` directory to populate, no client codegen step. Tests in this harness call your program through its own crate (its generated `instruction::*` types and your account structs), so the IDL never enters the loop. There's also no validator to install, no RPC to mock, no keypair files to manage. The next chapter writes a complete test against this.
