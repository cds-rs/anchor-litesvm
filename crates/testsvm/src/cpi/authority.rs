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
    super::graph::{render_flowchart, upsert, NodeStyle, Shape},
    super::model::CpiModel,
    super::renderer::{LegendCollector, Renderer},
    indexmap::{IndexMap, IndexSet},
};

/// The authority-graph renderer. A unit struct: the graph takes no options
/// today (a future variant might filter by program or collapse CPI depth).
pub(super) struct AuthorityGraph;

/// Node roles, in precedence order (rank): a pubkey invoked as a program
/// anywhere is a `program`; otherwise one that ever signs is a `signer`;
/// otherwise one that is ever writable is a `writable` account.
const SIGNER: NodeStyle = NodeStyle {
    shape: Shape::Stadium,
    class: "signer",
    rank: 1,
};
const PROGRAM: NodeStyle = NodeStyle {
    shape: Shape::Rect,
    class: "program",
    rank: 2,
};
const WRITABLE: NodeStyle = NodeStyle {
    shape: Shape::Cylinder,
    class: "writable",
    rank: 0,
};

impl Renderer for AuthorityGraph {
    fn render(&self, model: &CpiModel, aliases: &super::aliases::Aliases) -> String {
        if model.roots.is_empty() {
            return String::new();
        }
        let mut collector = LegendCollector::new(aliases, &model.events);
        // name -> style; first-seen insertion order kept for stable output.
        let mut nodes: IndexMap<String, NodeStyle> = IndexMap::new();
        let mut signs: IndexSet<(String, String)> = IndexSet::new(); // (signer, program)
        let mut writes: IndexSet<(String, String)> = IndexSet::new(); // (program, account)

        for frame in model.frames() {
            let program = collector.render_pubkey(&frame.program);
            upsert(&mut nodes, program.clone(), PROGRAM);
            for acct in &frame.accounts {
                let name = collector.render_pubkey(&acct.pubkey);
                // A frame's own program id can appear in its account list; skip
                // the self-edge it would produce.
                if name == program {
                    continue;
                }
                if acct.is_signer {
                    upsert(&mut nodes, name.clone(), SIGNER);
                    signs.insert((name, program.clone()));
                } else if acct.is_writable {
                    upsert(&mut nodes, name.clone(), WRITABLE);
                    writes.insert((program.clone(), name));
                }
                // Read-only non-signer accounts are intentionally dropped.
            }
        }

        render_flowchart(
            &[
                ("signer", "fill:#d4edda,stroke:#28a745"),
                ("program", "fill:#cce5ff,stroke:#007bff"),
                ("writable", "fill:#fff3cd,stroke:#ffc107"),
            ],
            &nodes,
            &[("signs", &signs), ("writes", &writes)],
        )
    }
}
