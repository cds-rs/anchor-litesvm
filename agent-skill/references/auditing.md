# Auditing

A different mode from the rest of the corpus: not a developer writing feature
tests, but an auditor producing a *finding* another auditor can verify. The
deliverable is a byte-stable `Report`, so the claim is something you point at,
not something you assert.

## Two tiers of observability

Which tier reaches your target is a property of the code, not a choice.

| tier | reaches | how |
|---|---|---|
| engine-grounded | only code the SVM executes | CPI trees, structured logs, the engine as oracle |
| report-narrated | any host-callable code, including a library the engine never runs | `Report` + `transition` + `expect_panic` |

A bug in a parser, codec, or math crate with no on-chain entrypoint is
invisible to the engine until some program *uses* it. Until then, the
report-narrated tier is the one that reaches it, and a `Report` is the unit of
a verifiable audit claim.

## The engine as oracle

When the target does have a runtime counterpart, let the engine establish
ground truth and assert the target agrees with it. For a parser of signed
data: the engine accepts a real signature and rejects a forged one, and the
parser must read back exactly the bytes the runtime judged.

```rust
let tx = svm.send(&[real_ix], &[&payer]);
assert!(tx.error.is_none());               // the runtime verified it
let parsed = Target::parse(&data)?;        // the target must agree
assert_eq!(parsed.signer(), expected);

// The scandal: a forged input the runtime rejects may still parse cleanly.
let tx = svm.send(&[forged_ix], &[&payer]);
assert!(tx.error.is_some());               // the runtime caught it
let parsed = Target::parse(&forged)?;      // ...but a parser is not a verifier
```

That gap (parses but does not verify) is itself a finding: a consumer must
gate on the runtime's verdict, never on parse success.

## The finding as a Report

Name the threat in the title and intent, show the crafted input, and assert.
For a *state change*, especially a violated invariant, a `- [x]` checklist
reads as "all good"; use `transition`, whose rendered before/after row is the
finding. The diff is the evidence.

```rust
    /// Record a state change as before -> after -> what it means, with teeth.
    ///
    /// For reports that document a transition (especially a *violated*
    /// invariant), [`check`](Report::check)'s checklist rendering works
    /// against the reader: `- [x]` rows of confirmed violations read as a
    /// passing feature list. `transition` renders a neutral table instead
    ///
    /// | Observation | Before | After | What it means |
    /// |---|---|---|---|
    /// | yes_votes | 0 | 255 | `-= 1` underflowed |
    ///
    /// and still asserts: `actual_after` is compared against
    /// `expected_after`, SOFT like `check` (recorded now, the test fails at
    /// `Drop` if any row missed), so presentation and enforcement never
    /// split. Consecutive `transition` calls collapse into one table.
    pub fn transition<T: PartialEq + Debug>(
        &mut self,
        label: impl Into<String>,
        before: T,
        expected_after: T,
        actual_after: T,
        meaning: impl Into<String>,
    ) -> &mut Self {
        self.events.push(Event::Transition {
            label: label.into(),
            before: format!("{before:?}"),
            expected: format!("{expected_after:?}"),
            actual: format!("{actual_after:?}"),
            meaning: meaning.into(),
            pass: expected_after == actual_after,
        });
        self
    }
```

A structural-trust finding falls straight out of it: parse a sensible input,
then one with a single field edited, and let the table show the silent change.

```rust
md.transition(
    "reported signer (head)",
    head(&sensible),        // before
    head(&tampered),        // expected (we predict the unsafe behavior)
    head(&tampered),        // actual
    "one field edit re-points the signer; the parse succeeds either way",
);
```

The test passes because it *confirms* the predicted behavior; the title and
the "what it means" column carry the judgment. A passing audit test means
"reproduced", and the committed report is the reproduction.

## Known-unsafe paths

Some findings are a panic or worse. Park a defined panic as a deliberate
`RED (expected)` artifact with `expect_panic` (pair it with
`#[should_panic]`), so the report records it without the run aborting.

```rust
#[test]
#[should_panic]
fn the_unguarded_path_aborts() {
    let mut md = Report::new("title", "intent");
    md.expect_panic("empty input indexes data[0]; no length guard precedes it");
    let _ = Target::parse(&[]);   // aborts here; the report flushes RED (expected)
}
```

For undefined behavior, do not run it. A test must not invoke UB (an
out-of-bounds read may pass, segfault, or do anything). Describe it in the
report's intent and demonstrate the *guard's absence* by contrast: the same
input that is UB in one build configuration is a clean rejection in another.

## Assembling the audit

Collect the per-finding `Report`s into one committed artifact. A `just audit`
recipe wipes `target/md-reports/`, runs the finding suite (both sides of any
feature seam, since the contrast is the finding), and concatenates the files
in `LC_ALL=C` order behind a `GENERATED, do not edit` banner. Because every
finding is deterministic, the artifact is byte-stable: regenerating it is a
no-op unless a finding changed, so its diff is a change in findings.
