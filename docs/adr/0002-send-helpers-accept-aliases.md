# ADR-0002: `send_ok` / `send_anchor_err` accept `&Aliases`

- **Status:** Superseded by [ADR-0003](0003-aliases-on-context-and-result.md) (the `&Aliases` parameter survives on the bare-LiteSVM `TransactionHelpers` trait, but `TransactionResult::print_logs_structured` and `AnchorContext::send_*` now read aliases from storage instead of taking them per call).
- **Date:** 2026-05-23
- **Implemented:** 2026-05-24 (commit `dafd372`)
- **Source:** dogfooding `#[derive(BundledPubkeys)]` against `~/sol/voting`

## Context

The shortcut helpers in `crates/litesvm-utils/src/transaction.rs` print the
structured CPI tree on the failure path, so a test author who has built an
alias map (mapping `Pubkey`s to readable names) sees readable output when an
assertion fires:

```rust,ignore
// transaction.rs (current, abridged)
fn send_ok(&mut self, instruction: Instruction, signers: &[&Keypair]) -> TransactionResult {
    let result = self.send_instruction(instruction, signers).expect("...");
    if !result.is_success() {
        eprintln!("\nsend_ok: transaction failed, structured CPI tree:");
        result.print_logs_structured(&Aliases::default());   // <- hardcoded
    }
    result.assert_success();
    result
}

fn send_anchor_err(&mut self, instruction: Instruction, signers: &[&Keypair], anchor_error: &str) {
    let result = self.send_instruction(instruction, signers).expect("...");
    let error_matches = /* ... */ ;
    if !error_matches {
        eprintln!("\nsend_anchor_err: assertion will fail, structured CPI tree:");
        result.print_logs_structured(&Aliases::default());   // <- hardcoded
    }
    result.assert_anchor_error(anchor_error);
}
```

Both helpers print the structured CPI tree on the failure path (and `send_ok`
returns the result so the happy path can chain
`.print_logs_structured(&my_aliases)`). But the failure-path prints use
`Aliases::default()` unconditionally, so any alias map the test author built
is dropped exactly when it's most useful.

The result is a forced choice in negative-path tests: use `send_anchor_err`
and lose alias-aware diagnostics, or drop down to `send_instruction` + a
manual `print_logs_structured(&aliases)` + `assert_anchor_error` chain and
duplicate the helper's whole body. We hit this in `test_vote.rs` for the two
`Voting{NotStarted,Ended}` boundary tests; the workaround was to keep
`send_anchor_err` and accept default-aliased output if either assertion ever
fires.

## Considered options

Two dimensions to settle: where the aliases live in the call (the *call-site
shape*) and what type the helpers actually accept (the *argument type*). They
compose; pick one from each.

### Call-site shape

**A. Add the argument to existing helpers.** Simplest; no parallel API to
maintain. The crate is greenfield, so there are no callers to break:

```rust,ignore
fn send_ok(&mut self, instruction: Instruction, signers: &[&Keypair], aliases: &Aliases)
    -> TransactionResult;
fn send_anchor_err(&mut self, instruction: Instruction, signers: &[&Keypair],
                   aliases: &Aliases, anchor_error: &str);
```

**B. `_with` suffix variants.** Non-breaking by construction; the bare forms
keep defaulting to `Aliases::default()`:

```rust,ignore
fn send_ok_with(&mut self, instruction: Instruction, signers: &[&Keypair],
                aliases: &Aliases) -> TransactionResult;
fn send_anchor_err_with(&mut self, instruction: Instruction, signers: &[&Keypair],
                        aliases: &Aliases, anchor_error: &str);
```

Useful if the crate had public callers; here it just doubles the API surface.

### Argument type

Four candidates:

| Type                  | `send_ok(ix, &[&s], X)` accepts                                  | Clone? | "Default" shorthand                              |
|-----------------------|------------------------------------------------------------------|--------|--------------------------------------------------|
| `&Aliases`            | `&Aliases::default()`, `&my_aliases`                             | no     | caller writes `&Aliases::default()`              |
| `impl AsRef<Aliases>` | `Aliases::default()`, `&my_aliases`, `my_aliases`                | no     | caller writes `Aliases::default()` (one fewer char) |
| `impl Into<Aliases>`  | same as `AsRef`, plus `()` via `From<()> for Aliases`            | yes    | `send_ok(ix, &[&s], ())`                         |
| `Option<&Aliases>`    | `Some(&my_aliases)`, `None`                                      | no     | `None`                                           |

- **`&Aliases`** is the boring choice. It matches the shape that
  `TransactionResult::print_logs_structured` already uses, so test authors
  who move between the two stay in the same idiom. No surprises, no traits
  to read.
- **`impl AsRef<Aliases>`** is the most ergonomic without paying for it. The
  `Aliases` struct just needs a trivial `impl AsRef<Aliases> for Aliases`,
  and the helper takes both `&Aliases` and `Aliases` without cloning (it
  only ever needs `&Aliases` internally, so it never owns or clones).
- **`impl Into<Aliases>`** is the most flexible: with `From<()> for
  Aliases` returning the default, `send_ok(ix, &[&s], ())` becomes the "I
  don't care about names" shorthand. The cost is that the common case ("I
  built an `aliases` map in setup and want every call to use it") either
  writes `my_aliases.clone()` at each call (visible) or relies on
  `From<&Aliases> for Aliases` cloning behind `.into()` (hidden). Either
  way it pays a `HashMap<Pubkey, String>` clone per call to support a niche.
- **`Option<&Aliases>`** says "default" loudest: `None` means default, the
  caller has to acknowledge the choice, no clone. Cost: mild ceremony at
  every call site that *does* have aliases (the common case).

### Rejected alternative: thread-local / context-bound aliases

A "set once on the context, helpers read from there" design (e.g.
`AnchorContext::with_aliases(...)` plus implicit lookup in the helpers) would
collapse the API change but adds hidden state. Tests that build different
alias maps per scenario (`UserAccounts` vs `Pool` actors in `02-amm`'s
style) would have to remember to reset between calls. Explicit `aliases:`
per call matches the pattern `print_logs_structured` already uses on
`TransactionResult` and stays predictable.

## Decision

**Shape: A** (add the argument to existing helpers).
**Type: `&Aliases`** (the boring choice).

Rationale (YAGNI):

- `&Aliases` matches the shape `TransactionResult::print_logs_structured`
  already uses, so test authors moving between the two stay in the same
  idiom.
- `AsRef` / `Into` solve ergonomics problems we don't have yet; we can widen
  to `impl AsRef<Aliases>` later without breaking existing call sites (the
  borrow conversion is a non-breaking widening).
- No clones, no traits in the public surface, no `Option` ceremony.

Final shape:

```rust,ignore
fn send_ok(&mut self, instruction: Instruction, signers: &[&Keypair], aliases: &Aliases)
    -> TransactionResult;
fn send_anchor_err(&mut self, instruction: Instruction, signers: &[&Keypair],
                   aliases: &Aliases, anchor_error: &str);
```

Implementation: swap `Aliases::default()` for the passed value. The
happy-path return of `send_ok` keeps working; tests that want the alias map
on success still chain `.print_logs_structured(&aliases)`, and tests that
want it on failure now get it for free.

## Outcome

Shipped in commit `dafd372` exactly as decided. Both helper signatures
in `crates/litesvm-utils/src/transaction.rs` gained `aliases: &Aliases`,
and the failure-path `print_logs_structured(&Aliases::default())` calls
were swapped for `print_logs_structured(aliases)`. Eight existing call
sites in `crates/litesvm-utils/src/transaction/tests.rs` were updated
to pass `&crate::Aliases::default()` (the tests don't build alias maps,
so they take the explicit-default path). Doc updates in
`crates/anchor-litesvm/src/lib.rs` and `EVALUATING.md` cover the new
signatures.

**Subsequent rename:** `send_anchor_err` was later renamed to
`send_err_named` when the negative-path helper family grew to include
`send_err` (no name). The pair (`send_ok` / `send_err`) covers
outcome-only assertions; `send_err_named(name)` is the named-substring
variant. The "anchor" prefix had always been shorthand for "this is
the form Anchor's error names take", but the underlying matching is
just substring on logs + the error field, so the qualifier was
misleading. The references in the body of this ADR still read
`send_anchor_err` to preserve the original decision context.

## Consequences

- Negative-path tests get alias-aware structured logs on assertion failure
  without dropping to the manual `send_instruction` + `print_logs_structured`
  + `assert_anchor_error` chain.
- Callers who don't care about aliases write `&Aliases::default()` at the
  call site (one extra argument). Acceptable given the greenfield status of
  the crate.
- Future widening to `impl AsRef<Aliases>` stays open and non-breaking if
  the explicit-default ceremony grows annoying in practice.

## Implementation pointers

- `crates/litesvm-utils/src/transaction.rs::TransactionHelpers::send_ok`
  and `::send_anchor_err`.
- `TransactionResult::print_logs_structured` is the existing alias-aware
  primitive; both helpers delegate to it with the caller's aliases on
  the failure path.
- Tests updated: `crates/litesvm-utils/src/transaction/tests.rs` (8 call
  sites).
- Docs updated: `crates/anchor-litesvm/src/lib.rs` (module-level example),
  `EVALUATING.md` (summary table + two worked examples).
