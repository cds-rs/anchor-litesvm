use frood_guide::{render_arranged, Assembly, Block, BlockId, Cell, DiagramFormat, TableModel};

use super::core::{Event, Report};
use super::render::{ReportRenderer, Status};

/// Renders a [`Report`] as the Markdown narrative format by building a
/// frood-guide [`Assembly`] and emitting it through frood-guide's Markdown skin.
///
/// The header (an `##` heading carrying the verdict, the intent as a blockquote,
/// and a declared red's expected-abort note) has no block of its own in the
/// vocabulary, so it rides [`Block::Verbatim`], which splices the blockquote
/// bytes as-is. The event stream lowers into the flat blocks below; an act
/// becomes a fold (a `<details>`), so the whole page is one assembly rendered by
/// one renderer, the same one frood emits through.
pub(super) struct MarkdownRenderer;

impl ReportRenderer for MarkdownRenderer {
    fn render(&self, report: &Report, status: Status) -> String {
        let mut asm = Assembly::new();
        asm.push(Block::Heading(2, format!("{} — {}", report.title, status.label())));
        // The intent (and a declared red's note) are blockquotes; no block
        // renders a bare `>` line, so they splice verbatim.
        asm.push(Block::Verbatim(format!("> {}", report.intent)));
        if let Status::RedExpected(reason) = &status {
            asm.push(Block::Verbatim(format!(
                "> **Expected abort:** {reason}\n>\n> The report stops at the last \
                 event recorded before the abort."
            )));
        }
        push_events(&mut asm, &report.events);

        let mut out = String::new();
        render_arranged(asm.blocks(), asm.arrange(), DiagramFormat::Mermaid, &mut out);
        // frood-guide's skin ends every block with a blank line; the document
        // closes on a single newline.
        out.truncate(out.trim_end().len());
        out.push('\n');
        out
    }
}

/// Lower an event sequence into an [`Assembly`], preserving the report's
/// grouping: a run of consecutive `Check`s collapses into one bullet list, a run
/// of `Transition`s into one neutral table, and an act (a [`Event::Container`])
/// folds its lowered children behind a `<details>` summary. Called recursively
/// for an act's body so the identical rules apply at every level.
fn push_events(asm: &mut Assembly, events: &[Event]) {
    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Check { .. } => {
                let mut items = Vec::new();
                while let Some(Event::Check { label, expected, actual, pass }) = events.get(i) {
                    items.push(if *pass {
                        format!("[x] {label}: `{actual}`")
                    } else {
                        format!("[ ] {label}: expected `{expected}`, got `{actual}`")
                    });
                    i += 1;
                }
                asm.push(Block::BulletList(items));
            }
            Event::Transition { .. } => {
                let mut rows = Vec::new();
                while let Some(Event::Transition { label, before, expected, actual, meaning, pass }) =
                    events.get(i)
                {
                    let after = if *pass {
                        format!("`{actual}`")
                    } else {
                        format!("`{actual}` (expected `{expected}`)")
                    };
                    rows.push(vec![
                        Cell::Text(label.clone()),
                        Cell::Text(format!("`{before}`")),
                        Cell::Text(after),
                        Cell::Text(meaning.clone()),
                    ]);
                    i += 1;
                }
                let headers = vec![
                    "Observation".to_string(),
                    "Before".to_string(),
                    "After".to_string(),
                    "What it means".to_string(),
                ];
                let table = TableModel::new(headers, rows)
                    .expect("a transition row is always four cells wide");
                asm.push(Block::Table(table));
            }
            Event::Step(h) => {
                asm.push(Block::Heading(3, h.clone()));
                i += 1;
            }
            Event::Note(p) => {
                asm.push(Block::Prose(p.clone()));
                i += 1;
            }
            Event::Snapshot { label, block } => {
                asm.push(Block::Prose(format!("**{label}**")));
                asm.push(block.clone());
                i += 1;
            }
            Event::Container { summary, children } => {
                let start = asm.blocks().len();
                push_events(asm, children);
                let end = asm.blocks().len();
                // A fold needs a non-empty contiguous run; an act always adds at
                // least one block, but guard the degenerate empty act rather than
                // hand frood an empty fold it would refuse.
                if end > start {
                    let ids: Vec<BlockId> = (start..end).map(BlockId).collect();
                    asm.fold(summary.clone(), ids, false);
                }
                i += 1;
            }
        }
    }
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
        let golden = r.render(false);
        assert!(golden.contains("## Demo Title — PASS"));
        assert!(golden.contains("> what we are proving"));
        assert!(golden.contains("- [x] fee: `0`"));
        r.disarm();
    }

    /// The header framing is reproduced byte-for-byte through frood-guide: the
    /// `##` verdict heading, the intent blockquote, and the checklist, joined by
    /// blank lines and closed on a single newline (convention class 4: header
    /// shape, unchanged by the bridge).
    #[test]
    fn the_header_and_body_frame_byte_for_byte() {
        let mut r = Report::new("Demo Title", "what we are proving");
        r.step("Before");
        r.note("set the stage");
        r.check("fee", 0u64, 0u64);
        let out = r.render(false);
        r.disarm();
        assert_eq!(
            out,
            "## Demo Title — PASS\n\n> what we are proving\n\n### Before\n\nset the \
             stage\n\n- [x] fee: `0`\n"
        );
    }
}

#[cfg(test)]
mod convention_tests {
    use crate::report::Report;
    use frood_guide::{Block, Cell, TableModel};

    /// Convention class 1 (table separator spacing): tables now render the
    /// GitHub-flavored spaced rule `| --- |`, frood-guide's shape, in place of
    /// the report's former `|---|`. Pinned here on both table producers: a
    /// `transition` and a snapshot table.
    #[test]
    fn tables_render_the_spaced_separator() {
        let mut r = Report::new("t", "i");
        r.transition("yes_votes", 0u8, 255, 255, "`-= 1` underflowed");
        let table = TableModel::new(
            vec!["k".into(), "v".into()],
            vec![vec![Cell::from("fee"), Cell::from("4000")]],
        )
        .expect("rectangular");
        r.block("state", Block::Table(table));
        let out = r.render(false);
        r.disarm();
        // The transition table.
        assert!(
            out.contains(
                "| Observation | Before | After | What it means |\n| --- | --- | --- | --- |\n"
            ),
            "{out}"
        );
        // The snapshot table.
        assert!(out.contains("| k | v |\n| --- | --- |\n| fee | 4000 |"), "{out}");
        // And the retired form is gone.
        assert!(!out.contains("|---|"), "old separator survived:\n{out}");
    }

    /// Convention class 2 (details-close newline): an act's `<details>` closes on
    /// a blank line before `</details>`, frood-guide's shape, where the report
    /// formerly closed on a single newline.
    #[test]
    fn an_act_closes_details_on_a_blank_line() {
        let mut r = Report::new("t", "i");
        r.act("Act 1", |a| {
            a.note("Alice opens the vault.");
        });
        let out = r.render(false);
        r.disarm();
        assert!(
            out.contains(
                "<details>\n<summary>Act 1</summary>\n\nAlice opens the vault.\n\n</details>"
            ),
            "{out}"
        );
    }

    /// Convention class 3 (fence width, adopted into frood-guide): a fenced body
    /// carrying its own ```` ```console ```` fence widens the wrapper to four
    /// backticks, unchanged from the report's former rule, and the sibling
    /// mermaid diagram (class 5) splices verbatim as a bare fence. The whole page
    /// stays a well-nested word.
    #[test]
    fn fenced_widening_and_verbatim_mermaid_survive() {
        let mut r = Report::new("vesting lifecycle", "the dogfooder's shape");
        r.block(
            "Act 1 logs",
            Block::Fenced {
                lang: Some("text".into()),
                text: "### Act 1\n```console\nLOG\n```".into(),
            },
        );
        r.block(
            "Act 1 diagram",
            Block::Verbatim("```mermaid\nsequenceDiagram\n    Alice ->> Vault: deposit\n```".into()),
        );
        let out = r.render(false);
        r.disarm();
        // The log wrapper out-ticks its inner fence.
        assert!(out.contains("````text\n### Act 1\n```console\nLOG\n```\n````"), "{out}");
        // The diagram survives as a real (bare-fenced) mermaid block.
        assert!(
            out.contains("```mermaid\nsequenceDiagram\n    Alice ->> Vault: deposit\n```"),
            "{out}"
        );
    }
}

#[cfg(test)]
mod status_tests {
    use super::super::render::Status;
    use crate::report::Report;

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
    // staging a real panic: render directly and read the heading. `disarm()`
    // suppresses the Drop impl's file write and panic escalation.
    fn heading(report: &mut Report, aborted: bool) -> String {
        let out = report.render(aborted);
        report.disarm();
        out.lines().next().unwrap().to_string()
    }

    #[test]
    fn declared_panic_that_aborts_is_red_expected() {
        let mut md = Report::new("t", "i");
        md.expect_panic("known bug; fix pending");
        let out = md.render(true);
        md.disarm();
        assert!(out.contains("— RED (expected)"), "{out}");
        assert!(out.contains("**Expected abort:** known bug; fix pending"), "{out}");
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
        md.disarm();

        // One table, neutral headers, no checklist syntax.
        assert_eq!(out.matches("| Observation | Before | After |").count(), 1);
        assert!(!out.contains("- [x]"), "{out}");
        assert!(out.contains("| yes_votes | `0` | `255` | `-= 1` underflowed |"));
        // The miss shows both values inline and fails the report.
        assert!(out.contains("| no_votes | `7` | `9` (expected `7`) |"), "{out}");
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
