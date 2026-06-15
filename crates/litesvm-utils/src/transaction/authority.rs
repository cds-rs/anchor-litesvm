//! Authority graph: a Mermaid `flowchart` of who signs what and which
//! accounts each program writes.
//!
//! A [`Renderer`] adapter over the per-frame account roles the model
//! resolves ([`ResolvedFrame::accounts`]). Where the tree and sequence
//! renderers answer "what got called, in what order", this answers "who
//! authorized it, and what state moved":
//!
//! ```text
//! signer --signs--> program --writes--> account
//! ```
//!
//! Node roles, in precedence order: a pubkey invoked as a program anywhere
//! is a `program`; otherwise one that ever signs is a `signer`; otherwise
//! one that is ever writable is a `writable` account. Read-only non-signer
//! accounts (sysvars, config) are omitted, to keep the graph about authority
//! and state change rather than every account in the tx.
//!
//! Caveat, same as the tree's `signer=` annotation: "signs" is the
//! account-list relationship, not a claim about intent. Signer X appears as
//! a required signer in an instruction to program P; it does not assert X
//! "meant" to authorize that specific inner call. A writable signer (a fee
//! payer) renders as a signer; its writability is left implicit.

use {
    super::model::{CpiModel, ResolvedFrame},
    super::renderer::{node_label, LegendCollector, NodeIds, Renderer},
    indexmap::{IndexMap, IndexSet},
    std::fmt::Write,
};

/// The authority-graph renderer. A unit struct: the graph takes no options
/// today (a future variant might filter by program or collapse CPI depth).
pub(super) struct AuthorityGraph;

/// A node's role in the graph. Ordered by precedence: when a pubkey shows up
/// in more than one role across the tx, the highest wins (a program that also
/// appears as a writable account is drawn as a program).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Writable,
    Signer,
    Program,
}

impl Renderer for AuthorityGraph {
    fn render(&self, model: &CpiModel, aliases: &super::aliases::Aliases) -> String {
        if model.roots.is_empty() {
            return String::new();
        }
        let mut collector = LegendCollector::new(aliases, &model.events);
        // name -> role; first-seen insertion order kept for stable output.
        let mut nodes: IndexMap<String, Role> = IndexMap::new();
        let mut signs: IndexSet<(String, String)> = IndexSet::new(); // (signer, program)
        let mut writes: IndexSet<(String, String)> = IndexSet::new(); // (program, account)

        for root in &model.roots {
            collect(
                &root.frame,
                &mut collector,
                &mut nodes,
                &mut signs,
                &mut writes,
            );
        }

        let mut out = String::new();
        out.push_str("```mermaid\n");
        out.push_str("flowchart LR\n");
        out.push_str("    classDef signer fill:#d4edda,stroke:#28a745;\n");
        out.push_str("    classDef program fill:#cce5ff,stroke:#007bff;\n");
        out.push_str("    classDef writable fill:#fff3cd,stroke:#ffc107;\n");
        let mut ids = NodeIds::new();
        for (name, role) in &nodes {
            let id = ids.id(name);
            let lbl = node_label(name);
            let _ = match role {
                Role::Signer => writeln!(out, "    {id}([{lbl}]):::signer"),
                Role::Program => writeln!(out, "    {id}[{lbl}]:::program"),
                Role::Writable => writeln!(out, "    {id}[({lbl})]:::writable"),
            };
        }
        for (s, p) in &signs {
            let (sid, pid) = (ids.id(s), ids.id(p));
            let _ = writeln!(out, "    {sid} -->|signs| {pid}");
        }
        for (p, a) in &writes {
            let (pid, aid) = (ids.id(p), ids.id(a));
            let _ = writeln!(out, "    {pid} -->|writes| {aid}");
        }
        out.push_str("```\n");
        out
    }
}

/// Walk a frame and its CPI children, recording nodes and edges. Pubkeys are
/// resolved to friendly names through `collector` (so the graph lines up with
/// the names the other renderers show).
fn collect(
    frame: &ResolvedFrame,
    collector: &mut LegendCollector<'_>,
    nodes: &mut IndexMap<String, Role>,
    signs: &mut IndexSet<(String, String)>,
    writes: &mut IndexSet<(String, String)>,
) {
    let program = collector.render_pubkey(&frame.program);
    upsert(nodes, program.clone(), Role::Program);
    for acct in &frame.accounts {
        let name = collector.render_pubkey(&acct.pubkey);
        // A frame's own program id can appear in its account list; skip the
        // self-edge it would produce.
        if name == program {
            continue;
        }
        if acct.is_signer {
            upsert(nodes, name.clone(), Role::Signer);
            signs.insert((name, program.clone()));
        } else if acct.is_writable {
            upsert(nodes, name.clone(), Role::Writable);
            writes.insert((program.clone(), name));
        }
        // Read-only non-signer accounts are intentionally dropped.
    }
    for child in &frame.children {
        collect(child, collector, nodes, signs, writes);
    }
}

/// Insert `name` at `role`, upgrading if it was already seen at a
/// lower-precedence role. Keeps the node's first-seen position (an
/// `IndexMap` re-insert updates in place).
fn upsert(nodes: &mut IndexMap<String, Role>, name: String, role: Role) {
    match nodes.get(&name) {
        Some(&existing) if existing >= role => {}
        _ => {
            nodes.insert(name, role);
        }
    }
}

#[cfg(test)]
mod tests;
