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
//! This complements [`print_markdown_pair`] rather than replacing it. That
//! method renders one *transaction* (its instruction + logs); a `Report`
//! renders one *scenario*: the intent, the before/after state, and the
//! pass/fail checks, as a single committable document per test.
//!
//! This module is domain-agnostic: it knows nothing about your program's
//! accounts. Domain types earn a place in a report by implementing
//! [`ToMarkdown`]; assemble the per-scenario files (one per test, named by a
//! slug of the title) into a single document with a `just`-style concat step.
//!
//! [`print_markdown_pair`]: crate::transaction::TransactionResult::print_markdown_pair

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
    /// A fenced code block (e.g. captured program logs).
    Fenced { lang: String, body: String },
}

impl MarkdownBlock {
    /// Convenience: a two-column "key / value" table from labelled cells.
    pub fn kv(headers: [&str; 2], rows: impl IntoIterator<Item = (String, String)>) -> Self {
        MarkdownBlock::Table {
            headers: vec![headers[0].to_string(), headers[1].to_string()],
            rows: rows.into_iter().map(|(k, v)| vec![k, v]).collect(),
        }
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
            MarkdownBlock::Fenced { lang, body } => {
                writeln!(out, "```{lang}\n{}\n```\n", body.trim_end()).unwrap();
            }
        }
    }
}

/// Build a key/value [`MarkdownBlock`] (a two-column "field / value" table)
/// from `key => value` pairs. Each key and value is stringified via
/// `ToString`, so `&str`, `String`, integers, and `bool` drop in without a
/// hand-written `.to_string()`.
///
/// ```
/// # use litesvm_utils::md_kv;
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
/// # use litesvm_utils::md_table;
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
///
/// The status in the rendered heading is one of:
///   - `PASS` / `FAIL`: the test ran to completion; the soft checks decide.
///   - `ABORTED`: the test panicked mid-flight (a failed `send_ok`, a
///     [`require`](Report::require), a stray `unwrap`). Whatever checks had
///     passed by then prove nothing, so this is never reported as `PASS`.
///   - `RED (expected)`: the abort was declared up front with
///     [`expect_panic`](Report::expect_panic) — a parked TDD spec.
pub struct Report {
    title: String,
    intent: String,
    events: Vec<Event>,
    emitted: bool,
    /// `Some(reason)` iff the test declared, via [`Report::expect_panic`],
    /// that it is a red spec expected to abort.
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

    /// Declare that this scenario is EXPECTED to abort mid-flight: it is a
    /// TDD red spec whose behaviour is not implemented yet.
    ///
    /// Pairs with Rust's `#[should_panic]` attribute on the test fn; the two
    /// cover different layers:
    ///   - `#[should_panic(expected = "...")]` keeps `cargo test` green while
    ///     the spec is parked;
    ///   - `expect_panic(reason)` keeps the *report* honest: the heading reads
    ///     `RED (expected)` with the reason, instead of `PASS` or `ABORTED`.
    ///
    /// The green flip: when the pending behaviour lands, the test stops
    /// panicking, `#[should_panic]` fails it ("test did not panic as
    /// expected"), and you remove both markers. The [`Drop`] impl deliberately
    /// does NOT panic on a stale `expect_panic`: if it did, `#[should_panic]`
    /// would be satisfied by that panic and the flip would never surface.
    ///
    /// ```ignore
    /// #[test]
    /// #[should_panic(expected = "Transaction failed")]
    /// fn restake_starts_a_new_cycle() {
    ///     let mut md = Report::new("restake", "an unstaked asset can re-enter");
    ///     md.expect_panic("stake re-adds the FreezeDelegate; fix pending");
    ///     // ... the spec, which currently dies at a send_ok() ...
    /// }
    /// ```
    pub fn expect_panic(&mut self, reason: impl Into<String>) -> &mut Self {
        self.expected_panic = Some(reason.into());
        self
    }

    fn failures(&self) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, Event::Check { pass: false, .. }))
            .count()
    }

    fn flush(&mut self) {
        if self.emitted {
            return;
        }
        self.emitted = true;
        emit_report(&self.title, &self.render(std::thread::panicking()));
    }

    /// `aborted` is whether the owning test is unwinding at flush time; it is
    /// passed in (rather than read off the thread inside) so the status matrix
    /// below is unit-testable without staging real panics.
    fn render(&self, aborted: bool) -> String {
        // Status precedence: an abort outranks the soft checks (a test that
        // died half-way through proves nothing, however many checks it had
        // passed by then), and a *declared* abort is the TDD red phase rather
        // than a surprise.
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
        // heading plus first lines gets the whole story.
        match (&self.expected_panic, aborted) {
            (Some(reason), true) => blocks.push(format!(
                "> **Expected abort:** {reason}\n>\n> The report stops at the last event \
                 recorded before the abort."
            )),
            (None, true) => blocks.push(
                "> **Aborted:** the test panicked before reaching its end; the report stops \
                 at the last recorded event."
                    .to_string(),
            ),
            (Some(reason), false) => blocks.push(format!(
                "> **Stale `expect_panic`:** declared \"{reason}\" but the test ran to \
                 completion. If the pending behaviour has landed, remove `expect_panic` and \
                 `#[should_panic]`."
            )),
            (None, false) => {}
        }

        // Consecutive checks must stay a *tight* Markdown list (one bullet per
        // line, no blank between), so a run of `Check`s collapses into one
        // block; everything else is its own block.
        let mut i = 0;
        while i < self.events.len() {
            match &self.events[i] {
                Event::Check { .. } => {
                    let mut lines = Vec::new();
                    while let Some(Event::Check {
                        label,
                        expected,
                        actual,
                        pass,
                    }) = self.events.get(i)
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
            }
        }

        let mut out = blocks.join("\n\n");
        out.push('\n');
        out
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
mod report_tests {
    use super::Report;

    // --- the status matrix, driven through render(aborted) directly ---------

    #[test]
    fn completed_run_passes_or_fails_on_checks() {
        let mut md = Report::new("t", "i");
        md.check("ok", 1, 1);
        assert!(md.render(false).starts_with("## t — PASS"));
        md.check("bad", 1, 2);
        assert!(md.render(false).starts_with("## t — FAIL"));
        // This report intentionally holds a failed check; skip Drop so its
        // escalation doesn't fail this (meta-)test.
        std::mem::forget(md);
    }

    #[test]
    fn unexpected_abort_renders_aborted_never_pass() {
        let mut md = Report::new("t", "i");
        // Every check green so far...
        md.check("ok so far", 1, 1);
        // ...but the test died before finishing.
        let out = md.render(true);
        assert!(out.starts_with("## t — ABORTED"), "got: {out}");
        assert!(out.contains("panicked before reaching its end"));
    }

    #[test]
    fn declared_abort_renders_red_expected_with_the_reason() {
        let mut md = Report::new("t", "i");
        md.expect_panic("stake re-adds the FreezeDelegate; fix pending");
        let out = md.render(true);
        assert!(out.starts_with("## t — RED (expected)"), "got: {out}");
        assert!(out.contains("stake re-adds the FreezeDelegate; fix pending"));
    }

    #[test]
    fn stale_expect_panic_warns_but_status_stays_with_the_checks() {
        let mut md = Report::new("t", "i");
        md.expect_panic("was pending, has now landed");
        md.check("ok", 1, 1);
        let out = md.render(false);
        assert!(out.starts_with("## t — PASS"), "got: {out}");
        assert!(out.contains("Stale `expect_panic`"));
    }

    // --- the Drop-path invariants, exercised with real unwinding ------------

    /// The declared-abort path: Drop flushes during unwind without
    /// double-panicking (a double panic would abort the whole test process),
    /// and the paired #[should_panic] is what keeps cargo green.
    #[test]
    #[should_panic(expected = "boom")]
    fn declared_abort_flushes_during_unwind_without_double_panic() {
        let mut md = Report::new("report-drop-during-unwind", "unwind path");
        md.expect_panic("testing the unwind path itself");
        md.step("about to die");
        panic!("boom");
    }

    /// A stale expect_panic must NOT panic in Drop. If it did, a paired
    /// #[should_panic] would be satisfied by that Drop panic, and the green
    /// flip (the pending behaviour finally landing) could never surface as a
    /// test failure.
    #[test]
    fn stale_expect_panic_does_not_fail_the_test() {
        let mut md = Report::new("report-stale-marker", "stale marker path");
        md.expect_panic("declared but nothing panics");
        md.check("fine", 1, 1);
        // Drop runs here: it must stay quiet.
    }
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
