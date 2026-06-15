use {
    super::*,
    crate::transaction::model::{AccountRef, CpiModel, Outcome, ResolvedFrame, Root},
    crate::transaction::renderer::Renderer,
    crate::Aliases,
    solana_program::pubkey::Pubkey,
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
