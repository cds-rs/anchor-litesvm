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
    super::graph::{render_flowchart, upsert, NodeStyle, Shape},
    super::model::CpiModel,
    super::renderer::{LegendCollector, Renderer},
    indexmap::{IndexMap, IndexSet},
};

/// The ownership-graph renderer.
pub(super) struct OwnershipGraph;

/// `Owner` outranks `Account` (rank), so a program that both owns accounts and
/// is itself written is drawn as an owner.
const OWNER: NodeStyle = NodeStyle {
    shape: Shape::Rect,
    class: "owner",
    rank: 1,
};
const ACCOUNT: NodeStyle = NodeStyle {
    shape: Shape::Cylinder,
    class: "account",
    rank: 0,
};

impl Renderer for OwnershipGraph {
    fn render(&self, model: &CpiModel, aliases: &super::aliases::Aliases) -> String {
        if model.roots.is_empty() {
            return String::new();
        }
        let mut collector = LegendCollector::new(aliases, &model.events);
        let mut nodes: IndexMap<String, NodeStyle> = IndexMap::new();
        let mut owns: IndexSet<(String, String)> = IndexSet::new(); // (owner, account)

        for frame in model.frames() {
            for acct in &frame.accounts {
                if !acct.is_writable {
                    continue;
                }
                let Some(owner_pk) = acct.owner else {
                    continue;
                };
                let account_name = collector.render_pubkey(&acct.pubkey);
                let owner_name = collector.render_pubkey(&owner_pk);
                // An account owned by itself (no real Solana account is) would
                // draw a self-loop; skip defensively.
                if owner_name == account_name {
                    continue;
                }
                upsert(&mut nodes, owner_name.clone(), OWNER);
                upsert(&mut nodes, account_name.clone(), ACCOUNT);
                owns.insert((owner_name, account_name));
            }
        }

        // Nothing to show if owners weren't filled (no svm lookup ran) or no
        // written account had a resolvable owner. Emit an empty string rather
        // than a bare `flowchart` with no edges.
        if owns.is_empty() {
            return String::new();
        }

        render_flowchart(
            &[
                ("owner", "fill:#cce5ff,stroke:#007bff"),
                ("account", "fill:#fff3cd,stroke:#ffc107"),
            ],
            &nodes,
            &[("owns", &owns)],
        )
    }
}

#[cfg(test)]
mod tests;
