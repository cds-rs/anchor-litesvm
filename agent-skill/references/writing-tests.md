# Writing Tests

One canonical suite shape. The code below is the escrow listing, compiled and
run in CI; adapt names, keep the structure.

## Anatomy

1. **World**: one `setup` function builds everything the scenario needs and
   returns one struct. The suite owns it (`tests/common/mod.rs`).
2. **Cast list**: inside setup, every account is created deterministically and
   aliased before any instruction runs.
3. **Act**: `ctx.tx(&signers).build(bundle, args).send_ok()` (or
   `send_err_named` for an expected failure).
4. **Assert**: `md.check(label, expected, actual)` records and asserts in one
   call; account state comes from `ctx.load` (`ctx.try_load` for a `Result`),
   balances from `ctx.svm.token_balance` / `ctx.svm.get_balance`.

## World setup (the scenario verb)

```rust
// tests/common/mod.rs
pub fn setup(ctx: &mut AnchorContext, seed: u64) -> EscrowWorld {
    // The trade this scenario sets up: the maker locks DEPOSIT of mint A and
    // wants RECEIVE of mint B in return; a taker holding mint B fills it. We
    // build both sides so a test can run make / take / refund against a real
    // cast.

    // The two parties. `cast_actor` rolls the deterministic keypair, the SOL
    // airdrop, and the alias into one call, so each name appears once.
    let maker = ctx.cast_actor("Maker");
    let taker = ctx.cast_actor("Taker");

    // The two tokens being traded, at distinct decimals so a take bug that
    // confused mint A with mint B couldn't hide. Each party is the authority
    // for the mint it brings. `cast_mint` derives the mint at a deterministic
    // address, creates it, and registers the leaf alias ("A", "B").
    let mint_a = ctx.cast_mint("A", &maker, MINT_A_DECIMALS);
    let mint_b = ctx.cast_mint("B", &taker, MINT_B_DECIMALS);

    // What each party brings to the trade, funded in their own ATA: the maker's
    // mint-A deposit (what `make` locks away) and the taker's mint-B payment
    // (what `take` hands over).
    let maker_ata_a = ctx.svm.create_associated_token_account(&mint_a, &maker).unwrap();
    ctx.svm.mint_to(&mint_a, &maker_ata_a, &maker, DEPOSIT).unwrap();
    let taker_ata_b = ctx.svm.create_associated_token_account(&mint_b, &taker).unwrap();
    ctx.svm.mint_to(&mint_b, &taker_ata_b, &taker, RECEIVE).unwrap();

    let maker_key = maker.pubkey();
    let taker_key = taker.pubkey();

    // Addresses the program owns or creates, derived now but not yet on chain:
    // `escrow` is the PDA holding the terms; `vault` is the escrow's own ATA for
    // mint A, custodying the locked deposit; `taker_ata_a` and `maker_ata_b` are
    // the settlement destinations, created `init_if_needed` during `take`, so we
    // only derive their addresses here.
    let escrow = ctx
        .svm
        .get_pda(&[escrow::ESCROW_SEED, maker_key.as_ref(), &seed.to_le_bytes()], &escrow::ID);
    let vault = get_associated_token_address(&escrow, &mint_a);
    let taker_ata_a = get_associated_token_address(&taker_key, &mint_a);
    let maker_ata_b = get_associated_token_address(&maker_key, &mint_b);

    // The full cast as pubkeys: the bundle every instruction builds from.
    let bundle = EscrowBundle {
        maker: maker_key,
        taker: taker_key,
        mint_a,
        mint_b,
        maker_ata_a,
        maker_ata_b,
        taker_ata_a,
        taker_ata_b,
        escrow,
        vault,
    };

    // The leaves named themselves as they were cast (Maker, Taker, A, B); name
    // the escrow PDA, then compose each token-account name from its owner and
    // mint with `alias_ata`, so the trace reads "Maker/A", "Escrow/A" (the
    // vault), and so on. The cast order already aliased every leaf before these
    // ATAs compose off them.
    ctx.alias(escrow, "Escrow");
    ctx.alias_ata(&maker_key, &mint_a); // Maker/A
    ctx.alias_ata(&maker_key, &mint_b); // Maker/B
    ctx.alias_ata(&taker_key, &mint_a); // Taker/A
    ctx.alias_ata(&taker_key, &mint_b); // Taker/B
    ctx.alias_ata(&escrow, &mint_a); // Escrow/A (the vault)

    EscrowWorld { bundle, maker, taker, mint_a, mint_b }
}
```

The shortcut form: `ctx.cast_actor("maker")` replaces the registry dance
(deterministic keypair, 100 SOL, aliased), `ctx.cast_actor_with_sol(name,
lamports)` casts at an exact stake, and `ctx.cast_account("recipient")` covers
passive accounts. For token plumbing, `ctx.cast_mint(name, &authority, decimals)`
casts a mint and `ctx.fund_ata(&owner, &mint, &authority, amount)` hands a holder
a balance in its aliased ATA, each in one call.

## Happy path, narrative shape

```rust
// tests/test_make.rs
#[test]
fn make_creates_escrow_and_funds_vault() {
    let mut md = Report::new(
        "Escrow: make creates the escrow and funds the vault",
        "The maker opens an escrow offering `deposit` of mint_a in exchange for \
         `receive` of mint_b. `make` records the terms in the escrow account and \
         moves the full deposit from the maker's source ATA into the vault \
         (an ATA owned by the escrow PDA).",
    );

    let mut ctx = AnchorLiteSVM::build_with_program(escrow::ID, "escrow", PROGRAM_SO);
    let w = setup(&mut ctx, SEED);

    md.step("Before: maker holds the deposit, vault does not exist yet");
    md.snapshot("balances", &balances(&ctx, &w));

    md.step("Action: maker calls make(seed, receive, deposit)");
    ctx.tx(&[&w.maker])
        .build(
            w.bundle,
            escrow::instruction::Make { seed: SEED, receive: RECEIVE, deposit: DEPOSIT },
        )
        .send_ok()
        .print_markdown_pair();

    md.step("After: escrow records the terms; the deposit sits in the vault");
    md.snapshot("balances", &balances(&ctx, &w));

    // The escrow account round-trips the instruction args. If a future change
    // shuffles `state::Escrow`, these checks pin the layout contract for `make`.
    let escrow_acct: escrow::Escrow = ctx.load(&w.bundle.escrow);
    md.check("escrow.seed", SEED, escrow_acct.seed);
    md.check("escrow.maker", w.bundle.maker, escrow_acct.maker);
    md.check("escrow.mint_a", w.bundle.mint_a, escrow_acct.mint_a);
    md.check("escrow.mint_b", w.bundle.mint_b, escrow_acct.mint_b);
    md.check("escrow.receive", RECEIVE, escrow_acct.receive);

    // The full deposit moved maker -> vault; checking both ends catches a
    // transfer with the wrong amount or direction.
    md.check("vault holds the deposit", Some(DEPOSIT), ctx.svm.token_balance(&w.bundle.vault));
    md.check("maker source drained", Some(0), ctx.svm.token_balance(&w.bundle.maker_ata_a));
}
```

The plain Arrange // Act // Assert shape is this test minus the `Report`:
build the context, call setup, send, then `assert_eq!` on the same accessors.
Prefer the narrative shape in a suite; the Markdown reports it writes under
`target/md-reports/` double as committed, byte-reproducible baselines.

## Expected failure, with the clock

```rust
// tests/test_refund.rs
#[test]
fn refund_fails_before_expiry() {
    let mut md = Report::new(
        "Escrow: refund is rejected before expiry",
        "refund is the mirror of take's gate: allowed only after the window \
         closes. Inside the window it must fail with EscrowNotExpired, so a maker \
         cannot yank the deposit out from under a taker who is mid-flight.",
    );

    let mut ctx = AnchorLiteSVM::build_with_program(escrow::ID, "escrow", PROGRAM_SO);
    let w = setup(&mut ctx, SEED);

    md.step("Setup: maker opens the escrow");
    ctx.tx(&[&w.maker])
        .build(
            w.bundle,
            escrow::instruction::Make { seed: SEED, receive: RECEIVE, deposit: DEPOSIT },
        )
        .send_ok()
        .print_markdown_pair();

    md.step("Advance only 19 days (comfortably inside the 90-day window)");
    ctx.svm.advance_days(19);

    md.step("Action: maker calls refund while still live → must fail");
    let rejection = ctx
        .tx(&[&w.maker])
        .build(w.bundle, escrow::instruction::Refund {})
        .send_err_named("EscrowNotExpired");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: the deposit is still escrowed");
    md.snapshot("balances", &balances(&ctx, &w));
    md.check("vault still holds the deposit", Some(DEPOSIT), ctx.svm.token_balance(&w.bundle.vault));
    md.check("escrow account still open", true, ctx.account_exists(&w.bundle.escrow));
}
```

`send_err_named("EscrowNotExpired")` asserts the failure by its Anchor error
name; no error-code arithmetic. `ctx.svm.advance_days(19)` moves the clock
(seconds/slots variants exist; `warp_to_timestamp` pins an absolute time).

`send_err_named` is Anchor-context sugar. A suite on the bare `TestSVM`
trait (a Pinocchio program on mollusk, say) asserts on
`model::Transaction::error` instead; see
[Backends](backends.md#asserting-failure-at-the-trait-level).

## Policy loops

Each helper-mediated send is its own transaction under a fresh blockhash, so
a rate-limit or spend-cap test resends the identical instruction in a plain
loop; no blockhash management appears anywhere in the test:

```rust
for _ in 0..3 {
    ctx.tx(&[&payer])
        .build(w.bundle, program::instruction::Spend { amount: 10 })
        .send_ok();
}
```

## Events

Register a program's events once, at setup, and the structured views render
them by name with destructured, alias-substituted fields (`🔔 Transfer { from:
maker, amount: 100 }`) instead of the raw `Program data:` blob. One line pulls
every event from the IDL:

```rust
ctx.register_events_from_idl(include_str!("../../target/idl/my_program.json"));
```

Per type instead of the whole IDL: `ctx.register_event::<my_program::events::Transfer>()`
(the event must `#[derive(Debug)]`). Registration rides on the backend, so every
send afterward carries it and the events appear in the CPI tree, the mermaid
note, and so in `report_execution`; nothing is attached per result.

A program that emits via self-CPI rather than a `Program data:` log (an
`emit_cpi!` engine, or a Pinocchio program on the bare `TestSVM` trait) registers
a decoder through `register_cpi_event(program_id, prefix, name, decoder)`
instead; the program id is part of the key, since the self-CPI tag is shared
across anchor-compatible programs.

## Snapshots

End a transacting test with `ctx.report_execution(&mut md)` to append the
execution overview (CPI tree, named actors, compute). Because every actor is
deterministic, the file is byte-stable and belongs in version control as a
regression baseline.

`report_execution` (and `spotlight`) are main-line only. On the
`compat/anchor-0.31` line, assemble the report from the log-based pieces it
does have, `logs_structured_string()`, `mermaid_string()`, and
`print_markdown_pair()`, since the trace-based execution snapshot isn't
available there. See [Anchor Version Compatibility](https://github.com/cds-rs/anchor-litesvm/blob/turbin3/book/src/appendix/anchor-compat.md).

When you splice a rendered piece into a `Report` by hand with `md.block`, match
the block type to the content. `MarkdownBlock::Fenced { lang, body }` wraps
*plain* text in a fresh code fence (the CPI tree, a log dump). `MarkdownBlock::Raw(..)`
splices a fragment that already carries its own fence, verbatim. The graph and
mermaid strings (`authority_graph_string`, `ownership_graph_string`,
`mermaid_string`) are already ` ```mermaid ` blocks, so they go in as `Raw`;
wrapping one in `Fenced` nests it inside a `text` code block, and it renders as
source instead of a diagram.
