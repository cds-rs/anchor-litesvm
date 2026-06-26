//! Render a transaction's CPI invocation tree as a Mermaid
//! `sequenceDiagram` block.
//!
//! Walks the same [`CpiFrame`] tree as [`super::tree::render`] but
//! emits participants + arrows instead of the box-drawing format.
//! Two rendering modes:
//!
//! - [`Mode::Plain`]: fire-and-forget `->>` arrows, one per CPI edge.
//!   Failed frames get a trailing `note over <target>: ✗ <msg>`.
//!   Compact; reads cleanly for shallow CPI trees but does not show
//!   round-trip nesting.
//! - [`Mode::Lifelines`]: paired `->>+` (call, activate) and `-->>-`
//!   (return, deactivate) arrows. The forward arrow carries the
//!   instruction name; the return arrow carries `ok` (or the error
//!   message) and the compute-unit total. Failed frames return with
//!   the `--x` "lost message" arrow. Shows the synchronous
//!   `parent-stays-active-while-children-run` nesting that the Plain
//!   mode hides, at the cost of roughly doubling the line count.
//!
//! Reuses [`super::tree::LegendCollector`] for alias resolution so the
//! participant set lines up with the names the structured renderer
//! would show for the same transaction.

use {
    super::model::{CpiModel, FrameLog, Outcome, ResolvedFrame},
    super::renderer::{LegendCollector, Renderer},
    std::fmt::Write,
};

pub(super) const INDENT: &str = "    ";

/// Selects between the two emit styles. See module docs for the
/// tradeoff. Defaults to `Plain` everywhere `render` is called without
/// an explicit mode.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Plain,
    Lifelines,
}

/// The Mermaid `sequenceDiagram` renderer. Owns its framing (the fenced
/// `mermaid` block + participant declarations); there is no external
/// header/footer for this format.
pub(super) struct MermaidRenderer {
    pub mode: Mode,
    pub include_logs: bool,
}

impl Renderer for MermaidRenderer {
    fn render(&self, model: &CpiModel, aliases: &super::aliases::Aliases) -> String {
        let mut collector = LegendCollector::new(aliases, &model.events);
        render(model, self.mode, self.include_logs, &mut collector)
    }
}

/// One line in the body of the `sequenceDiagram` block, deferred until
/// after the participants block.
///
/// Plain-mode variants:
///   - `Call`: a normal `->>` arrow, instruction-name labelled.
///   - `ErrorNote`: a `note over <target>: ✗ <msg>` line emitted after
///     a failed frame's children have been walked.
///
/// Lifelines-mode variants:
///   - `CallActivate`: a `->>+` arrow (start the callee's lifeline).
///   - `Return`: a `-->>-` arrow (end the lifeline with `ok (Ncu)`).
///   - `ErrorReturn`: a `--x` "lost message" arrow carrying the error
///     and the compute-unit total.
///
/// Region wrappers (both modes):
///   - `RectBegin` / `RectEnd`: emit `rect rgb(r,g,b)` and `end` lines
///     that paint a tinted background under everything between them.
///     Used to mark failed-frame regions in pale red so the failure
///     is visible at a glance even when the `--x` or `note over` are
///     not enough on their own.
///
/// Splitting Call vs Return (or ErrorNote vs ErrorReturn) so they
/// sequence independently is what keeps the chronology honest:
/// children's calls render before the parent's return, because Solana
/// runs inner CPIs before the parent's post-CPI check fires. The
/// structured tree renderer made the same fix in commit e959b2d.
enum Line {
    // Plain mode
    Call {
        source: String,
        target: String,
        label: String,
    },
    ErrorNote {
        target: String,
        message: String,
    },
    // Lifelines mode
    CallActivate {
        source: String,
        target: String,
        label: String,
    },
    Return {
        source: String,
        target: String,
        label: String,
    },
    ErrorReturn {
        source: String,
        target: String,
        label: String,
    },
    // Region wrappers
    RectBegin {
        rgb: (u8, u8, u8),
    },
    RectEnd,
    // Informational notifications back to the tx initiator
    Event {
        source: String,
        target: String,
        label: String,
    },
    Log {
        source: String,
        target: String,
        label: String,
    },
    /// A decoded event rendered as a `note over <emitter>` annotation rather
    /// than an arrow: an event is something the frame *recorded*, not a message
    /// it sent anywhere. Used when a decoder is registered for the event;
    /// undecoded events keep the informational [`Event`](Self::Event) arrow.
    EventNote {
        target: String,
        label: String,
    },
}

/// Light red used to tint the background of a failed-frame region.
/// Picked to read as "this region failed" without clashing with text
/// in either light or dark Mermaid themes.
const FAIL_RECT_RGB: (u8, u8, u8) = (255, 220, 220);

/// Maximum `event:` payload length rendered inline before truncation.
/// Event data is base64-encoded Anchor events; full payloads can run
/// hundreds of chars and break the diagram layout. Cap at this many
/// chars with an ellipsis suffix.
const EVENT_LABEL_MAX: usize = 60;

/// Whether `Program log: ...` lines should be surfaced as informational
/// arrows in the diagram. Events (`Program data: ...`) are always
/// surfaced; logs are typically noisier (3-10 per ix) and only
/// useful when investigating a specific test.
///
/// Set `ANCHOR_LITESVM_MERMAID_LOGS=1` in the environment to opt in.
/// Same convention as `ANCHOR_LITESVM_COLOR` in `style.rs`: presence
/// of the variable enables, any value works, unset disables.
pub(super) fn detect_include_logs() -> bool {
    std::env::var_os("ANCHOR_LITESVM_MERMAID_LOGS").is_some()
}

/// Render the invocation tree as a Mermaid `sequenceDiagram` block,
/// wrapped in a fenced ```mermaid code block ready to drop into a
/// markdown file.
///
/// Returns an empty string if the log stream contains no invocations.
///
/// `include_logs` controls whether `Program log: ...` lines surface as
/// informational `💬 log:` arrows back to the tx initiator. Events
/// (`Program data: ...`) always render, since they are scarce and
/// structurally meaningful.
pub(super) fn render(
    model: &CpiModel,
    mode: Mode,
    include_logs: bool,
    collector: &mut LegendCollector<'_>,
) -> String {
    if model.roots.is_empty() {
        return String::new();
    }

    let mut participants: Vec<String> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();

    // Source for each root: the per-root signer (first one) when the tx
    // specifies a required signer for the ix, else the fee payer
    // (`tx_signers[0]` by Solana's message format). With no signers at all
    // (vanishingly rare), fall back to a literal `User` so the diagram
    // still parses.
    let default_source = model
        .tx_signers
        .first()
        .map(|pk| collector.render_pubkey(pk))
        .unwrap_or_else(|| "User".to_string());

    for root in &model.roots {
        let root_source = root
            .signers
            .first()
            .map(|pk| collector.render_pubkey(pk))
            .unwrap_or_else(|| default_source.clone());
        record_participant(&root_source, &mut participants);
        walk_frame(
            &root.frame,
            &root_source,
            &root_source,
            mode,
            include_logs,
            collector,
            &mut participants,
            &mut lines,
        );
    }

    let mut out = String::new();
    out.push_str("```mermaid\n");
    out.push_str("sequenceDiagram\n");
    out.push_str(INDENT);
    out.push_str("autonumber\n");
    for p in &participants {
        emit_participant(&mut out, p);
    }
    for line in &lines {
        match line {
            Line::Call {
                source,
                target,
                label,
            } => {
                let _ = writeln!(
                    out,
                    "{INDENT}{} ->> {}: {}",
                    mermaid_id(source),
                    mermaid_id(target),
                    label,
                );
            }
            Line::ErrorNote { target, message } => {
                let _ = writeln!(
                    out,
                    "{INDENT}note over {}: ✗ {}",
                    mermaid_id(target),
                    message,
                );
            }
            Line::EventNote { target, label } => {
                let _ = writeln!(out, "{INDENT}note over {}: {}", mermaid_id(target), label);
            }
            Line::CallActivate {
                source,
                target,
                label,
            } => {
                let _ = writeln!(
                    out,
                    "{INDENT}{} ->>+ {}: {}",
                    mermaid_id(source),
                    mermaid_id(target),
                    label,
                );
            }
            Line::Return {
                source,
                target,
                label,
            } => {
                let _ = writeln!(
                    out,
                    "{INDENT}{} -->>- {}: {}",
                    mermaid_id(source),
                    mermaid_id(target),
                    label,
                );
            }
            Line::ErrorReturn {
                source,
                target,
                label,
            } => {
                let _ = writeln!(
                    out,
                    "{INDENT}{} --x {}: {}",
                    mermaid_id(source),
                    mermaid_id(target),
                    label,
                );
            }
            Line::RectBegin { rgb: (r, g, b) } => {
                let _ = writeln!(out, "{INDENT}rect rgb({r}, {g}, {b})");
            }
            Line::RectEnd => {
                let _ = writeln!(out, "{INDENT}end");
            }
            Line::Event {
                source,
                target,
                label,
            } => {
                let _ = writeln!(
                    out,
                    "{INDENT}{} -->> {}: {}",
                    mermaid_id(source),
                    mermaid_id(target),
                    label,
                );
            }
            Line::Log {
                source,
                target,
                label,
            } => {
                let _ = writeln!(
                    out,
                    "{INDENT}{} -->> {}: {}",
                    mermaid_id(source),
                    mermaid_id(target),
                    label,
                );
            }
        }
    }
    out.push_str("```\n");
    out
}

/// Truncate an event payload to keep the diagram readable. Anchor
/// emits events as base64 `Program data: ...` lines that can run to
/// hundreds of characters; rendering the full payload inline blows up
/// the arrow label. Mermaid breaks on long unbroken labels (no soft
/// wrap), so we cap and suffix with `…`.
fn truncate_payload(payload: &str) -> String {
    // Strip a trailing newline if present (cpi_tree usually has done
    // this already, belt-and-braces).
    let clean = payload.trim_end();
    if clean.len() <= EVENT_LABEL_MAX {
        clean.to_string()
    } else {
        format!("{}…", &clean[..EVENT_LABEL_MAX])
    }
}

/// Cap a *decoded* event label (the `🔔 Name { fields }` string) the same way
/// [`truncate_payload`] caps a raw payload, but by `char` rather than byte: the
/// label opens with a multibyte `🔔` and a field value may be non-ASCII, so a
/// byte slice could land mid-codepoint and panic. Caps at [`EVENT_LABEL_MAX`]
/// characters and suffixes with `…`.
fn truncate_label(label: &str) -> String {
    if label.chars().count() <= EVENT_LABEL_MAX {
        label.to_string()
    } else {
        format!(
            "{}…",
            label.chars().take(EVENT_LABEL_MAX).collect::<String>()
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_frame(
    frame: &ResolvedFrame,
    source: &str,
    root_initiator: &str,
    mode: Mode,
    include_logs: bool,
    collector: &mut LegendCollector<'_>,
    participants: &mut Vec<String>,
    lines: &mut Vec<Line>,
) {
    let target = collector.render_pubkey(&frame.program);
    record_participant(&target, participants);

    // An undecoded instruction has no name. The structured tree shows a bare
    // `?`, which is fine as plain text, but a Mermaid sequence message of
    // exactly `?` (no compute suffix to pad it) fails to parse on GitHub, so
    // the whole diagram renders as a raw fenced block. Use a word instead.
    //
    // The label is the name plus the decoded operand summary (a
    // `System::Transfer`'s parties + lamports), aliased through the collector.
    // Mermaid uses the unicode `→`: an ASCII `->` reads as a stray arrow token
    // in a sequence-message label.
    let resolved = crate::interpret::ResolvedFact {
        name: frame.instruction_name.clone(),
        operands: collector.resolve_operands(&frame.operands),
    };
    let summary = resolved.summary("→");
    let ix_name = match (&resolved.name, summary.is_empty()) {
        (Some(name), false) => format!("{name} {summary}"),
        (Some(name), true) => name.clone(),
        (None, false) => summary,
        (None, true) => "unnamed".to_string(),
    };
    let cu_suffix = match frame.compute_units {
        Some(cu) => format!(" ({}cu)", cu),
        None => String::new(),
    };

    // Push the forward arrow. Plain mode bakes CU into the call label
    // (no return arrow to put it on); Lifelines mode keeps the call
    // label terse and saves CU for the return arrow, since CU is the
    // *measured* value at frame end, not a property of the call.
    match mode {
        Mode::Plain => {
            lines.push(Line::Call {
                source: source.to_string(),
                target: target.clone(),
                label: format!("{ix_name}{cu_suffix}"),
            });
        }
        Mode::Lifelines => {
            lines.push(Line::CallActivate {
                source: source.to_string(),
                target: target.clone(),
                label: ix_name.to_string(),
            });
        }
    }

    // Children render BEFORE the parent's return / error line: Solana
    // runs the inner CPIs first and then fires the parent's post-CPI
    // check (which is what an Anchor `require!` failure surfaces as).
    // Mirrors the ordering fix tree::render landed in commit e959b2d.
    for child in &frame.children {
        walk_frame(
            child,
            &target,
            root_initiator,
            mode,
            include_logs,
            collector,
            participants,
            lines,
        );
    }

    // Events (`Program data: ...`) and logs (`Program log: ...`)
    // emitted directly by this frame render as informational dashed
    // arrows back to the tx initiator. Events always render; logs
    // only when `include_logs` is set (typically by
    // `ANCHOR_LITESVM_MERMAID_LOGS=1`).
    //
    // Placement: after all children's calls/returns, before this
    // frame's own return/error line. cpi_tree attributes logs to
    // their emitting frame correctly (deeper frames pop before their
    // parent's later logs land), but it does not preserve the
    // interleaving of logs with child invocations within the same
    // frame; rendering them as a trailing block before the return is
    // a faithful approximation and keeps the diagram easy to read.
    if !frame.logs.is_empty() {
        // The initiator participant must be in the participants list
        // before we draw arrows to it; the root iteration already
        // registered it, but record again as a no-op to be explicit.
        record_participant(root_initiator, participants);
        for entry in &frame.logs {
            match entry {
                FrameLog::Data(payload) => match collector.decode_event(payload) {
                    // A registered event: a `note over <emitter>` carrying the
                    // name and destructured (alias-substituted) fields. An event
                    // is an annotation the frame recorded, not a message it sent.
                    Some(info) => {
                        lines.push(Line::EventNote {
                            target: target.clone(),
                            label: truncate_label(&info.badge()),
                        });
                    }
                    // No decoder registered: keep the informational raw-base64
                    // arrow back to the initiator, exactly as before.
                    None => {
                        lines.push(Line::Event {
                            source: target.clone(),
                            target: root_initiator.to_string(),
                            label: format!("🔔 event: {}", truncate_payload(payload)),
                        });
                    }
                },
                FrameLog::Msg(text) if include_logs => {
                    lines.push(Line::Log {
                        source: target.clone(),
                        target: root_initiator.to_string(),
                        label: format!("💬 log: {}", escape_message(text)),
                    });
                }
                FrameLog::Msg(_) => {}
            }
        }
    }

    // Push the return / error line. Failures get the error line
    // wrapped in a tight red `rect` block so the failure marker
    // itself stands out, without tinting the surrounding work (which
    // typically succeeded; in Solana a parent often fails on a
    // post-CPI check after all its children have already returned
    // ok). Nested failures produce a vertical stack of small red
    // marks, one per failed level.
    //
    // For failure messages, prefer the Anchor-decoded error name
    // extracted from the frame's logs (`EscrowExpired`) over the
    // runtime's raw `custom program error: 0x1770`. Falls back to
    // the runtime message for non-Anchor failures.
    match (mode, &frame.outcome) {
        (Mode::Plain, Outcome::Failed { message }) => {
            let label = message.as_deref().map(escape_message);
            if let Some(msg) = label {
                lines.push(Line::RectBegin { rgb: FAIL_RECT_RGB });
                lines.push(Line::ErrorNote {
                    target,
                    message: msg,
                });
                lines.push(Line::RectEnd);
            }
            // Failed-without-message AND without an Anchor name: no
            // closing line; the missing context speaks for itself.
        }
        (Mode::Plain, _) => {}

        (Mode::Lifelines, Outcome::Success) => {
            lines.push(Line::Return {
                source: target,
                target: source.to_string(),
                label: format!("ok{cu_suffix}"),
            });
        }
        (Mode::Lifelines, Outcome::Failed { message }) => {
            let msg = message.as_deref().map(escape_message).unwrap_or_default();
            let label = if msg.is_empty() {
                format!("✗{cu_suffix}")
            } else {
                format!("✗ {msg}{cu_suffix}")
            };
            lines.push(Line::RectBegin { rgb: FAIL_RECT_RGB });
            lines.push(Line::ErrorReturn {
                source: target,
                target: source.to_string(),
                label,
            });
            lines.push(Line::RectEnd);
        }
        (Mode::Lifelines, Outcome::Truncated) => {
            // Close the activation so subsequent diagram state stays
            // balanced; label tells the truth.
            lines.push(Line::Return {
                source: target,
                target: source.to_string(),
                label: "(truncated)".to_string(),
            });
        }
    }
}

/// Insert `name` into `participants` if not already present. Order is
/// first-appearance, matching the legend ordering convention.
fn record_participant(name: &str, participants: &mut Vec<String>) {
    if !participants.iter().any(|p| p == name) {
        participants.push(name.to_string());
    }
}

/// Emit a `participant` line. When the display name and the
/// Mermaid-safe identifier diverge (e.g., truncated pubkeys contain
/// `…`), use the `participant Id as "Display"` form so the diagram
/// renders the readable name without breaking Mermaid's parser.
fn emit_participant(out: &mut String, name: &str) {
    let id = mermaid_id(name);
    if id == name {
        let _ = writeln!(out, "{INDENT}participant {id}");
    } else {
        let _ = writeln!(out, "{INDENT}participant {id} as \"{name}\"");
    }
}

/// Mermaid participant identifiers accept ASCII alphanumerics and `_`
/// only; everything else (dots, hyphens, the `…` from pubkey
/// truncation, unicode) is replaced with `_` so the diagram parses.
/// The original name is preserved for display via `participant Id as
/// "name"` when this sanitisation actually changes the string.
pub(super) fn mermaid_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Trim a multi-line error message to its first line and strip any
/// trailing newline. Mermaid `note over` only accepts a single line;
/// long messages with embedded newlines would break the parser.
fn escape_message(msg: &str) -> String {
    msg.lines().next().unwrap_or("").trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::cpi::test_support::{render_model, RenderInput},
        crate::cpi::EventRegistry,
        solana_pubkey::Pubkey,
        std::str::FromStr,
    };

    /// Build the model from `logs` (+ optional inner-frame data, for the name
    /// decode) and render it as Mermaid. The construction goes through the
    /// neutral [`from_transaction`](crate::cpi::model::from_transaction) path
    /// (split out into [`render_model`]); this drives the renderer over the
    /// resulting [`CpiModel`].
    fn render_with(
        logs: &[String],
        inner_data: &[Vec<u8>],
        aliases: &crate::aliases::Aliases,
        per_root: Vec<Vec<Pubkey>>,
        tx_signers: Vec<Pubkey>,
        mode: Mode,
    ) -> String {
        render_with_logs(logs, inner_data, aliases, per_root, tx_signers, mode, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_with_logs(
        logs: &[String],
        inner_data: &[Vec<u8>],
        aliases: &crate::aliases::Aliases,
        per_root: Vec<Vec<Pubkey>>,
        tx_signers: Vec<Pubkey>,
        mode: Mode,
        include_logs: bool,
    ) -> String {
        let model = render_model(RenderInput {
            logs,
            inner_data,
            per_root,
            tx_signers,
        });
        let empty_events = EventRegistry::new();
        let mut collector = super::super::renderer::LegendCollector::new(aliases, &empty_events);
        render(&model, mode, include_logs, &mut collector)
    }

    #[test]
    fn renders_transfer_operands_with_unicode_arrow() {
        // A decoded `System::Transfer` carries typed operands. In a
        // sequence-message label the parties are aliased and the separator is the
        // unicode `→`, which can't be mistaken for an arrow token (the text trees
        // keep the ASCII `->`).
        use crate::interpret::Operand;
        let system = Pubkey::new_unique();
        let from = Pubkey::new_unique();
        let to = Pubkey::new_unique();
        let model = crate::cpi::model::CpiModel {
            header: None,
            roots: vec![crate::cpi::model::Root {
                signers: vec![],
                frame: crate::cpi::model::ResolvedFrame {
                    program: system,
                    instruction_name: Some("Transfer".to_string()),
                    operands: vec![
                        ("from".to_string(), Operand::Pubkey(from)),
                        ("to".to_string(), Operand::Pubkey(to)),
                        ("lamports".to_string(), Operand::Lamports(1_000_000)),
                    ],
                    outcome: crate::cpi::model::Outcome::Success,
                    compute_units: None,
                    accounts: vec![],
                    logs: vec![],
                    data: vec![],
                    children: vec![],
                },
            }],
            tx_signers: vec![],
            error: None,
            compute_units: 0,
            fee: 0,
            events: Default::default(),
        };
        let aliases = crate::aliases::Aliases::default()
            .with(from, "Vault")
            .with(to, "Player");
        let empty_events = EventRegistry::new();
        let mut collector = super::super::renderer::LegendCollector::new(&aliases, &empty_events);

        let out = render(&model, Mode::Plain, false, &mut collector);

        assert!(
            out.contains("Transfer (Vault → Player) 1,000,000 lamports"),
            "operands aliased and the separator is the unicode arrow; got:\n{out}"
        );
        assert!(
            !out.contains(" -> "),
            "the ascii arrow should not survive in a mermaid label; got:\n{out}"
        );
        assert!(
            !out.contains(&from.to_string()),
            "raw base58 leaked into the diagram; got:\n{out}"
        );
    }

    #[test]
    fn empty_log_stream_produces_empty_string() {
        let out = render_with(
            &[],
            &[],
            &crate::aliases::Aliases::default(),
            vec![],
            vec![],
            Mode::Plain,
        );
        assert_eq!(out, "");
    }

    #[test]
    fn single_top_level_frame_emits_signer_to_program_arrow() {
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            "Program log: Instruction: Initialize".to_string(),
            format!("Program {amm_id} consumed 4079 of 200000 compute units"),
            format!("Program {amm_id} success"),
        ];
        let admin = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(admin, "admin")
            .with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(
            &logs,
            &[],
            &aliases,
            vec![vec![admin]],
            vec![admin],
            Mode::Plain,
        );
        let expected = "\
```mermaid
sequenceDiagram
    autonumber
    participant admin
    participant amm
    admin ->> amm: Initialize (4079cu)
```
";
        assert_eq!(out, expected);
    }

    #[test]
    fn nested_cpi_decodes_inner_instruction_names() {
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            "Program log: Instruction: Swap".to_string(),
            "Program 11111111111111111111111111111111 invoke [2]".to_string(),
            "Program 11111111111111111111111111111111 success".to_string(),
            format!("Program {amm_id} consumed 4079 of 200000 compute units"),
            format!("Program {amm_id} success"),
        ];
        // The CPI is System::Transfer (4-byte LE tag 2). Inner-frame data in DFS
        // order: root (none decodable) then the System child's transfer data.
        let inner_data = vec![vec![], vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]];
        let admin = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(admin, "admin")
            .with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(
            &logs,
            &inner_data,
            &aliases,
            vec![vec![admin]],
            vec![admin],
            Mode::Plain,
        );
        assert!(
            out.contains("admin ->> amm: Swap (4079cu)"),
            "expected top-level arrow with CU; got:\n{out}"
        );
        assert!(
            out.contains("amm ->> System: Transfer"),
            "expected decoded System::Transfer CPI arrow; got:\n{out}"
        );
        // System frame has no CU line in the log, so no `(Ncu)` suffix.
        assert!(
            !out.contains("Transfer ("),
            "System::Transfer should not carry a CU suffix; got:\n{out}"
        );
    }

    #[test]
    fn failed_frame_emits_note_over_with_error() {
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        // Anchor's custom error path: the program logs an error number and
        // then the runtime reports failure. The upstream parser surfaces
        // the message string on the failed frame.
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            "Program log: Instruction: Swap".to_string(),
            "Program log: AnchorError thrown in programs/amm/src/instructions/swap.rs:42. Error Code: PoolLocked. Error Number: 6000. Error Message: PoolLocked.".to_string(),
            format!("Program {amm_id} consumed 1000 of 200000 compute units"),
            format!("Program {amm_id} failed: custom program error: 0x1770"),
        ];
        let admin = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(admin, "admin")
            .with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(
            &logs,
            &[],
            &aliases,
            vec![vec![admin]],
            vec![admin],
            Mode::Plain,
        );
        assert!(
            out.contains("admin ->> amm: Swap (1000cu)"),
            "expected swap arrow; got:\n{out}"
        );
        assert!(
            out.contains("note over amm: ✗"),
            "expected note over with failure marker; got:\n{out}"
        );
    }

    #[test]
    fn failed_frame_with_children_renders_note_after_children() {
        // Regression test for the chronology fix: Solana logs CPIs
        // before the parent's post-CPI check fires, so the error note
        // must come AFTER the children's call arrows. Mirrors the
        // tree::render fix from commit e959b2d.
        let escrow_id = "H1GjRKWSauAuupurDtGiY5uvhLBtUngNhvrSBs75rH9o";
        let logs = vec![
            format!("Program {escrow_id} invoke [1]"),
            "Program log: Instruction: Take".to_string(),
            "Program 11111111111111111111111111111111 invoke [2]".to_string(),
            "Program 11111111111111111111111111111111 success".to_string(),
            "Program log: AnchorError thrown ... Error Code: EscrowExpired. Error Number: 6000."
                .to_string(),
            format!("Program {escrow_id} consumed 5000 of 200000 compute units"),
            format!("Program {escrow_id} failed: custom program error: 0x1770"),
        ];
        // The inner ix is System::CreateAccount (tag 0). DFS data: root, child.
        let inner_data = vec![vec![], vec![0, 0, 0, 0]];
        let taker = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(taker, "Taker")
            .with(Pubkey::from_str(escrow_id).unwrap(), "escrow");
        let out = render_with(
            &logs,
            &inner_data,
            &aliases,
            vec![vec![taker]],
            vec![taker],
            Mode::Plain,
        );

        // Find the byte offsets of the three lines that must be in
        // order: parent call, child call, note over.
        let parent_call = out
            .find("Taker ->> escrow: Take")
            .expect("parent call missing");
        let child_call = out
            .find("escrow ->> System: CreateAccount")
            .expect("child call missing");
        let note_over = out.find("note over escrow: ✗").expect("error note missing");

        assert!(
            parent_call < child_call,
            "parent call must precede child call; got:\n{out}"
        );
        assert!(
            child_call < note_over,
            "child call must precede error note; got:\n{out}"
        );
    }

    #[test]
    fn multi_ix_tx_keeps_distinct_per_root_signers() {
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            format!("Program {amm_id} success"),
            format!("Program {amm_id} invoke [1]"),
            format!("Program {amm_id} success"),
        ];
        let alice = Pubkey::new_unique();
        let bob = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(alice, "alice")
            .with(bob, "bob")
            .with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(
            &logs,
            &[],
            &aliases,
            vec![vec![alice], vec![bob]],
            vec![alice, bob],
            Mode::Plain,
        );
        assert!(
            out.contains("participant alice"),
            "expected alice participant; got:\n{out}"
        );
        assert!(
            out.contains("participant bob"),
            "expected bob participant; got:\n{out}"
        );
        assert!(
            out.contains("alice ->> amm"),
            "expected alice's arrow; got:\n{out}"
        );
        assert!(
            out.contains("bob ->> amm"),
            "expected bob's arrow; got:\n{out}"
        );
    }

    #[test]
    fn unaliased_pubkey_uses_safe_id_with_display_alias() {
        // No alias registered for the program. The pubkey's base58
        // truncates to `<8>…<4>`, which contains the unicode ellipsis;
        // the emitter must hide the ellipsis from the Mermaid id and
        // surface the readable form via `participant Id as "display"`.
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            format!("Program {amm_id} success"),
        ];
        // Only well-known aliases attached; the amm id stays unaliased.
        let aliases = crate::aliases::Aliases::with_well_known();
        let out = render_with(&logs, &[], &aliases, vec![], vec![], Mode::Plain);
        // Display name "CYbYnHW7…2yf5" appears verbatim in an `as "..."`
        // clause; Mermaid id replaces the `…` with `_`.
        assert!(
            out.contains("participant CYbYnHW7_2yf5 as \"CYbYnHW7…2yf5\""),
            "expected sanitised id + readable display; got:\n{out}"
        );
        assert!(
            out.contains("->> CYbYnHW7_2yf5: "),
            "expected arrow target to use sanitised id; got:\n{out}"
        );
    }

    #[test]
    fn lifelines_emits_activate_then_deactivate_with_cu_on_return() {
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            "Program log: Instruction: Initialize".to_string(),
            format!("Program {amm_id} consumed 4079 of 200000 compute units"),
            format!("Program {amm_id} success"),
        ];
        let admin = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(admin, "admin")
            .with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(
            &logs,
            &[],
            &aliases,
            vec![vec![admin]],
            vec![admin],
            Mode::Lifelines,
        );
        let expected = "\
```mermaid
sequenceDiagram
    autonumber
    participant admin
    participant amm
    admin ->>+ amm: Initialize
    amm -->>- admin: ok (4079cu)
```
";
        assert_eq!(out, expected);
    }

    #[test]
    fn lifelines_nests_children_inside_parent_activation() {
        // The proof: in Lifelines mode, every child's activate+return
        // pair must be sandwiched between the parent's CallActivate and
        // the parent's Return. Order: parent-activate, child-activate,
        // child-return, parent-return.
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            "Program log: Instruction: Swap".to_string(),
            "Program 11111111111111111111111111111111 invoke [2]".to_string(),
            "Program 11111111111111111111111111111111 success".to_string(),
            format!("Program {amm_id} consumed 4079 of 200000 compute units"),
            format!("Program {amm_id} success"),
        ];
        let inner_data = vec![vec![], vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]];
        let admin = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(admin, "admin")
            .with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(
            &logs,
            &inner_data,
            &aliases,
            vec![vec![admin]],
            vec![admin],
            Mode::Lifelines,
        );

        let parent_activate = out
            .find("admin ->>+ amm: Swap")
            .expect("parent activate missing");
        let child_activate = out
            .find("amm ->>+ System: Transfer")
            .expect("child activate missing");
        let child_return = out
            .find("System -->>- amm: ok")
            .expect("child return missing");
        let parent_return = out
            .find("amm -->>- admin: ok (4079cu)")
            .expect("parent return missing");

        assert!(
            parent_activate < child_activate,
            "parent activate must precede child activate; got:\n{out}"
        );
        assert!(
            child_activate < child_return,
            "child activate must precede child return; got:\n{out}"
        );
        assert!(
            child_return < parent_return,
            "child return must precede parent return; got:\n{out}"
        );
    }

    #[test]
    fn lifelines_failure_uses_lost_message_arrow_with_error_label() {
        // Same fixture as failed_frame_with_children_renders_note_after_children,
        // but in Lifelines mode the failure returns with `--x` (Mermaid's
        // "lost message" arrow) carrying the error label, instead of
        // emitting a separate `note over`.
        let escrow_id = "H1GjRKWSauAuupurDtGiY5uvhLBtUngNhvrSBs75rH9o";
        let logs = vec![
            format!("Program {escrow_id} invoke [1]"),
            "Program log: Instruction: Take".to_string(),
            "Program 11111111111111111111111111111111 invoke [2]".to_string(),
            "Program 11111111111111111111111111111111 success".to_string(),
            format!("Program {escrow_id} consumed 5000 of 200000 compute units"),
            format!("Program {escrow_id} failed: custom program error: 0x1770"),
        ];
        let inner_data = vec![vec![], vec![0, 0, 0, 0]];
        let taker = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(taker, "Taker")
            .with(Pubkey::from_str(escrow_id).unwrap(), "escrow");
        let out = render_with(
            &logs,
            &inner_data,
            &aliases,
            vec![vec![taker]],
            vec![taker],
            Mode::Lifelines,
        );

        assert!(
            !out.contains("note over"),
            "lifelines mode should not use `note over` for failures; got:\n{out}"
        );
        assert!(
            out.contains("System -->>- escrow: ok"),
            "successful child should return with -->>-; got:\n{out}"
        );
        assert!(
            out.contains("escrow --x Taker: ✗ custom program error: 0x1770 (5000cu)"),
            "failed parent should return with --x carrying the error; got:\n{out}"
        );

        // Child return must precede parent's error return.
        let child_return = out
            .find("System -->>- escrow: ok")
            .expect("child return missing");
        let parent_error_return = out
            .find("escrow --x Taker:")
            .expect("parent error return missing");
        assert!(
            child_return < parent_error_return,
            "child return must precede parent error return; got:\n{out}"
        );
    }

    #[test]
    fn plain_mode_wraps_only_the_error_note_in_red_rect() {
        // The rect tightly wraps just the `note over` line, not the
        // preceding call. Surrounding successful work stays untinted.
        let escrow_id = "H1GjRKWSauAuupurDtGiY5uvhLBtUngNhvrSBs75rH9o";
        let logs = vec![
            format!("Program {escrow_id} invoke [1]"),
            "Program log: Instruction: Take".to_string(),
            format!("Program {escrow_id} consumed 1000 of 200000 compute units"),
            format!("Program {escrow_id} failed: custom program error: 0x1770"),
        ];
        let taker = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(taker, "Taker")
            .with(Pubkey::from_str(escrow_id).unwrap(), "escrow");
        let out = render_with(
            &logs,
            &[],
            &aliases,
            vec![vec![taker]],
            vec![taker],
            Mode::Plain,
        );

        let call = out.find("Taker ->> escrow: Take").expect("call missing");
        let rect_open = out
            .find("rect rgb(255, 220, 220)")
            .expect("rect open missing");
        let note = out.find("note over escrow: ✗").expect("note missing");
        let rect_close = out.find("    end\n").expect("rect close missing");

        // Call comes first, THEN the rect wraps only the note. The
        // call line is outside the rect so it does not get tinted.
        assert!(
            call < rect_open,
            "rect opens AFTER the call (failure-marker-only wrap); got:\n{out}"
        );
        assert!(rect_open < note, "rect opens before the note; got:\n{out}");
        assert!(note < rect_close, "rect closes after the note; got:\n{out}");
    }

    #[test]
    fn lifelines_mode_wraps_only_the_error_return_in_red_rect() {
        // Lifelines variant of the same tight-wrap behavior. Children's
        // ok-returns stay untinted; only the `--x` failure return is
        // inside the rect.
        let escrow_id = "H1GjRKWSauAuupurDtGiY5uvhLBtUngNhvrSBs75rH9o";
        let logs = vec![
            format!("Program {escrow_id} invoke [1]"),
            "Program log: Instruction: Take".to_string(),
            "Program 11111111111111111111111111111111 invoke [2]".to_string(),
            "Program 11111111111111111111111111111111 success".to_string(),
            format!("Program {escrow_id} consumed 5000 of 200000 compute units"),
            format!("Program {escrow_id} failed: custom program error: 0x1770"),
        ];
        let inner_data = vec![vec![], vec![0, 0, 0, 0]];
        let taker = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(taker, "Taker")
            .with(Pubkey::from_str(escrow_id).unwrap(), "escrow");
        let out = render_with(
            &logs,
            &inner_data,
            &aliases,
            vec![vec![taker]],
            vec![taker],
            Mode::Lifelines,
        );

        let call_activate = out
            .find("Taker ->>+ escrow: Take")
            .expect("CallActivate missing");
        let child_return = out
            .find("System -->>- escrow: ok")
            .expect("child return missing");
        let rect_open = out
            .find("rect rgb(255, 220, 220)")
            .expect("rect open missing");
        let error_return = out
            .find("escrow --x Taker: ✗")
            .expect("error return missing");
        let rect_close = out.find("    end\n").expect("rect close missing");

        // Order: CallActivate < child_return < rect_open < error_return
        // < rect_close. The successful child return is OUTSIDE the
        // rect, which is the whole point of the tight wrap.
        assert!(
            call_activate < child_return,
            "CallActivate < child return; got:\n{out}"
        );
        assert!(
            child_return < rect_open,
            "child return is OUTSIDE the rect (no false-failure tint); got:\n{out}"
        );
        assert!(
            rect_open < error_return,
            "rect opens before error return; got:\n{out}"
        );
        assert!(
            error_return < rect_close,
            "rect closes after error return; got:\n{out}"
        );

        // Single failed frame -> single rect pair.
        assert_eq!(
            out.matches("rect rgb").count(),
            1,
            "expected exactly one rect open; got:\n{out}"
        );
        assert_eq!(
            out.matches("    end\n").count(),
            1,
            "expected exactly one rect close; got:\n{out}"
        );
    }

    #[test]
    fn event_lines_always_render_to_initiator() {
        // `Program data: <base64>` lines from the parser become FrameLog::Data
        // entries; render must emit them as dashed arrows back to the tx
        // initiator regardless of include_logs.
        let escrow_id = "H1GjRKWSauAuupurDtGiY5uvhLBtUngNhvrSBs75rH9o";
        let logs = vec![
            format!("Program {escrow_id} invoke [1]"),
            "Program log: Instruction: Make".to_string(),
            "Program data: AAAAAAAAAAA=".to_string(),
            format!("Program {escrow_id} consumed 5000 of 200000 compute units"),
            format!("Program {escrow_id} success"),
        ];
        let maker = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(maker, "Maker")
            .with(Pubkey::from_str(escrow_id).unwrap(), "escrow");

        // include_logs = false: event still renders.
        let out = render_with_logs(
            &logs,
            &[],
            &aliases,
            vec![vec![maker]],
            vec![maker],
            Mode::Plain,
            false,
        );
        assert!(
            out.contains("escrow -->> Maker: 🔔 event: AAAAAAAAAAA="),
            "expected event arrow even with logs off; got:\n{out}"
        );

        // include_logs = true: event still renders the same way.
        let out2 = render_with_logs(
            &logs,
            &[],
            &aliases,
            vec![vec![maker]],
            vec![maker],
            Mode::Plain,
            true,
        );
        assert!(
            out2.contains("escrow -->> Maker: 🔔 event: AAAAAAAAAAA="),
            "expected event arrow with logs on; got:\n{out2}"
        );
    }

    #[test]
    fn a_registered_event_renders_as_a_note_with_aliased_fields() {
        use base64::{engine::general_purpose, Engine as _};
        use std::sync::Arc;

        let maker = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();

        // A decoder whose formatted fields embed the maker's base58 key, so we
        // can prove the renderer substitutes it for the alias.
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
            operands: vec![],
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
                signers: vec![maker],
                frame,
            }],
            tx_signers: vec![maker],
            error: None,
            compute_units: 5000,
            fee: 0,
            events: reg,
        };
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(maker, "maker")
            .with(escrow, "escrow");

        let out = MermaidRenderer {
            mode: Mode::Plain,
            include_logs: false,
        }
        .render(&model, &aliases);

        // A `note over <emitter>`, not an arrow, carrying the decoded name...
        assert!(
            out.contains("note over escrow: 🔔 Transfer"),
            "expected a decoded event note; got:\n{out}"
        );
        // ...with the field pubkey substituted to its alias, not raw base58.
        assert!(
            out.contains("from: maker"),
            "alias not substituted; got:\n{out}"
        );
        assert!(
            !out.contains(&maker.to_string()),
            "raw base58 leaked into the note; got:\n{out}"
        );
    }

    #[test]
    fn log_lines_only_render_when_include_logs_is_true() {
        let escrow_id = "H1GjRKWSauAuupurDtGiY5uvhLBtUngNhvrSBs75rH9o";
        let logs = vec![
            format!("Program {escrow_id} invoke [1]"),
            "Program log: Instruction: Make".to_string(),
            "Program log: Funded vault with 1000 mint_a".to_string(),
            format!("Program {escrow_id} consumed 5000 of 200000 compute units"),
            format!("Program {escrow_id} success"),
        ];
        let maker = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(maker, "Maker")
            .with(Pubkey::from_str(escrow_id).unwrap(), "escrow");

        // include_logs = false: no log arrow.
        let out_off = render_with_logs(
            &logs,
            &[],
            &aliases,
            vec![vec![maker]],
            vec![maker],
            Mode::Plain,
            false,
        );
        assert!(
            !out_off.contains("💬 log:"),
            "log arrow should be absent when include_logs=false; got:\n{out_off}"
        );

        // include_logs = true: log arrow appears, pointing back to Maker.
        let out_on = render_with_logs(
            &logs,
            &[],
            &aliases,
            vec![vec![maker]],
            vec![maker],
            Mode::Plain,
            true,
        );
        assert!(
            out_on.contains("escrow -->> Maker: 💬 log: Funded vault with 1000 mint_a"),
            "expected log arrow when include_logs=true; got:\n{out_on}"
        );
    }

    #[test]
    fn instruction_dispatcher_announcement_is_not_rendered_as_a_log() {
        // The parser treats `Program log: Instruction: <Name>` as the
        // dispatcher announcement and strips prior Msg entries from the
        // frame's logs. We don't want to render the `Instruction:` line
        // itself either; it would duplicate the call label.
        let escrow_id = "H1GjRKWSauAuupurDtGiY5uvhLBtUngNhvrSBs75rH9o";
        let logs = vec![
            format!("Program {escrow_id} invoke [1]"),
            "Program log: Instruction: Make".to_string(),
            format!("Program {escrow_id} consumed 5000 of 200000 compute units"),
            format!("Program {escrow_id} success"),
        ];
        let maker = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(maker, "Maker")
            .with(Pubkey::from_str(escrow_id).unwrap(), "escrow");

        // With include_logs=true: still no log arrow, because the parser
        // never put the `Instruction:` line into frame.logs in the
        // first place (it consumed it for instruction_name decoding).
        let out = render_with_logs(
            &logs,
            &[],
            &aliases,
            vec![vec![maker]],
            vec![maker],
            Mode::Plain,
            true,
        );
        assert!(
            !out.contains("💬 log:"),
            "Instruction: line must not surface as a log arrow; got:\n{out}"
        );
    }

    #[test]
    fn long_event_payloads_truncate_with_ellipsis() {
        let escrow_id = "H1GjRKWSauAuupurDtGiY5uvhLBtUngNhvrSBs75rH9o";
        let long_payload = "A".repeat(EVENT_LABEL_MAX + 30);
        let logs = vec![
            format!("Program {escrow_id} invoke [1]"),
            "Program log: Instruction: Make".to_string(),
            format!("Program data: {long_payload}"),
            format!("Program {escrow_id} success"),
        ];
        let maker = Pubkey::new_unique();
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(maker, "Maker")
            .with(Pubkey::from_str(escrow_id).unwrap(), "escrow");
        let out = render_with_logs(
            &logs,
            &[],
            &aliases,
            vec![vec![maker]],
            vec![maker],
            Mode::Plain,
            false,
        );
        let expected_prefix = "A".repeat(EVENT_LABEL_MAX);
        assert!(
            out.contains(&format!("🔔 event: {expected_prefix}…")),
            "expected truncation at {EVENT_LABEL_MAX} chars; got:\n{out}"
        );
        // The full payload (longer) must NOT appear in the output.
        assert!(
            !out.contains(&long_payload),
            "untruncated payload leaked through; got:\n{out}"
        );
    }

    #[test]
    fn failure_label_prefers_anchor_error_name_over_runtime_message() {
        // The Anchor log line carries `Error Code: EscrowExpired`; the
        // runtime reports `custom program error: 0x1770` separately.
        // The mermaid label should use the friendly name in BOTH the
        // Plain `note over` line and the Lifelines `--x` line.
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
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(taker, "Taker")
            .with(Pubkey::from_str(escrow_id).unwrap(), "escrow");

        let plain = render_with(
            &logs,
            &[],
            &aliases,
            vec![vec![taker]],
            vec![taker],
            Mode::Plain,
        );
        assert!(
            plain.contains("note over escrow: ✗ EscrowExpired"),
            "Plain mode should use the Anchor name in note over; got:\n{plain}"
        );
        assert!(
            !plain.contains("custom program error: 0x1770"),
            "raw runtime message should be suppressed when name available; got:\n{plain}"
        );

        let lifelines = render_with(
            &logs,
            &[],
            &aliases,
            vec![vec![taker]],
            vec![taker],
            Mode::Lifelines,
        );
        assert!(
            lifelines.contains("escrow --x Taker: ✗ EscrowExpired (5000cu)"),
            "Lifelines mode should use the Anchor name in --x label; got:\n{lifelines}"
        );
        assert!(
            !lifelines.contains("custom program error: 0x1770"),
            "raw runtime message should be suppressed when name available; got:\n{lifelines}"
        );
    }

    #[test]
    fn no_signers_falls_back_to_user_placeholder() {
        let amm_id = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {amm_id} invoke [1]"),
            format!("Program {amm_id} success"),
        ];
        let aliases =
            crate::aliases::Aliases::with_well_known().with(Pubkey::from_str(amm_id).unwrap(), "amm");
        let out = render_with(&logs, &[], &aliases, vec![], vec![], Mode::Plain);
        assert!(
            out.contains("participant User"),
            "expected User placeholder participant; got:\n{out}"
        );
        assert!(
            out.contains("User ->> amm"),
            "expected User-sourced arrow; got:\n{out}"
        );
    }

    #[test]
    fn undecoded_instruction_renders_safe_label_not_bare_question_mark() {
        // A bare `?` mermaid message (undecoded name, no cu suffix to pad it)
        // fails to parse on GitHub and drops the whole diagram to raw text.
        // The label must be a non-empty word instead. Reported via dogfood
        // on a transaction carrying a ComputeBudget instruction.
        let prog = "CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5";
        let logs = vec![
            format!("Program {prog} invoke [1]"),
            format!("Program {prog} success"),
        ];
        let aliases = crate::aliases::Aliases::with_well_known()
            .with(Pubkey::from_str(prog).unwrap(), "prog");
        let out = render_with(&logs, &[], &aliases, vec![], vec![], Mode::Plain);
        assert!(
            out.contains(": unnamed"),
            "undecoded label should read `unnamed`; got:\n{out}"
        );
        assert!(
            !out.contains(": ?"),
            "a bare `: ?` mermaid label breaks GitHub rendering; got:\n{out}"
        );
    }
}
