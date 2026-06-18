use std::fmt::Debug;
use std::path::{Path, PathBuf};

use super::block::MarkdownBlock;
use super::markdown::MarkdownRenderer;
use super::render::{ReportRenderer, Status};

/// One entry in the report, kept in the order the test produced it.
pub(super) enum Event {
    /// A sub-heading that introduces a phase of the scenario.
    Step(String),
    /// A paragraph of prose.
    Note(String),
    /// A render-ready view of some state captured at one instant.
    Snapshot { label: String, block: MarkdownBlock },
    /// An observed value compared against an expectation.
    Check {
        label: String,
        expected: String,
        actual: String,
        pass: bool,
    },
    /// A state change told as a story: what it was, what it became, what that
    /// means. Renders as a neutral table row (not a checklist item), so a
    /// report documenting a *violated* invariant doesn't read as a green
    /// feature list.
    Transition {
        label: String,
        before: String,
        expected: String,
        actual: String,
        meaning: String,
        pass: bool,
    },
    /// A collapsible `<details>` group: a labelled sub-sequence of events
    /// rendered inside an HTML disclosure. `<details>` is a *distinct bracket
    /// type* from a code fence, so a diagram or log block nested inside an act
    /// can't collide with the act's own open/close the way nested code fences
    /// would; the writer owns the `<details>` tags and the children own only
    /// their own fences.
    Container {
        summary: String,
        children: Vec<Event>,
    },
}

/// A recorder threaded through a test; emits its Markdown report on `Drop`.
///
/// Construct one at the top of a `#[test] fn`, narrate as the test runs, and
/// let it fall out of scope: the report is written (and the test is failed iff
/// any soft [`check`](Report::check) missed) in the [`Drop`] impl.
pub struct Report {
    pub(super) title: String,
    pub(super) intent: String,
    pub(super) events: Vec<Event>,
    emitted: bool,
    /// Set by [`disarm`](Report::disarm) in tests that build a `Report` without
    /// wanting file output or panic escalation on drop.
    #[cfg(test)]
    pub(super) disarmed: bool,
    /// `Some(reason)` iff the test declared, via
    /// [`expect_panic`](Report::expect_panic), that it is a parked TDD spec for
    /// a known panic. Flips the emitted status from a surprise `ABORTED` to a
    /// deliberate `RED (expected)`.
    pub(super) expected_panic: Option<String>,
}

impl Report {
    pub fn new(title: impl Into<String>, intent: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            intent: intent.into(),
            events: Vec::new(),
            emitted: false,
            #[cfg(test)]
            disarmed: false,
            expected_panic: None,
        }
    }

    // --- intent channel ---------------------------------------------------

    /// A sub-heading introducing the next phase ("Before", "Action", "After").
    pub fn step(&mut self, heading: impl Into<String>) -> &mut Self {
        self.events.push(Event::Step(heading.into()));
        self
    }

    /// A paragraph of explanatory prose.
    pub fn note(&mut self, prose: impl Into<String>) -> &mut Self {
        self.events.push(Event::Note(prose.into()));
        self
    }

    // --- structure channel ------------------------------------------------

    /// Capture a render-ready view of some state at this instant.
    pub fn snapshot(&mut self, label: impl Into<String>, value: &impl super::block::ToMarkdown) -> &mut Self {
        self.events.push(Event::Snapshot {
            label: label.into(),
            block: value.to_markdown(),
        });
        self
    }

    /// Attach a pre-rendered block directly (e.g. a `Fenced` block of logs).
    pub fn block(&mut self, label: impl Into<String>, block: MarkdownBlock) -> &mut Self {
        self.events.push(Event::Snapshot {
            label: label.into(),
            block,
        });
        self
    }

    /// Append the authority story: the test's "who signed what, and which PDAs
    /// the program signed as via `invoke_signed`" flow, captured by
    /// [`AnchorContext::authority_story`](../../anchor_litesvm/struct.AnchorContext.html).
    /// Rendered as an "Authority flow" section.
    pub fn authority(&mut self, story: &impl super::block::ToMarkdown) -> &mut Self {
        self.events.push(Event::Snapshot {
            label: "Authority flow".to_string(),
            block: story.to_markdown(),
        });
        self
    }

    /// Record an observed value against an expectation.
    ///
    /// SOFT by default: a mismatch does NOT panic here, so every check in the
    /// body runs and appears in the report. If any check failed, the test is
    /// failed at `Drop` time. This is deliberate: when something breaks you want
    /// the whole observed picture, not just the first failing line.
    pub fn check<T: PartialEq + Debug>(
        &mut self,
        label: impl Into<String>,
        expected: T,
        actual: T,
    ) -> &mut Self {
        self.events.push(Event::Check {
            label: label.into(),
            expected: format!("{expected:?}"),
            actual: format!("{actual:?}"),
            pass: expected == actual,
        });
        self
    }

    /// HARD variant: record, then panic immediately on mismatch. For
    /// preconditions where continuing would be meaningless (a setup invariant
    /// that must hold before the interesting part runs).
    pub fn require<T: PartialEq + Debug>(
        &mut self,
        label: impl Into<String>,
        expected: T,
        actual: T,
    ) -> &mut Self {
        let pass = expected == actual;
        self.check(label, expected, actual);
        assert!(pass, "report precondition failed; see report");
        self
    }

    // ANCHOR: transition
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
    // ANCHOR_END: transition

    /// Group a sub-sequence of events into a collapsible `<details>` disclosure
    /// titled `summary`. Assemble the group's body in the closure; it renders
    /// inside the act, well-nested by construction: the act owns its
    /// `<details>` brackets, the children own only their own fences, and the two
    /// are different bracket types so they cannot collide.
    ///
    /// This is the structured alternative to hand-building a fenced string and
    /// passing it as a [`block`](Report::block): there the caller fences their
    /// own content and a stray delimiter can leak; here the writer owns every
    /// bracket, so an act carrying a ```` ```mermaid ```` diagram and a log dump
    /// stays a single well-formed disclosure.
    ///
    /// ```no_run
    /// # use testsvm::report::{Report, MarkdownBlock};
    /// # let mut md = Report::new("t", "i");
    /// md.act("Act 1: open the session", |a| {
    ///     a.note("Alice opens the vault, Bob funds it.");
    ///     a.block("diagram", MarkdownBlock::raw("```mermaid\nsequenceDiagram\n...\n```"));
    /// });
    /// ```
    pub fn act(
        &mut self,
        summary: impl Into<String>,
        build: impl FnOnce(&mut ActBuilder),
    ) -> &mut Self {
        let mut a = ActBuilder { events: Vec::new() };
        build(&mut a);
        self.events.push(Event::Container {
            summary: summary.into(),
            children: a.events,
        });
        self
    }

    /// Declare this report a parked TDD spec for a *known* panic, pairing with
    /// `#[should_panic]` on the test fn. `#[should_panic]` keeps `cargo test`
    /// green while the bug exists; `expect_panic(reason)` keeps the *report*
    /// honest: instead of a surprise `ABORTED`, the heading reads
    /// `RED (expected)` and carries `reason`. If a fix stops the panic,
    /// `#[should_panic]` fails the test ("did not panic"), so a stale spec
    /// surfaces; this call does not itself panic, which would mask that flip.
    ///
    /// ```no_run
    /// # use testsvm::report::Report;
    /// #[should_panic(expected = "Transaction failed")]
    /// # fn _example() {
    /// let mut md = Report::new("Stake re-freezes the asset", "known bug, fix pending");
    /// // ... the body that panics on the bug ...
    /// md.expect_panic("stake re-adds the FreezeDelegate; fix pending");
    /// # }
    /// ```
    pub fn expect_panic(&mut self, reason: impl Into<String>) -> &mut Self {
        self.expected_panic = Some(reason.into());
        self
    }

    /// Suppress the `Drop` emit and failure escalation, so tests that build a
    /// `Report` without wanting a file written or a panic on drop can clean up
    /// without side-effects.
    #[cfg(test)]
    pub(crate) fn disarm(&mut self) {
        self.emitted = true;
        self.disarmed = true;
    }

    pub(super) fn failures(&self) -> usize {
        self.events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::Check { pass: false, .. } | Event::Transition { pass: false, .. }
                )
            })
            .count()
    }

    /// The verdict, given whether `Drop` fired during a panicking unwind.
    /// `aborted` is a parameter (not read from `thread::panicking()` here) so it
    /// is unit-testable; `flush` passes `std::thread::panicking()`.
    pub(super) fn status(&self, aborted: bool) -> Status {
        match (&self.expected_panic, aborted) {
            (Some(reason), true) => Status::RedExpected(reason.clone()),
            (None, true) => Status::Aborted,
            _ => match self.failures() {
                0 => Status::Pass,
                n => Status::Failed(n),
            },
        }
    }

    // Thin shim retained so existing call sites/tests compile; delegates to the
    // markdown renderer with the computed status.
    #[allow(dead_code)]
    pub(super) fn render(&self, aborted: bool) -> String {
        MarkdownRenderer.render(self, self.status(aborted))
    }

    fn flush(&mut self) {
        if self.emitted {
            return;
        }
        self.emitted = true;
        let status = self.status(std::thread::panicking());
        emit_report(&self.title, &MarkdownRenderer.render(self, status));
    }
}

impl Drop for Report {
    fn drop(&mut self) {
        self.flush();

        // Escalate soft-check failures into a real test failure, BUT only if
        // we're not already unwinding. Panicking *during* a panic aborts the
        // process (libtest prints nothing useful, and no other report gets
        // flushed). So: if a hard failure (a bad tx, a `require`, a panic from
        // anywhere) already brought us here, the report is written and that
        // original panic is what fails the test; we stay quiet.
        let fails = self.failures();
        #[cfg(test)]
        if self.disarmed {
            return;
        }
        if fails > 0 && !std::thread::panicking() {
            panic!("{}: {fails} check(s) failed; see report", self.title);
        }
    }
}

/// Builds the body of a [`Report::act`] group. Mirrors the narrative subset of
/// [`Report`]'s recording methods; the act's `<details>` brackets are emitted
/// by the writer, so a body assembled here is always well-nested.
pub struct ActBuilder {
    pub(super) events: Vec<Event>,
}

impl ActBuilder {
    /// A sub-heading inside the act.
    pub fn step(&mut self, heading: impl Into<String>) -> &mut Self {
        self.events.push(Event::Step(heading.into()));
        self
    }

    /// A paragraph of prose inside the act.
    pub fn note(&mut self, prose: impl Into<String>) -> &mut Self {
        self.events.push(Event::Note(prose.into()));
        self
    }

    /// Capture a render-ready view of some state inside the act.
    pub fn snapshot(&mut self, label: impl Into<String>, value: &impl super::block::ToMarkdown) -> &mut Self {
        self.events.push(Event::Snapshot {
            label: label.into(),
            block: value.to_markdown(),
        });
        self
    }

    /// Attach a pre-built block inside the act.
    pub fn block(&mut self, label: impl Into<String>, block: MarkdownBlock) -> &mut Self {
        self.events.push(Event::Snapshot {
            label: label.into(),
            block,
        });
        self
    }
}

/// Resolve the directory reports are written to, robustly across project
/// layouts. Precedence:
///   1. `MD_REPORTS_DIR` env var (explicit override).
///   2. `CARGO_TARGET_DIR` env var + `md-reports` (honors a custom target dir).
///   3. Walk up from the runtime `CARGO_MANIFEST_DIR` to the first ancestor that
///      already contains a `target/` directory, and use `target/md-reports`
///      there. This finds the workspace target from a member crate (e.g.
///      `programs/foo`) as well as a standalone crate's own target.
///   4. Last resort: `target/md-reports` relative to the current directory.
///
/// We resolve at runtime (not via the `env!` macro) precisely so a library can
/// host this: `env!("CARGO_MANIFEST_DIR")` would bake in *this* crate's path,
/// whereas the env var is set by cargo to the crate under test.
fn reports_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("MD_REPORTS_DIR") {
        return PathBuf::from(explicit);
    }
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        return Path::new(&target).join("md-reports");
    }
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let mut dir: &Path = Path::new(&manifest);
        loop {
            if dir.join("target").is_dir() {
                return dir.join("target").join("md-reports");
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
        return Path::new(&manifest).join("target").join("md-reports");
    }
    PathBuf::from("target").join("md-reports")
}

/// Write the report to `<reports_dir>/<slug>.md` and echo it to stdout.
///
/// Each test runs on its own thread and writes its own slug-named file, so
/// there's no contention and no ordering race; a later step can concatenate the
/// files in sorted (i.e. deterministic) order. The filename is a slug of the
/// title and the content carries no timestamps, so the artifact is diffable and
/// commit-friendly.
fn emit_report(title: &str, body: &str) {
    let dir = reports_dir();
    let _ = std::fs::create_dir_all(&dir);

    let slug: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let _ = std::fs::write(dir.join(format!("{slug}.md")), body);

    // Also echo to stdout so `cargo test -- --nocapture` shows it inline.
    println!("{body}");
}
