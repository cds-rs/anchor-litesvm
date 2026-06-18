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
    let ix_name = frame.instruction_name.as_deref().unwrap_or("unnamed");
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
