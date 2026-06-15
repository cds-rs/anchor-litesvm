use {
    super::*,
    crate::transaction::model::{AccountRef, CpiModel, Outcome, ResolvedFrame, Root},
    crate::transaction::renderer::Renderer,
    crate::Aliases,
    solana_program::pubkey::Pubkey,
};

fn account(pubkey: Pubkey, is_writable: bool, owner: Option<Pubkey>) -> AccountRef {
    AccountRef {
        pubkey,
        is_signer: false,
        is_writable,
        owner,
    }
}

fn root(program: Pubkey, accounts: Vec<AccountRef>) -> CpiModel {
    CpiModel {
        header: None,
        roots: vec![Root {
            signers: vec![],
            frame: ResolvedFrame {
                program,
                instruction_name: Some("Take".to_string()),
                outcome: Outcome::Success,
                compute_units: None,
                accounts,
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
    }
}

#[test]
fn ownership_graph_groups_writable_accounts_by_owner() {
    let escrow = Pubkey::new_unique();
    let token = Pubkey::new_unique();
    let vault = Pubkey::new_unique(); // PDA owned by escrow
    let escrow_ata = Pubkey::new_unique(); // token account owned by token
    let config = Pubkey::new_unique(); // read-only -> dropped

    let aliases = Aliases::default()
        .with(escrow, "Escrow")
        .with(token, "Token")
        .with(vault, "vault")
        .with(escrow_ata, "escrow_ata")
        .with(config, "config");

    let model = root(
        escrow,
        vec![
            account(vault, true, Some(escrow)),
            account(escrow_ata, true, Some(token)),
            account(config, false, Some(escrow)),
        ],
    );

    let out = OwnershipGraph.render(&model, &aliases);
    assert!(out.contains("Escrow[Escrow]:::owner"), "{out}");
    assert!(out.contains("Token[Token]:::owner"), "{out}");
    assert!(out.contains("vault[(vault)]:::account"), "{out}");
    assert!(out.contains("escrow_ata[(escrow_ata)]:::account"), "{out}");
    assert!(out.contains("Escrow -->|owns| vault"), "{out}");
    assert!(out.contains("Token -->|owns| escrow_ata"), "{out}");
    assert!(
        !out.contains("config"),
        "read-only account should be dropped; got:\n{out}"
    );
}

#[test]
fn ownership_graph_empty_when_owners_unfilled() {
    // `build` leaves AccountRef.owner = None; without fill_owners there are no
    // edges, so the graph is the empty string rather than a bare flowchart.
    let prog = Pubkey::new_unique();
    let acct = Pubkey::new_unique();
    let model = root(prog, vec![account(acct, true, None)]);
    assert_eq!(OwnershipGraph.render(&model, &Aliases::default()), "");
}
