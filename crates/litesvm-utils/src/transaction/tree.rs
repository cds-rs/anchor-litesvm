//! Render a resolved CPI model as an annotated box-drawing tree.
//!
//! This is a [`Renderer`] adapter: it gets everything from the
//! [`CpiModel`](super::model::CpiModel) (the shared transformation does
//! the `cpi_tree` walk, name decode, and error-name resolution) and owns
//! its own framing: the `── program::ix ──` section header, the tree body,
//! and the `Compute Units / Fee / Legend` footer.
//!
//! Program IDs and signer pubkeys are substituted with friendly names via
//! the shared [`LegendCollector`], which both resolves and records each
//! `(name, Pubkey)` pair seen during the pass; the footer drains the
//! recorded pairs into the legend. Unaliased pubkeys are truncated to
//! `<8>…<4>` so trees stay narrow.

use {
    super::model::{CpiModel, FrameLog, Outcome, ResolvedFrame, Root},
    super::renderer::{LegendCollector, Renderer},
    solana_program::pubkey::Pubkey,
    std::fmt::Write,
};

const TREE_BRANCH: &str = "├── ";
const TREE_END: &str = "└── ";
const TREE_CONT: &str = "│   ";
const TREE_EMPTY: &str = "    ";

/// Total width (in `char`s) the section header fills with the trailing `─`
/// rule. Slightly wider than typical tree content (~45–55 chars) so the
/// rule visibly "extends past" the body and reads as a section break.
const HEADER_WIDTH: usize = 60;

/// Minimum trailing `─` count when the title alone exceeds `HEADER_WIDTH`
/// (very long program::ix names), so headers never collapse into the title.
const HEADER_MIN_TRAILING: usize = 4;

/// The box-drawing tree renderer.
pub(super) struct TreeRenderer {
    pub style: super::style::Style,
}

impl Renderer for TreeRenderer {
    /// Full structured output for a transaction:
    ///
    /// ```text
    /// ── <program>::<ix-name> ────────────────────…  (single-ix; batches omit; rule fills to HEADER_WIDTH)
    /// Transaction  signers=[...]
    /// <tree body>
    /// Error: ...                                     (tx-level failure only)
    /// Compute Units (this run): N
    /// Fee: N lamports
    /// Legend (M):                                    (omitted if no user aliases used)
    ///   alice          = <full base58 pubkey>
    /// ```
    ///
    /// The footer is one tight block (no internal blank) so the gap
    /// between transactions (the next render's leading `\n`) stays strictly
    /// larger than any gap inside one. The legend lists only aliases that
    /// appeared in this render (insertion-ordered, well-known names filtered).
    fn render(&self, model: &CpiModel, aliases: &super::aliases::Aliases) -> String {
        let style = self.style;
        let mut collector = LegendCollector::new(aliases, &model.events);
        let mut out = String::new();
        // Leading blank line separates our header from whatever the test
        // runner just printed (test name, prior assertions, etc.).
        out.push('\n');

        if let Some(header) = &model.header {
            let program_display = collector.render_pubkey(&header.program);
            // Single-line section opener in place of the old `=== ... ===`
            // pair. Batches (no header) let the `Transaction signers=[...]`
            // line lead. The trailing rule fills to HEADER_WIDTH so the eye
            // catches the boundary even when the previous legend ran long.
            let title = match &header.instruction_name {
                Some(name) => format!("── {program_display}::{name} "),
                None => format!("── {program_display} "),
            };
            push_section_header(&mut out, &title);
        }

        out.push_str(&fmt_tree(
            &model.roots,
            &model.tx_signers,
            &mut collector,
            style,
        ));

        if let Some(err) = &model.error {
            out.push_str(&format!("{}\n", style.red(&format!("Error: {err}"))));
        }
        out.push_str(&format!(
            "Compute Units (this run): {}\n",
            model.compute_units
        ));
        out.push_str(&format!("Fee: {} lamports\n", model.fee));

        let entries: Vec<(&str, Pubkey)> = collector
            .into_entries()
            .into_iter()
            .filter(|(name, _)| !super::aliases::is_well_known_name(name))
            .collect();
        if !entries.is_empty() {
            // No leading blank: keep CU / Fee / Legend as one tight footer
            // block (see the doc comment for why).
            let width = entries.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
            out.push_str(&format!("Legend ({}):\n", entries.len()));
            for (name, pk) in &entries {
                out.push_str(&format!("  {name:<width$} = {pk}\n"));
            }
        }
        out
    }
}

/// Write `title` followed by enough `─` to reach `HEADER_WIDTH` (or
/// `HEADER_MIN_TRAILING` dashes when the title is already wider), then a
/// newline. `title` is expected to end with a single ASCII space so the
/// rule sits flush against it.
fn push_section_header(out: &mut String, title: &str) {
    out.push_str(title);
    let trailing = HEADER_WIDTH
        .saturating_sub(title.chars().count())
        .max(HEADER_MIN_TRAILING);
    for _ in 0..trailing {
        out.push('─');
    }
    out.push('\n');
}

/// Render the tree body (the `Transaction` line + the box-drawing forest),
/// resolving pubkeys through `collector`. Exposed so the body tests can
/// drive it directly without the header/footer framing.
///
/// Returns an empty string if the model has no roots; otherwise the body
/// prefixed by `"Transaction"` (plus a `signers=[...]` annotation when the
/// tx has required signers).
pub(super) fn fmt_tree(
    roots: &[Root],
    tx_signers: &[Pubkey],
    collector: &mut LegendCollector<'_>,
    style: super::style::Style,
) -> String {
    if roots.is_empty() {
        return String::new();
    }
    let mut out = String::from("Transaction");
    if !tx_signers.is_empty() {
        let names: Vec<String> = tx_signers
            .iter()
            .map(|pk| collector.render_pubkey(pk))
            .collect();
        let _ = write!(out, "  signers=[{}]", names.join(", "));
    }
    out.push('\n');

    let last = roots.len() - 1;
    for (i, root) in roots.iter().enumerate() {
        render_frame(
            &root.frame,
            "",
            i == last,
            1,
            collector,
            Some(&root.signers),
            style,
            &mut out,
        );
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn render_frame(
    frame: &ResolvedFrame,
    ancestor_prefix: &str,
    is_last: bool,
    depth: usize,
    collector: &mut LegendCollector<'_>,
    signer_set: Option<&Vec<Pubkey>>,
    style: super::style::Style,
    out: &mut String,
) {
    let instruction = frame.instruction_name.as_deref();

    let connector = if is_last { TREE_END } else { TREE_BRANCH };
    let program_display = collector.render_pubkey(&frame.program);
    let _ = match instruction {
        Some(name) => write!(
            out,
            "{}{}{}::{} [{}]",
            ancestor_prefix, connector, program_display, name, depth
        ),
        None => write!(
            out,
            "{}{}{} [{}]",
            ancestor_prefix, connector, program_display, depth
        ),
    };
    match &frame.outcome {
        Outcome::Success => {
            out.push(' ');
            out.push_str(&style.green("✓"));
        }
        Outcome::Failed { .. } => {
            out.push(' ');
            out.push_str(&style.red("✗"));
        }
        Outcome::Truncated => {
            out.push(' ');
            out.push_str(&style.dim("(truncated)"));
        }
    }
    // `compute_units = None` means the log stream had no `consumed N of M`
    // line for this frame. Native programs (System, BPF Loader) don't emit
    // it; surface the absence explicitly so a reader doesn't mistake it for
    // a parser drop.
    match frame.compute_units {
        Some(cu) => {
            let _ = write!(out, " {}cu", cu);
        }
        None => {
            out.push(' ');
            out.push_str(&style.dim("(no cu)"));
        }
    }
    // Signer annotation: top-level frames only (depth == 1) and only when a
    // signer_set is supplied. signer=X means "X is a tx-required signer
    // whose pubkey is referenced in this ix's accounts", NOT "X authorized
    // this ix". Fee payers that appear in many ixs still show up here.
    if depth == 1 {
        if let Some(set) = signer_set {
            if !set.is_empty() {
                let names: Vec<String> = set.iter().map(|pk| collector.render_pubkey(pk)).collect();
                if names.len() == 1 {
                    let _ = write!(out, "  signer={}", names[0]);
                } else {
                    let _ = write!(out, "  signer=[{}]", names.join(", "));
                }
            }
        }
    }
    out.push('\n');

    let descendant_prefix = format!(
        "{}{}",
        ancestor_prefix,
        if is_last { TREE_EMPTY } else { TREE_CONT }
    );

    // Decoded events this frame emitted, each as a labelled block: a `🔔 Name`
    // header then one aligned `field: value` line. Only *registered* events
    // render (an unregistered one is opaque base64, omitted here; the mermaid
    // view keeps a raw arrow for those). Placed before children: the frame
    // announced the event, then its sub-calls ran, so `│` continues the frame's
    // spine down to those children (or its failure line) when they follow.
    let more_follows = !frame.children.is_empty()
        || matches!(&frame.outcome, Outcome::Failed { message: Some(_) });
    let conn = if more_follows { "│" } else { " " };
    for entry in &frame.logs {
        let FrameLog::Data(payload) = entry else {
            continue;
        };
        if let Some(info) = collector.decode_event(payload) {
            write_event_block(out, &descendant_prefix, conn, &info);
        }
    }
    // A self-CPI event (`emit_cpi!`-style): the frame's own instruction data
    // carries the event payload, with no `Program data:` log. The trace put that
    // data on the frame; decode and render it the same way as a logged event. A
    // frame whose data isn't a registered event misses cheaply (same as the
    // logged path above, which also tries every `Data` entry unguarded).
    if let Some(info) = collector.decode_cpi_event(&frame.program, &frame.data) {
        write_event_block(out, &descendant_prefix, conn, &info);
    }

    // Order under a frame: children first (in invocation order), then the
    // failure line. Solana logs inner CPIs before the parent's post-CPI
    // check fires, so chronologically children precede the error. The
    // `last` flag picks the connector: only the truly-last node at this
    // depth gets `└──`. When there's an error, no child can be last (the
    // error follows); when there isn't, the last child is last.
    let has_error_msg = matches!(
        &frame.outcome,
        Outcome::Failed { message } if message.is_some()
    );
    let children = &frame.children;
    for (i, child) in children.iter().enumerate() {
        let child_is_last = !has_error_msg && i + 1 == children.len();
        // CPI children never get signer annotations (None).
        render_frame(
            child,
            &descendant_prefix,
            child_is_last,
            depth + 1,
            collector,
            None,
            style,
            out,
        );
    }
    if let Outcome::Failed { message } = &frame.outcome {
        // `message` is already the Anchor error name when one was present
        // (resolved in the model), else the runtime message.
        render_failure(message.as_deref(), &descendant_prefix, style, out);
    }
}

/// Render one decoded event under a frame: a `🔔 Name` header then one aligned
/// `field: value` line per field, on the frame's spine (`conn`). Shared by the
/// logged-event path (`Program data:`) and the self-CPI event path.
fn write_event_block(out: &mut String, prefix: &str, conn: &str, info: &super::events::EventInfo) {
    let _ = writeln!(out, "{prefix}{conn} 🔔 {}", info.name);
    let width = info.fields.iter().map(|(k, _)| k.len()).max().unwrap_or(0) + 1;
    let last = info.fields.len().saturating_sub(1);
    for (i, (key, value)) in info.fields.iter().enumerate() {
        let field = format!("{key}:");
        let comma = if i == last { "" } else { "," };
        let _ = writeln!(out, "{prefix}{conn}      {field:<width$} {value}{comma}");
    }
}

/// Render the optional error-message line under a failed frame. Shares
/// `descendant_prefix` (carrying the parent frame's vertical bar) and uses
/// the same └── connector as a sole child, so the error aligns with the
/// rest of the subtree. Long messages are split on `. ` for readability.
/// The whole `Error: ...` line is wrapped in red when `style` is `On`.
fn render_failure(
    message: Option<&str>,
    descendant_prefix: &str,
    style: super::style::Style,
    out: &mut String,
) {
    let Some(msg) = message else { return };
    let item = format!("Error: {}", msg);
    let mut chunks = item.split(". ");
    if let Some(first) = chunks.next() {
        let _ = writeln!(out, "{}{}{}", descendant_prefix, TREE_END, style.red(first));
    }
    for chunk in chunks {
        let _ = writeln!(
            out,
            "{}{} {}",
            descendant_prefix,
            TREE_EMPTY,
            style.red(chunk)
        );
    }
}

#[cfg(test)]
mod tests;
