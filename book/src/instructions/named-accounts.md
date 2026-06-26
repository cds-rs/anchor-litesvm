# Named Accounts (No More Ordering)

## The common thread: the bundle

One idea runs through all of Part II: the **bundle**. To see why it's worth a name, start with what an instruction *is* on Solana.

A program can't reach for an account on its own. The runtime hands it exactly the accounts the instruction names, in a fixed list, and nothing else (this is Solana's account model: no ambient access, every account passed explicitly). So an instruction comes paired with an account set: the accounts it needs present to execute on-chain. That set isn't optional metadata; it's part of the instruction's identity. `make` stripped of its escrow, vault, and mints isn't a smaller `make`; it's a transaction the runtime won't run.

The **bundle** is an ergonomic name for that account set: a plain struct holding the pubkeys your test varies, one named field per account. Raw LiteSVM makes you spell the set as an ordered `Vec<AccountMeta>` (the [next section](#the-problem-ordered-vectors)); the bundle lets you name it instead. Same set the runtime requires, named rather than counted.

This chapter is about the first pain the bundle removes, and the one you'll feel first in raw LiteSVM: account ordering.

## The problem: ordered vectors

A Solana instruction carries its accounts as an ordered list. Raw, you build that list yourself, as a `Vec<AccountMeta>`, in exactly the order the program expects:

```rust
// Raw LiteSVM: you MUST get the order exactly right
let instruction = Instruction {
    program_id,
    accounts: vec![
        AccountMeta::new(maker.pubkey(), true),    // position 0
        AccountMeta::new(escrow_pda, false),        // position 1
        AccountMeta::new_readonly(mint_a, false),   // position 2
        AccountMeta::new_readonly(mint_b, false),   // position 3
        AccountMeta::new(maker_ata_a, false),       // position 4
        AccountMeta::new(vault, false),             // position 5
        // ... more, all position-sensitive
    ],
    data: instruction_data,
};
```

The order matters, and it's invisible. Swap positions 4 and 5 and, if you're lucky, the transaction fails with "invalid account"; if you're unlucky, it succeeds against the wrong accounts and corrupts state. The failure modes all trace to the same root:

- Swap two accounts: transaction fails, or worse, silently does the wrong thing.
- Miss an account: every subsequent position is off by one.
- The program adds an account: you hunt for the right insertion point or everything shifts.
- The program reorders its accounts: every test needs a manual, error-prone update.

This is the number-one bug source in raw Solana testing, and it's pure ceremony: the position carries no meaning a human cares about.

## The fix: a named bundle

With `anchor-litesvm` you fill in a named struct, a **bundle**, instead, and **order doesn't matter**:

```rust
let accs = MakeBundle {
    // any order you like; the names are what bind
    vault,
    maker: maker.pubkey(),
    escrow: escrow_pda,
    mint_b: mint_b.pubkey(),
    mint_a: mint_a.pubkey(),
    maker_ata_a,
};
let ix = ctx.program().build_ix(accs, vix::Make { seed: 42, amount: 1_000_000, receive: 500_000 });
```

Rearrange those fields however reads best; it compiles to the same instruction, because the program defines the canonical order through its own `ToAccountMetas`, not your memory. And notice the bundle is *shorter* than the raw `Vec`: there's no `token_program`, no `system_program`, no `associated_token_program`. Those have exactly one canonical pubkey each, so the bundle doesn't carry them; they're filled in for you (the [next chapters](builder.md) show how). The bundle holds only the pubkeys your test actually varies.

What you get from the switch:

1. **Type safety**: the compiler insists every required account is present.
2. **Named fields**: each account's purpose is on the page, not implied by its index.
3. **Order independence**: rearrange freely.
4. **Refactor safety**: if the program changes its account set, your test stops *compiling* (a loud, immediate failure) instead of failing mysteriously at runtime.
5. **IDE support**: autocomplete lists the required fields.
6. **Fewer bugs**: no hand-built `Vec`, no positions to miscount.

> **Pinocchio:** the bundle works because Anchor's `#[derive(Accounts)]` generates the `ToAccountMetas` that knows the order. A Pinocchio program generates nothing of the kind, so you order the account metas by hand; the naming win here is Anchor's, the engine underneath is shared. See [Testing Pinocchio Programs](../appendix/pinocchio.md).

## Where this goes next

You've met one face of the bundle: filling it in by name. Three chapters cover the rest, each a different face of the same struct: you *build* an instruction from it ([the builder](builder.md), which also shows where the struct comes from), you *populate* its fields with PDA and token helpers ([PDAs & tokens](pdas-and-tokens.md)), and one derive *wires* it to your program and to the [rendered output](bundled-pubkeys.md), covering auto-injection, the optionality fixups, and the rest of the family. The foundation under all four is this: you name accounts, the program orders them, and a whole category of bug disappears.
