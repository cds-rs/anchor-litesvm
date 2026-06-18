use std::fmt::Write as _;

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

    pub(super) fn render_into(&self, out: &mut String) {
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
pub(super) fn longest_backtick_run(s: &str) -> usize {
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
pub(super) fn fence_delim(body: &str) -> String {
    "`".repeat(longest_backtick_run(body).max(2) + 1)
}

/// Write `body` as a fenced code block whose delimiter dominates any backtick
/// run inside it (see [`fence_delim`]). `lang` is the info string (`""` for a
/// bare fence). This is the one place the writer materializes a fence, so both
/// a `Fenced` block and a contained-as-literal `Raw` fragment go through it.
pub(super) fn write_fenced(out: &mut String, lang: &str, body: &str) {
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
pub(super) fn fences_balanced(s: &str) -> bool {
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

/// Wrap rendered child Markdown in a collapsible `<details>` disclosure.
///
/// `<details>` is a CommonMark "type 6" HTML block: after the blank line that
/// follows `<summary>`, normal Markdown parsing resumes, so a ```` ```mermaid ````
/// diagram inside renders *as a diagram* (not literal text) while the
/// `</details>` boundary still keeps it from leaking. The one collision is a
/// child that literally contains `</details>`; in that (pathological) case we
/// drop the disclosure and fall back to a bold label, so the close tag is never
/// emitted against content that already closed it.
pub(super) fn render_details(summary: &str, inner: &str) -> String {
    if inner.contains("</details>") {
        return format!("**{summary}**\n\n{inner}");
    }
    format!("<details>\n<summary>{summary}</summary>\n\n{inner}\n</details>")
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
mod fence_tests {
    use super::{fence_delim, fences_balanced, longest_backtick_run, MarkdownBlock};
    use crate::report::Report;

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
