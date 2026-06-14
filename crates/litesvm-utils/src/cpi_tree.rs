//! Fold a flat transaction log stream into a nested CPI tree.
//!
//! Ported from `litesvm::cpi_tree` (litesvm 0.12, unreleased). litesvm
//! 0.6 doesn't expose this module, but the parser is pure log-string
//! handling so it lifts verbatim. Kept here only as long as the LTS
//! branch is alive; when the project moves back to main, switch the
//! consumer (`transaction/tree.rs`) to `litesvm::cpi_tree` and delete
//! this file.
//!
//! Solana program logs nest cleanly: `invoke` opens a frame,
//! `success`/`failed:` closes it. That's the Dyck language of balanced
//! brackets; a pushdown automaton recognises it by construction. We
//! parse it in two layers:
//!
//! 1. **Per-line classifier (FSA).** `classify` maps each log line to
//!    one `LogLine` token. No memory across lines.
//! 2. **Stream parser (PDA).** One control state; the stack does all
//!    the work. `Invoke` pushes; `Status` pops and attaches to parent
//!    (or roots if the stack is empty); `Consumed` mutates the top;
//!    payload-only lines (`Msg`/`Data`/`Other`) append to the top
//!    frame's logs. EOF drains the stack as `Truncated` frames.
//!
//! Payload-only tokens can't alter the stack shape, so a stray runtime
//! diagnostic mid-CPI cannot corrupt the tree. Anything that does
//! affect the stack must match the exact tokenised shape (an
//! invoke-shaped line with a malformed `[k]` falls back to `Other`).

use solana_program::pubkey::Pubkey;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputeUnits {
    /// Cumulative: includes this frame's CPI children. Summing
    /// top-level frames gives the transaction total.
    pub consumed: u64,
    pub available_at_start: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpiFrame {
    pub program_id: Pubkey,
    pub outcome: CpiOutcome,
    pub compute_units: Option<ComputeUnits>,
    pub instruction_name: Option<String>,
    pub logs: Vec<FrameLog>,
    pub children: Vec<CpiFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameLog {
    Msg(String),
    Data(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpiOutcome {
    Success,
    Failed {
        message: Option<String>,
    },
    /// Frame whose closing line never arrived. Distinct from `Failed`:
    /// we lost sight of it; we don't know that it failed.
    Truncated,
}

pub fn cpi_tree(logs: &[String]) -> Vec<CpiFrame> {
    let mut roots: Vec<CpiFrame> = Vec::new();
    let mut stack: Vec<CpiFrame> = Vec::new();

    for log in logs {
        // The three string-prefix branches are kept literal (rather
        // than going through `classify`) because they carry payload
        // content that we want to thread into the current frame's
        // `logs` field, not into the parser's stack-manipulation path.
        if let Some(name) = log.strip_prefix("Program log: Instruction: ") {
            // The pre-handler `msg!` chatter that Anchor's dispatcher
            // emits isn't useful in the frame's logs; clear `Msg`
            // entries collected before we knew the instruction's name.
            // Keep `Data` (a pre-dispatch `emit!` is legitimate).
            if let Some(frame) = stack.last_mut() {
                frame
                    .logs
                    .retain(|entry| !matches!(entry, FrameLog::Msg(_)));
                if frame.instruction_name.is_none() {
                    frame.instruction_name = Some(name.to_string());
                }
            }
            continue;
        }
        if let Some(text) = log.strip_prefix("Program log: ") {
            if let Some(frame) = stack.last_mut() {
                frame.logs.push(FrameLog::Msg(text.to_string()));
            }
            continue;
        }
        if let Some(payload) = log.strip_prefix("Program data: ") {
            if let Some(frame) = stack.last_mut() {
                frame.logs.push(FrameLog::Data(payload.to_string()));
            }
            continue;
        }

        match classify(log) {
            LogLine::Invoke(program) => {
                let Ok(program_id) = Pubkey::from_str(&program) else {
                    continue;
                };
                stack.push(CpiFrame {
                    program_id,
                    outcome: CpiOutcome::Truncated,
                    compute_units: None,
                    instruction_name: None,
                    logs: Vec::new(),
                    children: Vec::new(),
                });
            }
            LogLine::Consumed(cu) => {
                if let Some(frame) = stack.last_mut() {
                    frame.compute_units = Some(cu);
                }
            }
            LogLine::Status(status) => {
                let Some(mut frame) = stack.pop() else {
                    continue;
                };
                frame.outcome = match status {
                    Status::Success => CpiOutcome::Success,
                    Status::Failed { message } => CpiOutcome::Failed { message },
                };
                push_into_parent_or_roots(frame, &mut stack, &mut roots);
            }
            LogLine::Other => {
                if let Some(frame) = stack.last_mut() {
                    frame.logs.push(FrameLog::Msg(log.clone()));
                }
            }
        }
    }

    // Truncation falls out of the EOF transition naturally: each frame
    // is created with `outcome: Truncated`, so we just drain.
    while let Some(frame) = stack.pop() {
        push_into_parent_or_roots(frame, &mut stack, &mut roots);
    }

    roots
}

fn push_into_parent_or_roots(
    frame: CpiFrame,
    stack: &mut [CpiFrame],
    roots: &mut Vec<CpiFrame>,
) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(frame);
    } else {
        roots.push(frame);
    }
}

enum LogLine {
    Invoke(String),
    Consumed(ComputeUnits),
    Status(Status),
    Other,
}

enum Status {
    Success,
    Failed { message: Option<String> },
}

fn classify(log: &str) -> LogLine {
    let tokens: Vec<&str> = log.split(' ').collect();
    match tokens.as_slice() {
        ["Program", _name, "invoke", bracket] if parse_depth_bracket(bracket).is_some() => {
            LogLine::Invoke(tokens[1].to_string())
        }
        ["Program", _, "success"] => LogLine::Status(Status::Success),
        ["Program", _, "failed:", ..] => {
            // splitn preserves internal whitespace in the message.
            let raw = log.splitn(4, ' ').nth(3).unwrap_or("").trim();
            let message = (!raw.is_empty()).then(|| raw.to_string());
            LogLine::Status(Status::Failed { message })
        }
        [
            "Program",
            _,
            "consumed",
            consumed,
            "of",
            available,
            "compute",
            "units",
        ] => match (consumed.parse::<u64>(), available.parse::<u64>()) {
            (Ok(consumed), Ok(available_at_start)) => LogLine::Consumed(ComputeUnits {
                consumed,
                available_at_start,
            }),
            _ => LogLine::Other,
        },
        _ => LogLine::Other,
    }
}

fn parse_depth_bracket(s: &str) -> Option<u32> {
    s.strip_prefix('[')?.strip_suffix(']')?.parse().ok()
}
