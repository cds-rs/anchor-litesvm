//! The dogfood "crime-scene" renderer: one Markdown page per scenario, plus a
//! master index. Engine-neutral, because it renders a [`model::Transaction`] and
//! nothing engine-specific, so the same renderer drops onto every adapter
//! (litesvm, mollusk, quasar) and the page reads the same whichever engine
//! witnessed the execution.
//!
//! A page carries the intent, the outcome, a link back to the test that produced
//! it, then four trace-sourced renders: the structured execution log, a plain
//! sequence diagram (no activation lifelines), and the authority + ownership
//! graphs. The boolean `matches!(result, Success)` the program's own suite
//! asserts is the witness who saw nothing; these renders are what it could not
//! say.

use crate::model::Transaction;

/// Wrap `body` in a fenced code block, trimming trailing whitespace so the
/// closing fence sits flush. An empty `lang` yields a plain ``` fence.
fn fenced(lang: &str, body: &str) -> String {
    format!("```{lang}\n{}\n```\n", body.trim_end())
}

/// The 1-based line of `fn <test_fn>(` in the test file, for a `#L<n>` anchor on
/// the source link. Read at render time, so the line matches the file in the
/// commit the report is generated from; regenerate after editing tests.
///
/// `manifest_dir` is the consuming test crate's `CARGO_MANIFEST_DIR`: this
/// renderer lives in the framework, so `env!("CARGO_MANIFEST_DIR")` here would
/// resolve to testsvm's own directory, not the dogfood's. The caller passes its
/// own (`env!("CARGO_MANIFEST_DIR")` from the test crate).
fn test_fn_line(manifest_dir: &str, test_file: &str, test_fn: &str) -> Option<usize> {
    let path = format!("{manifest_dir}/{test_file}");
    let needle = format!("fn {test_fn}(");
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .position(|line| line.contains(&needle))
        .map(|i| i + 1)
}

/// One scenario's Markdown page: intent, outcome, a link back to the test that
/// produced it, then the four trace-sourced renders. The sequence diagram is
/// plain (no activation lifelines).
///
/// `manifest_dir` is the test crate's `CARGO_MANIFEST_DIR` (used to resolve the
/// source-link line, see [`test_fn_line`]); `test_file` is crate-relative (e.g.
/// `tests/agent_identity.rs`); `test_fn` is the test's function name.
pub fn render_scenario(
    manifest_dir: &str,
    title: &str,
    intent: &str,
    test_file: &str,
    test_fn: &str,
    tx: &Transaction,
) -> String {
    let outcome = match &tx.error {
        None => "succeeded".to_string(),
        Some(e) => format!("failed: `{e}`"),
    };
    let anchor = test_fn_line(manifest_dir, test_file, test_fn)
        .map(|n| format!("#L{n}"))
        .unwrap_or_default();
    let mut md = String::new();
    md.push_str(&format!("# {title}\n\n"));
    md.push_str(&format!("**Intent.** {intent}\n\n"));
    md.push_str(&format!("**Outcome.** The transaction {outcome}.\n\n"));
    md.push_str(&format!(
        "**Source.** [`{test_file}::{test_fn}`](../{test_file}{anchor})\n\n"
    ));
    md.push_str("## Structured execution log\n\n");
    md.push_str(&fenced("", &tx.pretty_cpi_tree()));
    md.push_str("\n## Sequence diagram\n\n");
    md.push_str(tx.mermaid_string().trim_end());
    md.push_str("\n\n## Authority graph\n\n");
    md.push_str("Who signed for what; an `invoke_signed` PDA appears as its own authority.\n\n");
    md.push_str(tx.authority_graph_string().trim_end());
    md.push_str("\n\n## Ownership graph\n\n");
    md.push_str("Which program owns each account the transaction wrote.\n\n");
    md.push_str(tx.ownership_graph_string().trim_end());
    md.push('\n');
    md
}

/// The index page: a `heading` and `intro` paragraph the dogfood supplies (the
/// program and engine differ; the table does not), then one row per scenario.
/// `entries` are `(page_filename, title, outcome)`.
pub fn render_index(heading: &str, intro: &str, entries: &[(String, String, String)]) -> String {
    let mut md = String::new();
    md.push_str(&format!("# {heading}\n\n"));
    md.push_str(intro);
    md.push_str("\n\n| Scenario | Outcome | Report |\n|---|---|---|\n");
    for (file, title, outcome) in entries {
        md.push_str(&format!("| {title} | {outcome} | [page]({file}) |\n"));
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty execution record: enough for the renderer's scaffold (the graph
    /// renders draw from `frames`, so an empty tx renders an empty-but-valid page).
    fn empty_tx(error: Option<String>) -> Transaction {
        Transaction {
            frames: vec![],
            account_keys: vec![],
            logs: vec![],
            error,
            compute_units: 0,
            fee: None,
            message: Default::default(),
            trace: None,
            return_data: None,
            aliases: Default::default(),
            instruction_names: Default::default(),
            error_names: Default::default(),
            events: Default::default(),
        }
    }

    #[test]
    fn index_injects_heading_and_intro_over_a_fixed_table() {
        let entries = vec![
            ("create.md".into(), "Create an asset".into(), "succeeded".into()),
            ("reject.md".into(), "Reject a forgery".into(), "failed".into()),
        ];
        let md = render_index("My heading", "My intro.", &entries);
        assert!(md.starts_with("# My heading\n\nMy intro.\n\n"));
        assert!(md.contains("| Scenario | Outcome | Report |\n|---|---|---|\n"));
        assert!(md.contains("| Create an asset | succeeded | [page](create.md) |\n"));
        assert!(md.contains("| Reject a forgery | failed | [page](reject.md) |\n"));
    }

    #[test]
    fn scenario_renders_outcome_and_the_four_sections() {
        let ok = render_scenario("/nope", "T", "intent", "tests/x.rs", "go", &empty_tx(None));
        assert!(ok.starts_with("# T\n\n**Intent.** intent\n\n"));
        assert!(ok.contains("**Outcome.** The transaction succeeded.\n\n"));
        for section in [
            "## Structured execution log",
            "## Sequence diagram",
            "## Authority graph",
            "## Ownership graph",
        ] {
            assert!(ok.contains(section), "missing {section}");
        }

        let failed =
            render_scenario("/nope", "T", "intent", "tests/x.rs", "go", &empty_tx(Some("boom".into())));
        assert!(failed.contains("**Outcome.** The transaction failed: `boom`.\n\n"));
    }

    /// `manifest_dir` that can't resolve the test file drops the `#L` anchor
    /// rather than guessing a line; the link still points at the file.
    #[test]
    fn source_link_degrades_to_no_anchor_when_the_file_is_unreadable() {
        let md = render_scenario("/does/not/exist", "T", "i", "tests/x.rs", "go", &empty_tx(None));
        assert!(md.contains("**Source.** [`tests/x.rs::go`](../tests/x.rs)\n\n"));
        assert!(!md.contains("#L"));
    }
}
