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
    solana_pubkey::Pubkey,
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
mod tests {
    use {
        super::*,
        crate::aliases::Aliases,
        crate::cpi::renderer::Renderer,
        crate::cpi::style::Style,
        crate::cpi::test_support::{render_model, RenderInput},
        crate::cpi::EventRegistry,
        proptest::prelude::*,
        solana_pubkey::Pubkey,
    };

    fn default_aliases() -> Aliases {
        Aliases::with_well_known()
    }

    /// Render the tree BODY (no header/footer) from a log stream, going through
    /// the neutral [`from_transaction`](crate::cpi::model::from_transaction)
    /// path. The body is what the old `render_with` exercised; the full framing
    /// is checked separately by the [`TreeRenderer`] literal tests below.
    fn render_with(
        logs: &[String],
        inner_data: &[Vec<u8>],
        aliases: &Aliases,
        per_root: Vec<Vec<Pubkey>>,
        tx_signers: Vec<Pubkey>,
    ) -> String {
        let model = render_model(RenderInput {
            logs,
            inner_data,
            per_root,
            tx_signers,
        });
        let empty_events = EventRegistry::new();
        let mut collector = super::super::renderer::LegendCollector::new(aliases, &empty_events);
        fmt_tree(&model.roots, &model.tx_signers, &mut collector, Style::Off)
    }

    #[test]
    fn render_substitutes_program_ids() {
        let logs = vec![
            "Program CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5 invoke [1]".to_string(),
            "Program 11111111111111111111111111111111 invoke [2]".to_string(),
            "Program 11111111111111111111111111111111 success".to_string(),
            "Program CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5 success".to_string(),
        ];
        let out = render_with(&logs, &[], &default_aliases(), vec![], vec![]);
        assert!(
            out.contains("System"),
            "expected System substitution; got:\n{out}"
        );
        assert!(
            out.contains("CYbYnHW7"),
            "expected unknown program ID to pass through; got:\n{out}"
        );
        assert!(
            !out.contains("Program 11111111111111111111111111111111"),
            "raw System pubkey leaked through; got:\n{out}"
        );
    }

    #[test]
    fn a_registered_event_renders_as_a_tree_line_with_aliased_fields() {
        use base64::{engine::general_purpose, Engine as _};
        use std::sync::Arc;

        let maker = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();

        // A decoder whose fields embed the maker's base58 key, to prove the tree
        // substitutes it for the alias just as the mermaid note does.
        let mut reg = EventRegistry::new();
        let maker_b58 = maker.to_string();
        reg.register(
            [7u8; 8],
            "Transfer",
            Arc::new(move |_b: &[u8]| {
                Some(vec![
                    ("from".to_string(), maker_b58.clone()),
                    ("amount".to_string(), "100".to_string()),
                ])
            }),
        );
        let mut raw = [7u8; 8].to_vec();
        raw.extend_from_slice(&100u64.to_le_bytes());
        let payload = general_purpose::STANDARD.encode(&raw);

        let frame = crate::cpi::model::ResolvedFrame {
            program: escrow,
            instruction_name: Some("Make".to_string()),
            outcome: crate::cpi::model::Outcome::Success,
            compute_units: Some(5000),
            accounts: vec![],
            logs: vec![crate::cpi::model::FrameLog::Data(payload)],
            data: vec![],
            children: vec![],
        };
        let model = crate::cpi::model::CpiModel {
            header: None,
            roots: vec![crate::cpi::model::Root {
                signers: vec![],
                frame,
            }],
            tx_signers: vec![],
            error: None,
            compute_units: 5000,
            fee: 0,
            events: reg,
        };
        let aliases = Aliases::with_well_known()
            .with(maker, "maker")
            .with(escrow, "escrow");

        let out = TreeRenderer { style: Style::Off }.render(&model, &aliases);

        assert!(
            out.contains("🔔 Transfer"),
            "expected a decoded event header in the tree; got:\n{out}"
        );
        // The `from` field, on its own aligned line, shows the alias not base58.
        assert!(
            out.lines()
                .any(|l| l.contains("from:") && l.contains("maker")),
            "alias not substituted on the from line; got:\n{out}"
        );
        assert!(
            !out.contains(&maker.to_string()),
            "raw base58 leaked into the tree; got:\n{out}"
        );
    }

    #[test]
    fn render_failure_prefers_anchor_name_over_runtime_message() {
        // End-to-end through the parser + renderer: a log stream that
        // contains both the AnchorError line and the runtime's
        // "custom program error: 0x1770" should render with the friendly
        // name as the `Error: ...` line, not the raw runtime message.
        use std::str::FromStr;
        let escrow_id = "H1GjRKWSauAuupurDtGiY5uvhLBtUngNhvrSBs75rH9o";
        let logs = vec![
            format!("Program {escrow_id} invoke [1]"),
            "Program log: Instruction: Take".to_string(),
            "Program log: AnchorError thrown in programs/escrow/src/instructions/take.rs:42. Error Code: EscrowExpired. Error Number: 6000. Error Message: EscrowExpired."
                .to_string(),
            format!("Program {escrow_id} consumed 5000 of 200000 compute units"),
            format!("Program {escrow_id} failed: custom program error: 0x1770"),
        ];
        let taker = Pubkey::new_unique();
        let aliases = Aliases::with_well_known()
            .with(taker, "Taker")
            .with(Pubkey::from_str(escrow_id).unwrap(), "escrow");
        let out = render_with(&logs, &[], &aliases, vec![vec![taker]], vec![taker]);
        assert!(
            out.contains("Error: EscrowExpired"),
            "expected friendly name; got:\n{out}"
        );
        assert!(
            !out.contains("custom program error: 0x1770"),
            "raw runtime message should be suppressed when name available; got:\n{out}"
        );
    }

    #[test]
    fn render_annotates_inner_instructions_via_decoder() {
        let logs = vec![
            "Program CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5 invoke [1]".to_string(),
            "Program 11111111111111111111111111111111 invoke [2]".to_string(),
            "Program 11111111111111111111111111111111 success".to_string(),
            "Program CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5 success".to_string(),
        ];

        // System::Transfer: 4-byte little-endian tag 2. DFS data: root, child.
        let inner_data = vec![vec![], vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]];

        let out = render_with(&logs, &inner_data, &default_aliases(), vec![], vec![]);
        assert!(
            out.contains("System::Transfer"),
            "expected System::Transfer annotation; got:\n{out}"
        );
    }

    #[test]
    fn render_emits_signer_annotation_on_top_level_frame() {
        use std::str::FromStr;
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            format!("Program {amm_id} consumed 4079 of 200000 compute units"),
            format!("Program {amm_id} success"),
        ];
        let admin = Pubkey::new_unique();
        let aliases = Aliases::with_well_known()
            .with(admin, "admin")
            .with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(&logs, &[], &aliases, vec![vec![admin]], vec![admin]);
        let expected = "\
Transaction  signers=[admin]
└── amm [1] ✓ 4079cu  signer=admin
";
        assert_eq!(out, expected);
    }

    #[test]
    fn render_emits_per_root_signer_for_multi_signer_multi_ix() {
        use std::str::FromStr;
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            format!("Program {amm_id} success"),
            format!("Program {amm_id} invoke [1]"),
            format!("Program {amm_id} success"),
        ];
        let alice = Pubkey::new_unique();
        let bob = Pubkey::new_unique();
        let aliases = Aliases::with_well_known()
            .with(alice, "alice")
            .with(bob, "bob")
            .with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(
            &logs,
            &[],
            &aliases,
            vec![vec![alice], vec![bob]],
            vec![alice, bob],
        );
        let expected = "\
Transaction  signers=[alice, bob]
├── amm [1] ✓ (no cu)  signer=alice
└── amm [1] ✓ (no cu)  signer=bob
";
        assert_eq!(out, expected);
    }

    #[test]
    fn render_omits_signer_annotation_on_cpi_frames() {
        use std::str::FromStr;
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let token_id = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            format!("Program {token_id} invoke [2]"),
            format!("Program {token_id} success"),
            format!("Program {amm_id} success"),
        ];
        let admin = Pubkey::new_unique();
        let aliases = Aliases::with_well_known()
            .with(admin, "admin")
            .with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(&logs, &[], &aliases, vec![vec![admin]], vec![admin]);
        assert!(
            out.contains("└── amm [1] ✓ (no cu)  signer=admin\n"),
            "expected amm[1] to have signer=admin; got:\n{out}"
        );
        assert!(
            out.contains("└── Token [2] ✓ (no cu)\n"),
            "expected Token[2] frame without signer= annotation; got:\n{out}"
        );
        assert!(
            !out.contains("Token [2] ✓ (no cu)  signer="),
            "Token CPI should not carry signer= annotation; got:\n{out}"
        );
    }

    #[test]
    fn render_fee_payer_signer_appears_on_all_frames_referencing_it() {
        // Documents the semantic: signer=X means "X is a tx-required signer
        // whose pubkey is referenced in this ix's accounts", NOT "X authorized
        // this ix". A fee payer that appears in every ix's account list
        // (common) shows up in signer= everywhere.
        use std::str::FromStr;
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            format!("Program {amm_id} success"),
            format!("Program {amm_id} invoke [1]"),
            format!("Program {amm_id} success"),
        ];
        let admin = Pubkey::new_unique();
        let aliases = Aliases::with_well_known()
            .with(admin, "admin")
            .with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(
            &logs,
            &[],
            &aliases,
            vec![vec![admin], vec![admin]],
            vec![admin],
        );
        assert!(out.contains("├── amm [1] ✓ (no cu)  signer=admin\n"));
        assert!(out.contains("└── amm [1] ✓ (no cu)  signer=admin\n"));
    }

    #[test]
    fn render_truncates_unaliased_pubkeys_in_rich_path() {
        let user_program = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {user_program} invoke [1]"),
            format!("Program {user_program} success"),
        ];
        let aliases = Aliases::with_well_known();
        let out = render_with(&logs, &[], &aliases, vec![vec![]], vec![]);
        assert!(
            out.contains("CYbYnHW7…2yf5"),
            "expected truncated form; got:\n{out}"
        );
        assert!(
            !out.contains(user_program),
            "raw form should not appear when truncating; got:\n{out}"
        );
    }

    #[test]
    fn lock_attack_trace_reads_as_english_with_aliases() {
        // Golden output: three top-level ixs (set_locked, swap, set_locked) all
        // signed by admin. The middle swap has two Token::TransferChecked CPIs.
        // With the alias map populated for admin and amm, the trace should make
        // the "three admin-signed ixs in one tx, one a swap" pattern visible at
        // a glance.
        use std::str::FromStr;

        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let token_id = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            format!("Program {amm_id} consumed 4081 of 200000 compute units"),
            format!("Program {amm_id} success"),
            format!("Program {amm_id} invoke [1]"),
            format!("Program {token_id} invoke [2]"),
            format!("Program {token_id} consumed 105 of 200000 compute units"),
            format!("Program {token_id} success"),
            format!("Program {token_id} invoke [2]"),
            format!("Program {token_id} consumed 105 of 200000 compute units"),
            format!("Program {token_id} success"),
            format!("Program {amm_id} consumed 23615 of 200000 compute units"),
            format!("Program {amm_id} success"),
            format!("Program {amm_id} invoke [1]"),
            format!("Program {amm_id} consumed 4079 of 200000 compute units"),
            format!("Program {amm_id} success"),
        ];

        let admin = Pubkey::new_unique();
        let aliases = Aliases::with_well_known()
            .with(admin, "admin")
            .with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(
            &logs,
            &[],
            &aliases,
            vec![vec![admin], vec![admin], vec![admin]],
            vec![admin],
        );
        let expected = "\
Transaction  signers=[admin]
├── amm [1] ✓ 4081cu  signer=admin
├── amm [1] ✓ 23615cu  signer=admin
│   ├── Token [2] ✓ 105cu
│   └── Token [2] ✓ 105cu
└── amm [1] ✓ 4079cu  signer=admin
";
        assert_eq!(out, expected);
    }

    #[test]
    fn failed_frame_with_children_renders_children_first_then_error() {
        // Mirrors the escrow `take_and_close_fails_after_expiry` shape: a
        // top-level frame fails after one or more CPI children completed.
        // Solana logs the children in invocation order before the parent's
        // post-CPI check fires, so chronologically children precede the
        // error. The renderer must:
        //   1. Render children first, error last.
        //   2. Mark each child `├──` (since the error follows) and the
        //      error `└──` (since it's the actual last node).
        use std::str::FromStr;
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            "Program 11111111111111111111111111111111 invoke [2]".to_string(),
            "Program 11111111111111111111111111111111 success".to_string(),
            format!("Program {amm_id} consumed 1234 of 200000 compute units"),
            format!("Program {amm_id} failed: custom program error: 0x42"),
        ];
        let aliases = default_aliases().with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(&logs, &[], &aliases, vec![], vec![]);

        // Child gets `├──` because the error follows it at the same depth.
        assert!(
            out.contains("├── System"),
            "child must use ├── when an error follows; got:\n{out}"
        );
        // Error gets `└──` as the last node.
        assert!(
            out.contains("└── Error: custom program error: 0x42"),
            "error must use └── as the last node; got:\n{out}"
        );
        // Chronology: child appears before error in the rendered text.
        let child_pos = out.find("System").expect("child line present");
        let error_pos = out.find("Error:").expect("error line present");
        assert!(
            child_pos < error_pos,
            "child must render before error (Solana logs children first); got:\n{out}"
        );
        // And there's only one `└──` per parent (the error) at the child depth.
        let inner_last_markers = out.matches("\n    └──").count();
        assert_eq!(
            inner_last_markers, 1,
            "expected exactly one └── at the child depth (the error); got:\n{out}"
        );
    }

    #[test]
    fn failed_frame_with_no_children_still_uses_end_connector_for_error() {
        // The simpler case: a frame fails without any CPI children. The
        // error is the only child, gets `└──`, and nothing precedes it.
        use std::str::FromStr;
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            format!("Program {amm_id} consumed 100 of 200000 compute units"),
            format!("Program {amm_id} failed: custom program error: 0x7"),
        ];
        let aliases = default_aliases().with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(&logs, &[], &aliases, vec![], vec![]);
        assert!(
            out.contains("└── Error: custom program error: 0x7"),
            "error must be the └── leaf; got:\n{out}"
        );
    }

    proptest! {
        /// render on arbitrary garbage must never panic and must either produce
        /// empty output or start with "Transaction\n".
        #[test]
        fn render_well_formed(logs in prop::collection::vec(".*", 0..50)) {
            let out = render_with(&logs, &[], &default_aliases(), vec![], vec![]);
            prop_assert!(out.is_empty() || out.starts_with("Transaction\n"));
        }
    }
}
