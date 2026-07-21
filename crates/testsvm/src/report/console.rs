//! The console renderer: a styled, scannable stdout view of a Report.
//! Markdown-only blocks (Raw) are skipped; the file is the complete artifact.

use {
    super::{
        core::{Event, Report},
        render::{ReportRenderer, Status},
    },
    crate::style::Style,
    frood_guide::Block,
};

pub(super) struct ConsoleRenderer {
    pub(super) style: Style,
}

impl ReportRenderer for ConsoleRenderer {
    fn render(&self, report: &Report, status: Status) -> String {
        let mut out = String::new();
        out.push_str(&self.style.bold(&report.title));
        out.push('\n');
        out.push_str(&self.style.dim(&report.intent));
        out.push('\n');
        self.render_events(&report.events, 0, &mut out);
        out.push('\n');
        out.push_str(&self.status_line(&status));
        out.push('\n');
        out
    }
}

impl ConsoleRenderer {
    fn render_events(&self, events: &[Event], indent: usize, out: &mut String) {
        let pad = "  ".repeat(indent);
        for event in events {
            match event {
                Event::Step(s) => {
                    out.push('\n');
                    out.push_str(&pad);
                    out.push_str(&self.style.bold(&format!("▸ {s}")));
                    out.push('\n');
                }
                Event::Note(s) => {
                    out.push_str(&pad);
                    out.push_str(s);
                    out.push('\n');
                }
                Event::Snapshot { label, block } => {
                    out.push_str(&pad);
                    out.push_str(&self.style.dim(label));
                    out.push('\n');
                    self.render_block(block, indent, out);
                }
                Event::Check { label, expected, actual, pass } => {
                    out.push_str(&pad);
                    if *pass {
                        // Show the asserted value on the happy path too: in a
                        // report the number IS the substance, and the markdown
                        // renderer keeps it (`- [x] label: \`value\``).
                        out.push_str(&self.style.green("✓"));
                        out.push(' ');
                        out.push_str(label);
                        out.push_str(": ");
                        out.push_str(actual);
                        out.push('\n');
                    } else {
                        out.push_str(&self.style.red("✗"));
                        out.push(' ');
                        out.push_str(label);
                        out.push('\n');
                        out.push_str(&pad);
                        out.push_str(&format!("    expected {expected} / actual {actual}\n"));
                    }
                }
                Event::Transition { label, before, actual, meaning, .. } => {
                    out.push_str(&pad);
                    out.push_str(&format!("{label}: {before} → {actual}  ({meaning})\n"));
                }
                Event::Container { summary, children } => {
                    out.push_str(&pad);
                    out.push_str(&self.style.bold(&format!("▼ {summary}")));
                    out.push('\n');
                    self.render_events(children, indent + 1, out);
                }
            }
        }
    }

    fn render_block(&self, block: &Block, indent: usize, out: &mut String) {
        let pad = "  ".repeat(indent);
        match block {
            // A table renders through frood-guide's own Unicode-box terminal
            // skin, indented into the report's tree.
            Block::Table(t) => {
                for line in t.terminal().lines() {
                    out.push_str(&pad);
                    out.push_str(line);
                    out.push('\n');
                }
            }
            // Fenced content (logs, a captured dump) prints as indented literal.
            Block::Fenced { text, .. } => {
                for line in text.lines() {
                    out.push_str(&pad);
                    out.push_str("    ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            Block::Prose(text) | Block::Heading(_, text) => {
                out.push_str(&pad);
                out.push_str(text);
                out.push('\n');
            }
            Block::BulletList(items) => {
                for item in items {
                    out.push_str(&pad);
                    out.push_str("  • ");
                    out.push_str(item);
                    out.push('\n');
                }
            }
            Block::Callout { body, .. } => {
                out.push_str(&pad);
                out.push_str(body);
                out.push('\n');
            }
            // Verbatim markdown (mermaid, spliced fragments) and the diagram
            // models are markdown-only: skipped in the console; they live only in
            // the committed .md file.
            Block::Verbatim(_) | Block::Sequence(_) | Block::Graph(_) => {}
        }
    }

    fn status_line(&self, status: &Status) -> String {
        match status {
            Status::Pass => self.style.green("PASS"),
            Status::Failed(n) => self.style.red(&format!("FAILED ({n})")),
            Status::RedExpected(reason) => self.style.dim(&format!("RED (expected): {reason}")),
            Status::Aborted => self.style.red("ABORTED"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frood_guide::{Cell, TableModel};

    fn plain() -> ConsoleRenderer {
        ConsoleRenderer { style: Style::Off }
    }

    #[test]
    fn passing_check_shows_a_green_tick_glyph() {
        let mut r = Report::new("T", "i");
        r.check("fee", 0u64, 0u64);
        let out = plain().render(&r, Status::Pass);
        // The asserted value rides on the happy path, not just the label.
        assert!(out.contains("✓ fee: 0"), "got:\n{out}");
        assert!(out.contains("PASS"), "footer missing:\n{out}");
        r.disarm();
    }

    #[test]
    fn failing_check_shows_cross_and_expected_actual() {
        let mut r = Report::new("T", "i");
        r.check("fee", 0u64, 1u64);
        let out = plain().render(&r, Status::Failed(1));
        assert!(out.contains("✗ fee"), "got:\n{out}");
        assert!(out.contains("expected") && out.contains("actual"), "got:\n{out}");
        r.disarm();
    }

    #[test]
    fn table_snapshot_frames_a_table() {
        let mut r = Report::new("T", "i");
        let table = TableModel::new(
            vec!["k".into(), "meaning".into()],
            vec![vec![Cell::from("x"), Cell::from("the delegate is set")]],
        )
        .expect("rectangular");
        r.block("state", Block::Table(table));
        let out = plain().render(&r, Status::Pass);
        assert!(out.contains('┌') && out.contains('│'), "framed table missing:\n{out}");
        r.disarm();
    }

    #[test]
    fn verbatim_block_is_skipped_in_console() {
        let mut r = Report::new("T", "i");
        r.block("diagram", Block::Verbatim("```mermaid\nflowchart LR\nA-->B\n```".into()));
        let out = plain().render(&r, Status::Pass);
        assert!(!out.contains("mermaid"), "Verbatim leaked into console:\n{out}");
        r.disarm();
    }

    #[test]
    fn color_on_wraps_a_failed_check_in_sgr() {
        let mut r = Report::new("T", "i");
        r.check("fee", 0u64, 1u64);
        let out = ConsoleRenderer { style: Style::On }.render(&r, Status::Failed(1));
        assert!(out.contains("\x1b[31m"), "expected red SGR:\n{out}");
        r.disarm();
    }
}
