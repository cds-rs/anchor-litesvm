//! Structured rendering of transaction logs as an invocation tree.
//!
//! Parses Solana's flat `Program X invoke [n]` / `consumed` / `success|failed`
//! log stream into a tree of [`TreeNode`]s and renders it with box-drawing
//! characters.

use std::fmt::Write;

const TREE_BRANCH: &str = "├── ";
const TREE_END: &str = "└── ";
const TREE_CONT: &str = "│   ";
const TREE_EMPTY: &str = "    ";

pub(super) struct TreeNode {
    pub(super) info: String,
    pub(super) outcome: Option<Outcome>,
    pub(super) compute_units: Option<u64>,
    pub(super) diagnostics: Vec<String>,
    pub(super) children: Vec<TreeNode>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Success,
    Failed { message: Option<String> },
}

/// Render the invocation tree for a transaction's logs.
///
/// Returns an empty string if no `Program ... invoke [n]` lines were found
/// (e.g. a malformed or empty log stream); otherwise returns the tree body
/// prefixed by `"Transaction\n"`.
pub(super) fn render(logs: &[String]) -> String {
    let roots = parse(logs);
    if roots.is_empty() {
        return String::new();
    }
    let mut out = String::from("Transaction\n");
    let last = roots.len() - 1;
    for (i, root) in roots.iter().enumerate() {
        render_node(root, "", i == last, 1, &mut out);
    }
    out
}

fn render_node(
    node: &TreeNode,
    ancestor_prefix: &str,
    is_last: bool,
    depth: usize,
    out: &mut String,
) {
    let connector = if is_last { TREE_END } else { TREE_BRANCH };
    let _ = write!(out, "{}{}{} [{}]", ancestor_prefix, connector, node.info, depth);

    if let Some(outcome) = &node.outcome {
        out.push(' ');
        out.push_str(match outcome {
            Outcome::Success => "✓",
            Outcome::Failed { .. } => "✗",
        });
    }
    if let Some(cu) = node.compute_units {
        let _ = write!(out, " {}cu", cu);
    }
    out.push('\n');

    // Prefix used for everything rendered underneath this node: error line,
    // diagnostics, and child subtrees. If this node is its parent's last
    // child, the column above it is blank; otherwise it carries `│`.
    let descendant_prefix = format!(
        "{}{}",
        ancestor_prefix,
        if is_last { TREE_EMPTY } else { TREE_CONT }
    );

    // Error message renders as a phantom `└──` line under the node.
    if let Some(Outcome::Failed { message: Some(error) }) = &node.outcome {
        let _ = writeln!(out, "{}{}Error: {}", descendant_prefix, TREE_END, error);
    }

    // Diagnostics use plain depth-based indent (no parent vertical line).
    // This intentionally matches the prior visual; revisit if it's wrong.
    if !node.diagnostics.is_empty() {
        let base_indent = TREE_EMPTY.repeat(depth);
        let last_diag = node.diagnostics.len() - 1;
        for (i, diag) in node.diagnostics.iter().enumerate() {
            let connector = if i == last_diag { TREE_END } else { TREE_BRANCH };
            // Split diagnostic on period-space boundaries for readability
            let mut chunks = diag.split(". ");
            if let Some(first) = chunks.next() {
                let _ = writeln!(out, "{}{}{}", base_indent, connector, first);
            }
            for chunk in chunks {
                let _ = writeln!(out, "{}{} {}", base_indent, TREE_EMPTY, chunk);
            }
        }
    }

    let last_child = node.children.len().saturating_sub(1);
    for (i, child) in node.children.iter().enumerate() {
        render_node(child, &descendant_prefix, i == last_child, depth + 1, out);
    }
}

/// Parse the log stream into a forest of invocation trees.
///
/// Most transactions produce exactly one root (the outer instruction). A
/// transaction containing multiple top-level instructions produces one root
/// per instruction.
pub(super) fn parse(logs: &[String]) -> Vec<TreeNode> {
    let mut roots: Vec<TreeNode> = vec![];
    // In-progress nodes whose `success`/`failed` line we haven't seen yet.
    // Solana's strict invocation nesting means every consumed/success/failed
    // line refers to the top of this stack, regardless of program name.
    let mut stack: Vec<TreeNode> = vec![];
    let mut accumulated_logs: Vec<String> = vec![];

    for log in logs {
        // Skip "Program log: Instruction: X" and reset diagnostics
        if log.starts_with("Program log: Instruction: ") {
            accumulated_logs.clear();
            continue;
        }
        if let Some(diag) = log.strip_prefix("Program log: ") {
            accumulated_logs.push(diag.to_string());
        }

        match classify_log_line(log) {
            LogLine::Invoke(info) => {
                stack.push(TreeNode {
                    info,
                    outcome: None,
                    compute_units: None,
                    diagnostics: vec![],
                    children: vec![],
                });
                accumulated_logs.clear();
            }
            LogLine::Consumed(cu) => {
                if let Some(top) = stack.last_mut() {
                    top.compute_units = Some(cu);
                }
            }
            LogLine::Status(outcome) => {
                if let Some(mut node) = stack.pop() {
                    if matches!(outcome, Outcome::Failed { .. }) {
                        node.diagnostics = std::mem::take(&mut accumulated_logs);
                    }
                    node.outcome = Some(outcome);
                    attach(&mut stack, &mut roots, node);
                }
            }
            LogLine::Other => {}
        }
    }

    // Truncated logs: drain remaining in-progress nodes (innermost first)
    // and attach each upward. This preserves whatever partial structure exists.
    while let Some(node) = stack.pop() {
        attach(&mut stack, &mut roots, node);
    }

    roots
}

fn attach(stack: &mut [TreeNode], roots: &mut Vec<TreeNode>, node: TreeNode) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else {
        roots.push(node);
    }
}

enum LogLine {
    Invoke(String),
    Consumed(u64),
    Status(Outcome),
    Other,
}

/// Tokenize on single spaces and dispatch on the resulting slice. Each arm
/// states the exact shape of one Solana log line, so a program name that
/// happens to look like a keyword (`"consumed"`, `"of"`, `"success"`) slots
/// harmlessly into the name position and can't shadow the real keyword.
///
/// The Failed arm pulls the message substring from the raw log via
/// `splitn(4, ' ')` so internal whitespace in the message is preserved
/// exactly. (Splitting into tokens and re-joining with single spaces would
/// collapse `"foo  bar"` to `"foo bar"`.)
fn classify_log_line(log: &str) -> LogLine {
    let tokens: Vec<&str> = log.split(' ').collect();
    match tokens.as_slice() {
        ["Program", name, "invoke", bracket] if parse_depth_bracket(bracket).is_some() => {
            LogLine::Invoke((*name).to_string())
        }
        ["Program", _, "success"] => LogLine::Status(Outcome::Success),
        ["Program", _, "failed:", ..] => {
            let raw = log.splitn(4, ' ').nth(3).unwrap_or("").trim();
            let message = if raw.is_empty() {
                None
            } else {
                Some(raw.to_string())
            };
            LogLine::Status(Outcome::Failed { message })
        }
        ["Program", _, "consumed", cu, "of", _, "compute", "units"] => match cu.parse::<u64>() {
            Ok(n) => LogLine::Consumed(n),
            Err(_) => LogLine::Other,
        },
        _ => LogLine::Other,
    }
}

fn parse_depth_bracket(token: &str) -> Option<usize> {
    token.strip_prefix('[')?.strip_suffix(']')?.parse::<usize>().ok()
}

#[cfg(test)]
mod tests;
