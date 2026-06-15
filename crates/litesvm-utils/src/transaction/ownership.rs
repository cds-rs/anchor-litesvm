//! Ownership graph: a Mermaid `flowchart` of which program owns each account
//! the transaction wrote.
//!
//! Sibling to the authority graph, over the same model. Where authority asks
//! "who signed, and which program wrote what", ownership asks "and who *owns*
//! the accounts that were written":
//!
//! ```text
//! owner-program --owns--> account
//! ```
//!
//! The owner frequently differs from the writer: an Escrow program writes a
//! token account that the Token program owns (reached by CPI), or a wallet
//! the System program owns. Surfacing that gap is the whole point.
//!
//! Scope: writable accounts with a known owner. Read-only accounts are
//! dropped (ownership of a config/sysvar you only read is noise here), and an
//! account whose owner wasn't resolved is skipped.
//!
//! The owner is post-execution account state, so it is NOT in the CpiModel by
//! default; [`super::model::fill_owners`] populates `AccountRef.owner` from an
//! svm lookup before this renders. Once litesvm carries owner metadata on the
//! frame, that fill step drops and this stays a pure model consumer.

use {
    super::model::{CpiModel, ResolvedFrame},
    super::renderer::{node_label, LegendCollector, NodeIds, Renderer},
    indexmap::{IndexMap, IndexSet},
    std::fmt::Write,
};

/// The ownership-graph renderer.
pub(super) struct OwnershipGraph;

/// A node's role. `Owner` outranks `Account` so a program that both owns
/// accounts and is itself written is drawn as an owner.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Account,
    Owner,
}

impl Renderer for OwnershipGraph {
    fn render(&self, model: &CpiModel, aliases: &super::aliases::Aliases) -> String {
        if model.roots.is_empty() {
            return String::new();
        }
        let mut collector = LegendCollector::new(aliases, &model.events);
        let mut nodes: IndexMap<String, Role> = IndexMap::new();
        let mut owns: IndexSet<(String, String)> = IndexSet::new(); // (owner, account)

        for root in &model.roots {
            collect(&root.frame, &mut collector, &mut nodes, &mut owns);
        }

        // Nothing to show if owners weren't filled (no svm lookup ran) or no
        // written account had a resolvable owner. Emit an empty string rather
        // than a bare `flowchart` with no edges.
        if owns.is_empty() {
            return String::new();
        }

        let mut out = String::new();
        out.push_str("```mermaid\n");
        out.push_str("flowchart LR\n");
        out.push_str("    classDef owner fill:#cce5ff,stroke:#007bff;\n");
        out.push_str("    classDef account fill:#fff3cd,stroke:#ffc107;\n");
        let mut ids = NodeIds::new();
        for (name, role) in &nodes {
            let id = ids.id(name);
            let lbl = node_label(name);
            let _ = match role {
                Role::Owner => writeln!(out, "    {id}[{lbl}]:::owner"),
                Role::Account => writeln!(out, "    {id}[({lbl})]:::account"),
            };
        }
        for (owner, account) in &owns {
            let (oid, aid) = (ids.id(owner), ids.id(account));
            let _ = writeln!(out, "    {oid} -->|owns| {aid}");
        }
        out.push_str("```\n");
        out
    }
}

/// Walk a frame and its CPI children, recording `owner --owns--> account`
/// edges for writable accounts whose owner is known.
fn collect(
    frame: &ResolvedFrame,
    collector: &mut LegendCollector<'_>,
    nodes: &mut IndexMap<String, Role>,
    owns: &mut IndexSet<(String, String)>,
) {
    for acct in &frame.accounts {
        if !acct.is_writable {
            continue;
        }
        let Some(owner_pk) = acct.owner else {
            continue;
        };
        let account_name = collector.render_pubkey(&acct.pubkey);
        let owner_name = collector.render_pubkey(&owner_pk);
        // An account owned by itself (no real Solana account is) would draw a
        // self-loop; skip defensively.
        if owner_name == account_name {
            continue;
        }
        upsert(nodes, owner_name.clone(), Role::Owner);
        upsert(nodes, account_name.clone(), Role::Account);
        owns.insert((owner_name, account_name));
    }
    for child in &frame.children {
        collect(child, collector, nodes, owns);
    }
}

/// Insert `name` at `role`, upgrading if already seen at a lower-precedence
/// role. Keeps first-seen position.
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
