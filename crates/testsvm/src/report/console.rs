//! The console renderer: a styled, scannable stdout view of a Report.
//! Markdown-only blocks (Raw) are skipped; the file is the complete artifact.

use {
    super::{
        block::MarkdownBlock,
        core::{Event, Report},
        render::{ReportRenderer, Status},
    },
    crate::style::Style,
    comfy_table::{presets::UTF8_BORDERS_ONLY, ContentArrangement, Table},
};

pub(super) const CONSOLE_WIDTH: u16 = 100;

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

    fn render_block(&self, block: &MarkdownBlock, indent: usize, out: &mut String) {
        let pad = "  ".repeat(indent);
        match block {
            MarkdownBlock::Table { headers, rows } => {
                let mut t = Table::new();
                t.load_preset(UTF8_BORDERS_ONLY)
                    .set_content_arrangement(ContentArrangement::Dynamic)
                    .set_width(CONSOLE_WIDTH);
                t.set_header(headers.clone());
                for row in rows {
                    t.add_row(row.clone());
                }
                for line in t.to_string().lines() {
                    out.push_str(&pad);
                    out.push_str(line);
                    out.push('\n');
                }
            }
            MarkdownBlock::Fenced { body, .. } => {
                for line in body.lines() {
                    out.push_str(&pad);
                    out.push_str("    ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            // Raw is verbatim markdown (mermaid, spliced fragments): skipped in
            // the console; it lives only in the committed .md file.
            MarkdownBlock::Raw(_) => {}
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
    use crate::report::MarkdownBlock;

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
    fn table_snapshot_frames_and_wraps_a_long_cell() {
        let mut r = Report::new("T", "i");
        r.block(
            "state",
            MarkdownBlock::Table {
                headers: vec!["k".into(), "meaning".into()],
                rows: vec![vec![
                    "x".into(),
                    "a very long explanation that must wrap across more than one line in the cell so we exceed the width".into(),
                ]],
            },
        );
        let out = plain().render(&r, Status::Pass);
        assert!(out.contains('┌') && out.contains('│'), "framed table missing:\n{out}");
        // wrapped → the long cell produced more than one body line
        assert!(out.matches('│').count() > 4, "expected wrapping:\n{out}");
        r.disarm();
    }

    #[test]
    fn raw_block_is_skipped_in_console() {
        let mut r = Report::new("T", "i");
        r.block("diagram", MarkdownBlock::raw("```mermaid\nflowchart LR\nA-->B\n```"));
        let out = plain().render(&r, Status::Pass);
        assert!(!out.contains("mermaid"), "Raw leaked into console:\n{out}");
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
