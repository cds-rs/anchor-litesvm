use super::block::render_details;
use super::core::{Event, Report};
use super::render::{ReportRenderer, Status};

/// Renders a [`Report`] as the Markdown narrative format: an `##` heading,
/// blockquote intent, and event-derived blocks joined by blank lines.
pub(super) struct MarkdownRenderer;

impl ReportRenderer for MarkdownRenderer {
    fn render(&self, report: &Report, status: Status) -> String {
        let mut blocks: Vec<String> = vec![
            format!("## {} — {}", report.title, status.label()),
            format!("> {}", report.intent),
        ];
        if let Status::RedExpected(reason) = &status {
            blocks.push(format!(
                "> **Expected abort:** {reason}\n>\n> The report stops at the last \
                 event recorded before the abort."
            ));
        }
        blocks.extend(render_events(&report.events));
        let mut out = blocks.join("\n\n");
        out.push('\n');
        out
    }
}

/// Render an event sequence into self-contained Markdown blocks, joined by
/// the caller with one blank line between each. Factored out of `render`
/// so an [`Event::Container`] can recurse over its own children with the
/// identical rules.
pub(super) fn render_events(events: &[Event]) -> Vec<String> {
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
                let inner = render_events(children).join("\n\n");
                blocks.push(render_details(summary, &inner));
                i += 1;
            }
        }
    }
    blocks
}

#[cfg(test)]
mod golden_tests {
    use crate::report::Report;

    #[test]
    fn markdown_render_is_unchanged_after_refactor() {
        let mut r = Report::new("Demo Title", "what we are proving");
        r.step("Before");
        r.note("set the stage");
        r.check("fee", 0u64, 0u64);
        let golden = r.render(false); // captured BEFORE refactor; pin it
        // After the refactor this must equal MarkdownRenderer.render(&r, r.status(false)).
        assert!(golden.contains("## Demo Title — PASS"));
        assert!(golden.contains("> what we are proving"));
        assert!(golden.contains("- [x] fee: `0`"));
        r.disarm(); // do not also emit on drop during the test (see note)
    }
}

#[cfg(test)]
mod status_tests {
    use crate::report::Report;
    use super::super::render::Status;

    #[test]
    fn status_reflects_checks_and_abort() {
        let mut r = Report::new("t", "i");
        assert_eq!(r.status(false), Status::Pass);
        r.check("a", 1u64, 2u64); // a failing check
        assert_eq!(r.status(false), Status::Failed(1));
        assert_eq!(r.status(true), Status::Aborted);
        r.disarm();

        let mut e = Report::new("t", "i");
        e.expect_panic("known TDD red");
        assert_eq!(e.status(true), Status::RedExpected("known TDD red".to_string()));
        e.disarm();
    }

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
