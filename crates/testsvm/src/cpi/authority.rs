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

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::aliases::Aliases,
        crate::cpi::model::{AccountRef, CpiModel, Outcome, ResolvedFrame, Root},
        crate::cpi::renderer::Renderer,
        solana_pubkey::Pubkey,
    };

    fn account(pubkey: Pubkey, is_signer: bool, is_writable: bool) -> AccountRef {
        AccountRef {
            pubkey,
            is_signer,
            is_writable,
            owner: None,
        }
    }

    fn leaf(program: Pubkey, name: &str, accounts: Vec<AccountRef>) -> ResolvedFrame {
        ResolvedFrame {
            program,
            instruction_name: Some(name.to_string()),
            outcome: Outcome::Success,
            compute_units: None,
            accounts,
            logs: vec![],
            data: vec![],
            children: vec![],
        }
    }

    #[test]
    fn authority_graph_draws_signs_and_writes_and_omits_readonly() {
        let escrow = Pubkey::new_unique();
        let alice = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        let aliases = Aliases::default()
            .with(escrow, "Escrow")
            .with(alice, "alice")
            .with(vault, "vault")
            .with(mint, "mint");

        let model = CpiModel {
            header: None,
            roots: vec![Root {
                signers: vec![alice],
                frame: leaf(
                    escrow,
                    "Take",
                    vec![
                        // writable signer (fee payer) -> rendered as a signer
                        account(alice, true, true),
                        // writable non-signer -> a `writes` target
                        account(vault, false, true),
                        // read-only non-signer -> omitted entirely
                        account(mint, false, false),
                    ],
                ),
            }],
            tx_signers: vec![alice],
            error: None,
            compute_units: 0,
            fee: 0,
            events: Default::default(),
        };

        let out = AuthorityGraph.render(&model, &aliases);

        assert!(out.contains("flowchart LR"), "{out}");
        assert!(out.contains("alice([alice]):::signer"), "{out}");
        assert!(out.contains("Escrow[Escrow]:::program"), "{out}");
        assert!(out.contains("vault[(vault)]:::writable"), "{out}");
        assert!(out.contains("alice -->|signs| Escrow"), "{out}");
        assert!(out.contains("Escrow -->|writes| vault"), "{out}");
        assert!(
            !out.contains("mint"),
            "read-only non-signer account should be omitted; got:\n{out}"
        );
    }

    #[test]
    fn authority_graph_dedupes_edges_across_frames_and_descends_cpi() {
        let parent = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let admin = Pubkey::new_unique();
        let pool = Pubkey::new_unique();

        let aliases = Aliases::default()
            .with(parent, "Amm")
            .with(token, "Token")
            .with(admin, "admin")
            .with(pool, "pool");

        // Root: Amm signed by admin, writes pool. CPI child: Token, also touches
        // admin (signer) and pool (writable). The admin->Amm edge and the
        // Amm->pool edge must each appear once; Token gets its own edges.
        let child = leaf(
            token,
            "Transfer",
            vec![account(admin, true, false), account(pool, false, true)],
        );
        let mut root = leaf(
            parent,
            "Swap",
            vec![account(admin, true, true), account(pool, false, true)],
        );
        root.children = vec![child];

        let model = CpiModel {
            header: None,
            roots: vec![Root {
                signers: vec![admin],
                frame: root,
            }],
            tx_signers: vec![admin],
            error: None,
            compute_units: 0,
            fee: 0,
            events: Default::default(),
        };

        let out = AuthorityGraph.render(&model, &aliases);

        // Each unique edge appears exactly once.
        assert_eq!(out.matches("admin -->|signs| Amm").count(), 1, "{out}");
        assert_eq!(out.matches("Amm -->|writes| pool").count(), 1, "{out}");
        // The CPI child contributed its own edges.
        assert!(out.contains("admin -->|signs| Token"), "{out}");
        assert!(out.contains("Token -->|writes| pool"), "{out}");
        // pool is a writable account node, declared once.
        assert_eq!(out.matches("pool[(pool)]:::writable").count(), 1, "{out}");
    }
}
