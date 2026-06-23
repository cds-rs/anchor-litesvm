# The CPI Tree

This is the view you reach for first, and the one you'll read ninety percent of the time. `result.print_logs_structured()` turns a transaction's cross-program-invocation structure into an annotated box-drawing tree: who called whom, in what order, with what outcome and what cost. When a test fails, this is usually all you need to see.

> **Pinocchio:** the names in this tree come from different places. Anchor emits `Program log: Instruction: <Name>`, which the tree reads for free; a Pinocchio program logs only a discriminator byte, so names come from a registered `#[derive(Discriminator)]` table. Once registered, the tree reads identically. See [Testing Pinocchio Programs](../appendix/pinocchio.md).

Every example in this book renders its transactions this way. Here's the real output for an associated-token-account creation (the `account_graphs` example, which you can run yourself with `cargo run -p anchor-litesvm --example account_graphs`):

```text
── AssociatedToken::Create ─────────────────────────────────
Transaction  signers=[payer]
└── AssociatedToken [1] ✓ 15017cu  signer=payer
    ├── Token::GetAccountDataSize [2] ✓ 183cu
    ├── System::CreateAccount [2] ✓ (no cu)
    ├── Token::InitializeImmutableOwner [2] ✓ 38cu
    └── Token::InitializeAccount3 [2] ✓ 235cu
Compute Units (this run): 15017
Fee: 5000 lamports
Legend (1):
  payer = 5Aa8wUS2te5EYat2quxoyUngoPfxisxZj6SgTYNrXmh
```

## Anatomy

Read it top to bottom; it has three parts.

**The header.** The `── AssociatedToken::Create ──` line, filled to a fixed width with `─` so it reads as a section break rather than a label. It names the top-level instruction you issued (program plus decoded instruction name). Multi-instruction sends (batches) have no single header, so they omit this line and lead with the `Transaction` line instead.

**The body.** A box-drawing tree, one node per frame. (A *frame* is one invocation of a program: the top-level instruction opens the root, each CPI opens a nested child.) Reading the root frame line:

```text
└── AssociatedToken [1] ✓ 15017cu  signer=payer
```

- `AssociatedToken` is the program, in its alias name. (Without aliasing this would be base58; see [Accounts as Actors](../running/accounts-as-actors.md). The well-known programs come pre-aliased.)
- `[1]` is the **CPI depth**: `[1]` is a top-level instruction, `[2]` is a direct CPI, and so on. The four children above are all `[2]`: the `AssociatedToken` program called `Token` and `System` directly.
- `✓` is the outcome glyph: success. A failed frame shows `✗` with the error.
- `15017cu` is the compute consumed by this frame.
- `signer=payer` is annotated on **top-level frames only**. A CPI inherits its caller's authority, so repeating signers on every child would just be noise.

<div class="callout spotlight">

**N.B.** `signer=payer` means "payer is a required signer referenced in this instruction's accounts," not "payer authorized this specific call." The account list carries the relationship; intent isn't something the logs can tell you.

</div>

**The footer.** Three lines:

- `Compute Units (this run)`: the total for the transaction. The `(this run)` is a deliberate caveat, covered in [Reading Compute & Fees](compute-fees.md): per-frame CU drifts across runs, so don't diff this number between runs and conclude there's a regression.
- `Fee`: lamports.
- `Legend`: every *user* alias that actually appeared, in first-appearance order, mapped to its full pubkey. Well-known program names (System, Token, ...) are filtered out to keep it short; if you *rename* a well-known program, your name does show up, because the filter checks the name, not the pubkey.

## The two annotations people misread

**`(no cu)`** (on the `System::CreateAccount` line) is not a parser drop. Native programs don't emit a `consumed N of M` compute line, and rather than silently render nothing (which looks like a bug), the tree says `(no cu)` explicitly so you know the absence is real.

**Failed frames** carry the resolved error name, not the raw code. If a frame fails an Anchor constraint, you see `✗ EscrowExpired`, not `✗ custom program error: 0x1770`. The model lifts the Anchor `Error Code: <Name>` out of the frame logs for you, which is usually the single most useful thing in a failing test.

## The string variant

`print_logs_structured()` prints and returns `self` (so it chains). When you want the text instead, `logs_structured_string()` returns it as a `String`, which is what you'd assert against in a test or write to a file.

For getting the tree *and* a sequence diagram into a markdown file in one shot (a PR description, an issue), `print_markdown_pair()` wraps the tree in a ```console fence and the [lifelines diagram](mermaid.md) in a `<details>` block. The next chapter covers the diagram half.
