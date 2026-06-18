//! `Report`: a test recorder that emits a Markdown narrative.
//!
//! Two channels, interleaved in author order:
//!   - intent    (`note`, `step`): prose; what regime we're in and why.
//!   - structure (`snapshot`, `check`): values observed from the running test.
//!
//! The structural channel is the trustworthy one: every number in it is the
//! same number the test asserted on, so the report can't quietly disagree with
//! the code the way a stale comment can. Prose can drift; observed values can't.
//!
//! This complements `print_markdown_pair` (the per-transaction renderer on the
//! litesvm `TransactionResult`) rather than replacing it. That method renders
//! one *transaction* (its instruction + logs); a `Report` renders one
//! *scenario*: the intent, the before/after state, and the pass/fail checks, as
//! a single committable document per test.
//!
//! This module is domain-agnostic: it knows nothing about your program's
//! accounts. Domain types earn a place in a report by implementing
//! [`ToMarkdown`]; assemble the per-scenario files (one per test, named by a
//! slug of the title) into a single document with a `just`-style concat step.

use std::fmt::{Debug, Write as _};
use std::path::{Path, PathBuf};

/// One entry in the report, kept in the order the test produced it.
enum Event {
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

/// A self-contained, render-ready Markdown fragment.
///
/// By the time a value becomes a `MarkdownBlock`, every `Pubkey` should already
/// be resolved to an alias name: nothing run-varying (base58 keys, timestamps)
/// should survive into it, so the rendered report is byte-stable across runs and
/// safe to commit. That determinism is the *implementor's* contract (see
/// [`ToMarkdown`]), not something this layer can enforce.
pub enum MarkdownBlock {
    /// A table with a header row and zero or more body rows.
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    /// *Literal* content shown as a fenced code block: program logs, a captured
    /// dump, anything you want displayed verbatim. The `body` is the content
    /// alone and must NOT carry its own fence: the writer materializes the
    /// backtick fence at render time and widens it to out-tick any backtick run
    /// inside `body` (see [`fence_delim`]). Because the delimiter is *computed
    /// from the content* rather than stored, a body that itself contains
    /// ```` ``` ```` cannot close the wrapper early, so this block can never
    /// double-fence itself.
    Fenced { lang: String, body: String },
    /// Markdown spliced *verbatim* into the document: a fragment you want
    /// rendered (headings, a real ```` ```mermaid ```` diagram), not shown as
    /// literal text. The caller owns the fragment's nesting; the writer keeps
    /// the document well-nested regardless by checking the fragment's fences
    /// balance ([`fences_balanced`]) and, if they don't, containing it as
    /// literal so a stray fence can't leak into the blocks that follow. This is
    /// the one block whose delimiters the writer does *not* own, so it is the
    /// single place a malformed fence can originate; it degrades to safe-literal
    /// rather than corrupting the page.
    Raw(String),
}

impl MarkdownBlock {
    /// Convenience: a two-column "key / value" table from labelled cells.
    pub fn kv(headers: [&str; 2], rows: impl IntoIterator<Item = (String, String)>) -> Self {
        MarkdownBlock::Table {
            headers: vec![headers[0].to_string(), headers[1].to_string()],
            rows: rows.into_iter().map(|(k, v)| vec![k, v]).collect(),
        }
    }

    /// Splice a Markdown fragment verbatim (see [`MarkdownBlock::Raw`]).
    pub fn raw(md: impl Into<String>) -> Self {
        MarkdownBlock::Raw(md.into())
    }

    fn render_into(&self, out: &mut String) {
        match self {
            MarkdownBlock::Table { headers, rows } => {
                writeln!(out, "| {} |", headers.join(" | ")).unwrap();
                writeln!(out, "|{}|", vec!["---"; headers.len()].join("|")).unwrap();
                for r in rows {
                    writeln!(out, "| {} |", r.join(" | ")).unwrap();
                }
                writeln!(out).unwrap();
            }
            MarkdownBlock::Fenced { lang, body } => write_fenced(out, lang, body),
            MarkdownBlock::Raw(md) => {
                let md = md.trim_end();
                if fences_balanced(md) {
                    // A well-nested fragment: render it as the markdown it is.
                    writeln!(out, "{md}\n").unwrap();
                } else {
                    // Not well-nested on its own: contain it as literal (a fence
                    // with no language) so its stray fence can't leak.
                    write_fenced(out, "", md);
                }
            }
        }
    }
}

/// The longest run of consecutive backticks anywhere in `s` (0 if none).
fn longest_backtick_run(s: &str) -> usize {
    s.as_bytes()
        .split(|&b| b != b'`')
        .map(<[u8]>::len)
        .max()
        .unwrap_or(0)
}

/// A backtick fence delimiter that strictly dominates every backtick run in
/// `body`, floored at the conventional three.
///
/// CommonMark closes a fenced code block at the first line of *at least* the
/// opening run length that carries no info string, so an enclosing fence must
/// out-tick anything it wraps or that inner run closes it early. `run + 1` is
/// the minimal width that can't collide; `.max(2) + 1` keeps the common case
/// (a body with no fences) at three backticks.
fn fence_delim(body: &str) -> String {
    "`".repeat(longest_backtick_run(body).max(2) + 1)
}

/// Write `body` as a fenced code block whose delimiter dominates any backtick
/// run inside it (see [`fence_delim`]). `lang` is the info string (`""` for a
/// bare fence). This is the one place the writer materializes a fence, so both
/// a `Fenced` block and a contained-as-literal `Raw` fragment go through it.
fn write_fenced(out: &mut String, lang: &str, body: &str) {
    let body = body.trim_end();
    let fence = fence_delim(body);
    writeln!(out, "{fence}{lang}\n{body}\n{fence}\n").unwrap();
}

/// Whether the backtick fences in `s` form a well-nested (Dyck) word: every
/// opened code fence is eventually closed, and none closes without an open.
///
/// This is the detector for [`MarkdownBlock::Raw`]: a balanced fragment is safe
/// to splice verbatim; an unbalanced one would leak past its own block, so the
/// writer contains it instead. It models CommonMark's rule that *inside* a code
/// fence everything is literal until a bare closing fence of sufficient width,
/// so an inner line carrying an info string (e.g. ```` ```mermaid ````) opens
/// nothing while we are already inside a block.
fn fences_balanced(s: &str) -> bool {
    let mut stack: Vec<usize> = Vec::new();
    for line in s.lines() {
        let trimmed = line.trim_start();
        let run = trimmed.bytes().take_while(|&b| b == b'`').count();
        if run < 3 {
            continue;
        }
        let info = trimmed[run..].trim();
        match stack.last() {
            // Inside a code block: only a bare fence of >= the opening width
            // closes it; an info string or a narrower run is literal content.
            Some(&open) if info.is_empty() && run >= open => {
                stack.pop();
            }
            Some(_) => {}
            // Outside any block: this opens one (an info string is allowed).
            None => stack.push(run),
        }
    }
    stack.is_empty()
}

/// Build a key/value [`MarkdownBlock`] (a two-column "field / value" table)
/// from `key => value` pairs. Each key and value is stringified via
/// `ToString`, so `&str`, `String`, integers, and `bool` drop in without a
/// hand-written `.to_string()`.
///
/// ```
/// # use testsvm::md_kv;
/// let _block = md_kv! {
///     "transaction succeeded" => true,
///     "fee (lamports)"        => 5_000u64,
/// };
/// ```
#[macro_export]
macro_rules! md_kv {
    ( $( $key:expr => $val:expr ),+ $(,)? ) => {
        $crate::report::MarkdownBlock::kv(
            ["field", "value"],
            [ $( (($key).to_string(), ($val).to_string()) ),+ ],
        )
    };
}

/// Build an N-column table [`MarkdownBlock`]: a header row, then one row per
/// line, cells separated by commas and rows by `;`. Every cell is stringified
/// via `ToString`, so mixed `&str` / numeric / `bool` rows need no
/// per-cell `.to_string()`.
///
/// ```
/// # use testsvm::md_table;
/// let _block = md_table! {
///     "item",  "before", "after";
///     "fee",   0u64,     4_000u64;
///     "owner", "maker",  "taker";
/// };
/// ```
#[macro_export]
macro_rules! md_table {
    ( $( $header:expr ),+ $(,)? ; $( $( $cell:expr ),+ $(,)? );+ $(;)? ) => {
        $crate::report::MarkdownBlock::Table {
            headers: ::std::vec![ $( ($header).to_string() ),+ ],
            rows: ::std::vec![ $( ::std::vec![ $( ($cell).to_string() ),+ ] ),+ ],
        }
    };
}

/// A value that knows how to render itself as a Markdown fragment.
///
/// Implementors resolve any `Pubkey`s to alias names *before* this call, so the
/// output is deterministic across runs (no base58 leaking into the report).
pub trait ToMarkdown {
    fn to_markdown(&self) -> MarkdownBlock;
}

/// A recorder threaded through a test; emits its Markdown report on `Drop`.
///
/// Construct one at the top of a `#[test] fn`, narrate as the test runs, and
/// let it fall out of scope: the report is written (and the test is failed iff
/// any soft [`check`](Report::check) missed) in the [`Drop`] impl.
pub struct Report {
    title: String,
    intent: String,
    events: Vec<Event>,
    emitted: bool,
    /// `Some(reason)` iff the test declared, via
    /// [`expect_panic`](Report::expect_panic), that it is a parked TDD spec for
    /// a known panic. Flips the emitted status from a surprise `ABORTED` to a
    /// deliberate `RED (expected)`.
    expected_panic: Option<String>,
}

impl Report {
    pub fn new(title: impl Into<String>, intent: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            intent: intent.into(),
            events: Vec::new(),
            emitted: false,
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
    pub fn snapshot(&mut self, label: impl Into<String>, value: &impl ToMarkdown) -> &mut Self {
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
    pub fn authority(&mut self, story: &impl ToMarkdown) -> &mut Self {
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

    fn failures(&self) -> usize {
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

    fn flush(&mut self) {
        if self.emitted {
            return;
        }
        self.emitted = true;
        // Captured at emit time: on a panicking unwind, Drop fires here with
        // `panicking()` true, which is what distinguishes a deliberate abort
        // from a clean end (a test that died half-way proves nothing, however
        // many checks it had passed by then).
        emit_report(&self.title, &self.render(std::thread::panicking()));
    }

    fn render(&self, aborted: bool) -> String {
        // A *declared* abort is the TDD red phase rather than a surprise; an
        // undeclared one is a genuine ABORTED. Only a clean end reports the
        // PASS/FAIL the checks earned.
        let status = match (&self.expected_panic, aborted) {
            (Some(_), true) => "RED (expected)",
            (None, true) => "ABORTED",
            _ => {
                if self.failures() == 0 {
                    "PASS"
                } else {
                    "FAIL"
                }
            }
        };

        // Build a list of self-contained blocks, then join with exactly one
        // blank line between each. Doing the spacing at the seam (rather than
        // trailing every block with `\n`) is what keeps a `note` or `snapshot`
        // that follows a `check` from gluing onto the checklist.
        let mut blocks: Vec<String> = vec![
            format!("## {} — {status}", self.title),
            format!("> {}", self.intent),
        ];

        // Abort context, right under the intent, so a reader skimming the
        // heading and first lines gets the whole story.
        if let (Some(reason), true) = (&self.expected_panic, aborted) {
            blocks.push(format!(
                "> **Expected abort:** {reason}\n>\n> The report stops at the last \
                 event recorded before the abort."
            ));
        }

        blocks.extend(Self::render_events(&self.events));

        let mut out = blocks.join("\n\n");
        out.push('\n');
        out
    }

    /// Render an event sequence into self-contained Markdown blocks, joined by
    /// the caller with one blank line between each. Factored out of [`render`]
    /// so an [`Event::Container`] can recurse over its own children with the
    /// identical rules.
    fn render_events(events: &[Event]) -> Vec<String> {
        // Consecutive checks must stay a *tight* Markdown list (one bullet per
        // line, no blank between), so a run of `Check`s collapses into one
        // block; everything else is its own block.
        let mut blocks: Vec<String> = Vec::new();
        let mut i = 0;
        while i < events.len() {
            match &events[i] {
                Event::Check { .. } => {
                    let mut lines = Vec::new();
                    while let Some(Event::Check {
                        label,
                        expected,
                        actual,
                        pass,
                    }) = events.get(i)
                    {
                        lines.push(if *pass {
                            format!("- [x] {label}: `{actual}`")
                        } else {
                            format!("- [ ] {label}: expected `{expected}`, got `{actual}`")
                        });
                        i += 1;
                    }
                    blocks.push(lines.join("\n"));
                }
                Event::Transition { .. } => {
                    // Consecutive transitions collapse into one neutral table.
                    let mut rows = Vec::new();
                    while let Some(Event::Transition {
                        label,
                        before,
                        expected,
                        actual,
                        meaning,
                        pass,
                    }) = events.get(i)
                    {
                        let after = if *pass {
                            format!("`{actual}`")
                        } else {
                            format!("`{actual}` (expected `{expected}`)")
                        };
                        rows.push(format!("| {label} | `{before}` | {after} | {meaning} |"));
                        i += 1;
                    }
                    let mut table = String::from(
                        "| Observation | Before | After | What it means |\n|---|---|---|---|\n",
                    );
                    table.push_str(&rows.join("\n"));
                    blocks.push(table);
                }
                Event::Step(h) => {
                    blocks.push(format!("### {h}"));
                    i += 1;
                }
                Event::Note(p) => {
                    blocks.push(p.clone());
                    i += 1;
                }
                Event::Snapshot { label, block } => {
                    let mut s = format!("**{label}**\n\n");
                    block.render_into(&mut s);
                    blocks.push(s.trim_end().to_string());
                    i += 1;
                }
                Event::Container { summary, children } => {
                    let inner = Self::render_events(children).join("\n\n");
                    blocks.push(render_details(summary, &inner));
                    i += 1;
                }
            }
        }
        blocks
    }
}

/// Wrap rendered child Markdown in a collapsible `<details>` disclosure.
///
/// `<details>` is a CommonMark "type 6" HTML block: after the blank line that
/// follows `<summary>`, normal Markdown parsing resumes, so a ```` ```mermaid ````
/// diagram inside renders *as a diagram* (not literal text) while the
/// `</details>` boundary still keeps it from leaking. The one collision is a
/// child that literally contains `</details>`; in that (pathological) case we
/// drop the disclosure and fall back to a bold label, so the close tag is never
/// emitted against content that already closed it.
fn render_details(summary: &str, inner: &str) -> String {
    if inner.contains("</details>") {
        return format!("**{summary}**\n\n{inner}");
    }
    format!("<details>\n<summary>{summary}</summary>\n\n{inner}\n</details>")
}

/// Builds the body of a [`Report::act`] group. Mirrors the narrative subset of
/// [`Report`]'s recording methods; the act's `<details>` brackets are emitted
/// by the writer, so a body assembled here is always well-nested.
pub struct ActBuilder {
    events: Vec<Event>,
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
    pub fn snapshot(&mut self, label: impl Into<String>, value: &impl ToMarkdown) -> &mut Self {
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
        if fails > 0 && !std::thread::panicking() {
            panic!("{}: {fails} check(s) failed; see report", self.title);
        }
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

#[cfg(test)]
mod macro_tests {
    use crate::report::MarkdownBlock;

    #[test]
    fn md_kv_builds_a_field_value_table_and_stringifies_values() {
        let block = md_kv! {
            "ok"  => true,
            "n"   => 42u64,
            "msg" => "hi",
        };
        match block {
            MarkdownBlock::Table { headers, rows } => {
                assert_eq!(headers, vec!["field", "value"]);
                assert_eq!(
                    rows,
                    vec![vec!["ok", "true"], vec!["n", "42"], vec!["msg", "hi"],]
                );
            }
            _ => panic!("md_kv should build a Table"),
        }
    }

    #[test]
    fn md_table_builds_rows_and_stringifies_mixed_cells() {
        let block = md_table! {
            "item",  "before", "after";
            "fee",   0u64,     4_000u64;
            "owner", "maker",  "taker";
        };
        match block {
            MarkdownBlock::Table { headers, rows } => {
                assert_eq!(headers, vec!["item", "before", "after"]);
                assert_eq!(
                    rows,
                    vec![vec!["fee", "0", "4000"], vec!["owner", "maker", "taker"],]
                );
            }
            _ => panic!("md_table should build a Table"),
        }
    }
}

#[cfg(test)]
mod status_tests {
    use super::Report;

    // The (expected_panic, aborted) status logic is unit-testable without
    // staging a real panic: render directly and read the heading. `emitted` is
    // set so the Drop impl doesn't also write the report to disk.
    fn heading(report: &mut Report, aborted: bool) -> String {
        let out = report.render(aborted);
        report.emitted = true;
        out.lines().next().unwrap().to_string()
    }

    #[test]
    fn declared_panic_that_aborts_is_red_expected() {
        let mut md = Report::new("t", "i");
        md.expect_panic("known bug; fix pending");
        let out = md.render(true);
        md.emitted = true;
        assert!(out.contains("— RED (expected)"), "{out}");
        assert!(
            out.contains("**Expected abort:** known bug; fix pending"),
            "{out}"
        );
    }

    #[test]
    fn undeclared_abort_is_aborted() {
        let mut md = Report::new("t", "i");
        assert!(heading(&mut md, true).contains("— ABORTED"));
    }

    #[test]
    fn clean_end_reports_pass_even_when_panic_was_declared() {
        // A declared `expect_panic` that does NOT abort falls through to
        // PASS/FAIL; the paired `#[should_panic]` catches the stale-spec flip,
        // not this code.
        let mut md = Report::new("t", "i");
        md.expect_panic("known bug");
        assert!(heading(&mut md, false).contains("— PASS"));
    }

    #[test]
    fn transitions_render_one_neutral_table_and_carry_teeth() {
        let mut md = Report::new("t", "i");
        // Two consecutive transitions: one as expected, one missed.
        md.transition("yes_votes", 0u8, 255, 255, "`-= 1` underflowed");
        md.transition("no_votes", 7u8, 7, 9, "must be untouched");
        let out = md.render(false);
        md.emitted = true;

        // One table, neutral headers, no checklist syntax.
        assert_eq!(out.matches("| Observation | Before | After |").count(), 1);
        assert!(!out.contains("- [x]"), "{out}");
        assert!(out.contains("| yes_votes | `0` | `255` | `-= 1` underflowed |"));
        // The miss shows both values inline and fails the report.
        assert!(
            out.contains("| no_votes | `7` | `9` (expected `7`) |"),
            "{out}"
        );
        assert!(out.contains("— FAIL"), "{out}");
        assert_eq!(md.failures(), 1);
        // The miss above is the fixture, not a real failure: disarm Drop's
        // escalation (which is itself under test here).
        md.events.clear();
    }

    #[test]
    fn passing_transitions_keep_the_report_green() {
        let mut md = Report::new("t", "i");
        md.transition("fee", 0u64, 4_000, 4_000, "swap fee accrued");
        assert!(heading(&mut md, false).contains("— PASS"));
    }
}

#[cfg(test)]
mod fence_tests {
    use super::{fence_delim, fences_balanced, longest_backtick_run, MarkdownBlock, Report};

    fn rendered(block: MarkdownBlock) -> String {
        let mut s = String::new();
        block.render_into(&mut s);
        s
    }

    #[test]
    fn longest_run_finds_the_widest_backtick_span() {
        assert_eq!(longest_backtick_run("no ticks here"), 0);
        assert_eq!(longest_backtick_run("inline `x` only"), 1);
        assert_eq!(longest_backtick_run("a ```fence``` and ```` four"), 4);
    }

    #[test]
    fn fence_delim_floors_at_three_and_dominates_the_body() {
        assert_eq!(fence_delim("plain log line"), "```");
        // A body that itself carries a ```console fence needs a 4-tick wrapper.
        assert_eq!(fence_delim("```console\nLOG\n```"), "````");
    }

    #[test]
    fn fenced_body_with_a_fence_widens_so_it_cannot_self_close() {
        // The exact shape that broke the dogfooder: pre-fenced content handed
        // to a Fenced block. The wrapper must out-tick the inner fence.
        let out = rendered(MarkdownBlock::Fenced {
            lang: "text".into(),
            body: "### Act 1\n```console\nLOG\n```".into(),
        });
        assert!(out.starts_with("````text\n"), "wrapper too narrow: {out}");
        assert!(out.contains("```console\nLOG\n```"), "body mangled: {out}");
        // The rendered block, read on its own, is a well-nested Dyck word.
        assert!(
            fences_balanced(&out),
            "Fenced output is not well-nested: {out}"
        );
    }

    #[test]
    fn raw_balanced_markdown_splices_verbatim() {
        // A real mermaid diagram should render as a diagram: a bare ```mermaid
        // fence, NOT widened, NOT wrapped.
        let out = rendered(MarkdownBlock::raw("```mermaid\nsequenceDiagram\n```"));
        assert_eq!(out.trim_end(), "```mermaid\nsequenceDiagram\n```");
    }

    #[test]
    fn raw_unbalanced_fragment_is_contained_not_leaked() {
        // An opened-but-never-closed fence is not a well-nested fragment; the
        // writer must seal it so it can't swallow whatever follows.
        let frag = "```mermaid\nsequenceDiagram\n(no close)";
        assert!(!fences_balanced(frag));
        let out = rendered(MarkdownBlock::raw(frag));
        assert!(fences_balanced(&out), "unbalanced Raw leaked: {out}");
        assert!(
            out.starts_with("````\n"),
            "should contain as literal: {out}"
        );
    }

    #[test]
    fn report_with_a_pre_fenced_block_and_a_sibling_diagram_stays_well_nested() {
        // The full issue-#7 regression: a Fenced log block whose body carries
        // its own ```console fences, followed by a sibling mermaid. Before the
        // width fix the log wrapper leaked and swallowed the diagram; the whole
        // document must now be a well-nested word.
        let mut md = Report::new("vesting lifecycle", "the dogfooder's shape");
        md.block(
            "Act 1 logs",
            MarkdownBlock::Fenced {
                lang: "text".into(),
                body: "### Act 1\n```console\nAlice ->> ComputeBudget: unnamed\n```".into(),
            },
        );
        md.block(
            "Act 1 diagram",
            MarkdownBlock::raw("```mermaid\nsequenceDiagram\n    Alice ->> Vault: deposit\n```"),
        );
        let out = md.render(false);
        md.emitted = true;
        assert!(fences_balanced(&out), "report is not well-nested: {out}");
        // The diagram survives as a real (bare-fenced) mermaid block.
        assert!(
            out.contains("```mermaid\nsequenceDiagram"),
            "diagram lost: {out}"
        );
    }

    #[test]
    fn act_wraps_children_in_a_details_disclosure_and_stays_well_nested() {
        let mut md = Report::new("vesting lifecycle", "acts as disclosures");
        md.act("Act 1: open the session", |a| {
            a.note("Alice opens, Bob funds.");
            a.block(
                "logs",
                MarkdownBlock::Fenced {
                    lang: "text".into(),
                    body: "```console\nLOG\n```".into(),
                },
            );
            a.block(
                "diagram",
                MarkdownBlock::raw("```mermaid\nsequenceDiagram\n```"),
            );
        });
        let out = md.render(false);
        md.emitted = true;
        assert!(out.contains("<details>\n<summary>Act 1: open the session</summary>"));
        assert!(out.contains("</details>"));
        // A diagram inside the disclosure stays a bare (rendering) mermaid fence,
        // and the log block inside widened to a 4-tick wrapper.
        assert!(out.contains("```mermaid\nsequenceDiagram\n```"), "{out}");
        assert!(out.contains("````text\n```console"), "{out}");
        assert!(fences_balanced(&out), "act document not well-nested: {out}");
    }
}
