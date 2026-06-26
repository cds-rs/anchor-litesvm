//! The account index: a census of every account a test touched, classified
//! by **owner program** and **authority class**, with **ATA parent edges**
//! recovered by reverse-derivation.
//!
//! This is the trace-only ("derived") core of the account-index idea: it
//! reports exactly what execution proves and nothing it would have to guess.
//! Three things, all from the per-send [`CpiModel`] (whose account authority is
//! trace-sourced, so this reads the same facts the diagram does):
//!
//! - **Census + owner.** Every distinct account that appeared, and its owner
//!   program (the runtime's mutation-permission fact).
//! - **Authority class.** Whether the account ever signed at the transaction
//!   level (a human key), ever signed only inside a CPI (a program-signed
//!   PDA, `invoke_signed`), or never authorized anything (passive).
//! - **ATA edges.** For each token-program-owned account, brute-force
//!   `get_associated_token_address(holder, mint)` over the accounts already
//!   in the census; a match is a *proof* that the account is the holder's ATA
//!   for that mint, so the token accounts nest under their holders. (A
//!   program PDA's seeds are not recoverable this way: `find_program_address`
//!   is one-way. That parentage is author-supplied, out of scope here.)
//!
//! What this deliberately omits, because the trace cannot prove it: PDA seed
//! definitions, account-data field layouts, and in-data values. Those are
//! author annotations (notes), layered on top, not derived.
//!
//! The render defines every name it prints: accounts are nodes in the tree,
//! and the programs they are "owned by" (which the census itself drops as
//! executables) are declared in a `── programs ──` footer.

use {
    super::{aliases::Aliases, model::CpiModel},
    crate::report::{MarkdownBlock, ToMarkdown},
    solana_pubkey::Pubkey,
    std::collections::BTreeMap,
    std::fmt::Write,
    std::str::FromStr,
};

/// The loaders that own *executable* accounts. An account owned by one of
/// these is a program, not state, so the index drops it: the System / Token /
/// AssociatedToken programs get passed as instruction accounts but they are
/// not part of a program's account model.
///
/// Dropped programs still appear as `owned by` references on the nodes that
/// survive; `to_tree` defines those references in its programs footer rather
/// than leaving them dangling.
fn is_loader_owned(owner: &Pubkey) -> bool {
    const LOADERS: &[&str] = &[
        "NativeLoader1111111111111111111111111111111",
        "BPFLoaderUpgradeab1e11111111111111111111111",
        "BPFLoader1111111111111111111111111111111111",
        "BPFLoader2111111111111111111111111111111111",
    ];
    LOADERS
        .iter()
        .any(|id| Pubkey::from_str(id).is_ok_and(|loader| loader == *owner))
}

/// How an account's authority showed up in execution. Ordering is render
/// ordering (signers first) and also classification priority: an account
/// keeps the strongest class it was ever observed in.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuthorityClass {
    /// Signed at the transaction level: a human key.
    HumanSigner,
    /// Signed only inside a CPI, never at the tx level: a program-signed PDA
    /// (`invoke_signed`).
    ProgramSigned,
    /// Appeared but never authorized anything.
    Passive,
}

impl AuthorityClass {
    fn describe(self) -> &'static str {
        match self {
            AuthorityClass::HumanSigner => "human signer",
            AuthorityClass::ProgramSigned => "program-signed",
            AuthorityClass::Passive => "passive",
        }
    }
}

/// One account in the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountNode {
    pub pubkey: Pubkey,
    /// The account's owner program (its `owner` field).
    pub owner: Pubkey,
    pub authority: AuthorityClass,
    /// `Some((holder, mint))` when the account is provably the holder's
    /// associated token account for that mint.
    pub ata_of: Option<(Pubkey, Pubkey)>,
}

/// The full census for a test (or any set of traces), render-ready.
#[derive(Clone, Debug, Default)]
pub struct AccountIndex {
    nodes: Vec<AccountNode>,
}

impl AccountIndex {
    pub fn nodes(&self) -> &[AccountNode] {
        &self.nodes
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Build the census from a set of per-send models and the union of every
    /// transaction's required signers. Owner is taken from the first frame
    /// an account appears in (stable post-execution); authority class is the
    /// strongest role observed across all frames.
    pub(super) fn build(models: &[&CpiModel], all_tx_signers: &[Pubkey]) -> Self {
        // owner + whether-ever-a-frame-signer, in first-appearance order.
        struct Accum {
            owner: Pubkey,
            ever_frame_signer: bool,
        }
        let mut seen: BTreeMap<Pubkey, Accum> = BTreeMap::new();
        let mut order: Vec<Pubkey> = Vec::new();

        for model in models {
            for frame in model.frames() {
                for acc in &frame.accounts {
                    // The model's accounts are trace-sourced, so owner is set;
                    // a frame the trace didn't cover (owner None) is skipped.
                    let Some(owner) = acc.owner else {
                        continue;
                    };
                    // Programs (loader-owned) are executables passed as
                    // instruction accounts, not part of the account model.
                    if is_loader_owned(&owner) {
                        continue;
                    }
                    let entry = seen.entry(acc.pubkey).or_insert_with(|| {
                        order.push(acc.pubkey);
                        Accum {
                            owner,
                            ever_frame_signer: false,
                        }
                    });
                    if acc.is_signer {
                        entry.ever_frame_signer = true;
                    }
                }
            }
        }

        let all_accounts: Vec<Pubkey> = order.clone();

        // ATA edges: a token-owned account whose address matches
        // get_associated_token_address(holder, mint) for some pair already in
        // the census. The mint is itself token-owned; the holder is whatever
        // owns it (a wallet or a PDA, never token-owned).
        let token_id = crate::token::SPL_TOKEN_ID;
        let token_owned: Vec<Pubkey> = all_accounts
            .iter()
            .copied()
            .filter(|pk| seen.get(pk).map(|a| a.owner) == Some(token_id))
            .collect();

        let ata_of = |ata: &Pubkey| -> Option<(Pubkey, Pubkey)> {
            for holder in &all_accounts {
                for mint in &token_owned {
                    if crate::token::associated_token_address(holder, mint, &crate::token::SPL_TOKEN_ID)
                        == *ata
                    {
                        return Some((*holder, *mint));
                    }
                }
            }
            None
        };

        let nodes = order
            .iter()
            .map(|pk| {
                let accum = &seen[pk];
                let authority = if all_tx_signers.contains(pk) {
                    AuthorityClass::HumanSigner
                } else if accum.ever_frame_signer {
                    AuthorityClass::ProgramSigned
                } else {
                    AuthorityClass::Passive
                };
                let ata_of = if accum.owner == token_id {
                    ata_of(pk)
                } else {
                    None
                };
                AccountNode {
                    pubkey: *pk,
                    owner: accum.owner,
                    authority,
                    ata_of,
                }
            })
            .collect();

        Self { nodes }
    }

    /// Render the index as a box-drawing tree, holders with their ATAs nested
    /// beneath them. Roots (non-ATA accounts) are ordered by authority class
    /// then alias; ATA children by alias. `aliases` resolves every pubkey to
    /// its name (and the owner program to its name).
    ///
    /// A `── programs ──` footer follows the accounts. The tree refers to
    /// programs only as `owned by X` labels (the census drops executables;
    /// see `is_loader_owned`), so the footer defines each referenced
    /// program by what the census proves about it: which accounts it owns,
    /// and, for the AssociatedToken program, which ATA edges it derived.
    /// Every name in the render is therefore either a node or declared in
    /// the footer.
    pub fn to_tree(&self, aliases: &Aliases) -> String {
        // Roots = everything that isn't an ATA. ATAs are grouped under their
        // holder.
        let mut children: BTreeMap<Pubkey, Vec<&AccountNode>> = BTreeMap::new();
        let mut roots: Vec<&AccountNode> = Vec::new();
        for node in &self.nodes {
            match node.ata_of {
                Some((holder, _)) => children.entry(holder).or_default().push(node),
                None => roots.push(node),
            }
        }

        // Roots: by (class, alias). Children: by alias.
        let label = |pk: &Pubkey| aliases.label(pk);
        // An account's display name: ATAs get the proved "<holder>/<mint>"
        // composite (so they read in real names even when `alias_ata` was
        // never called); an explicit alias still wins. Everything else is
        // its alias or short form.
        let node_label = |node: &AccountNode| -> String {
            match node.ata_of {
                Some((holder, mint)) => aliases
                    .resolve_by_pubkey(&node.pubkey)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("{}/{}", label(&holder), label(&mint))),
                None => label(&node.pubkey),
            }
        };
        roots.sort_by(|a, b| {
            a.authority
                .cmp(&b.authority)
                .then_with(|| label(&a.pubkey).cmp(&label(&b.pubkey)))
        });
        for kids in children.values_mut() {
            kids.sort_by(|a, b| label(&a.pubkey).cmp(&label(&b.pubkey)));
        }

        let mut out = String::new();
        for root in &roots {
            let _ = writeln!(
                out,
                "{}  ({}, owned by {})",
                label(&root.pubkey),
                root.authority.describe(),
                label(&root.owner),
            );
            if let Some(kids) = children.get(&root.pubkey) {
                let n = kids.len();
                for (i, kid) in kids.iter().enumerate() {
                    let branch = if i + 1 == n { "└──" } else { "├──" };
                    // ata_of is Some here by construction (it's a child).
                    let (_, mint) = kid.ata_of.unwrap_or_default();
                    let _ = writeln!(
                        out,
                        "  {branch} {}  (ATA · mint {})",
                        node_label(kid),
                        label(&mint)
                    );
                }
            }
        }

        // ---- programs footer ----
        // Owners first (sorted by label), then the ATA program when any
        // derivation edge exists; the two relations read differently
        // ("owns" vs "derived"). An empty census renders nothing at all.
        let mut owned_by: BTreeMap<Pubkey, Vec<String>> = BTreeMap::new();
        for node in &self.nodes {
            owned_by
                .entry(node.owner)
                .or_default()
                .push(node_label(node));
        }
        let ata_edges = self.nodes.iter().filter(|n| n.ata_of.is_some()).count();
        if owned_by.is_empty() && ata_edges == 0 {
            return out;
        }

        let _ = writeln!(out);
        let _ = writeln!(out, "── programs ──");

        let mut owners: Vec<(String, Vec<String>)> = owned_by
            .into_iter()
            .map(|(owner, mut owned)| {
                owned.sort();
                (label(&owner), owned)
            })
            .collect();
        owners.sort();

        // Name the owned accounts while that stays readable; fall back to a
        // count once it wouldn't.
        const LIST_OWNED_MAX: usize = 3;
        for (owner_label, owned) in &owners {
            let what = if owned.len() <= LIST_OWNED_MAX {
                format!("owns {}", owned.join(", "))
            } else {
                format!("owns {} accounts", owned.len())
            };
            let _ = writeln!(out, "{owner_label}  ({what})");
        }

        if ata_edges > 0 {
            let plural = if ata_edges == 1 { "edge" } else { "edges" };
            let _ = writeln!(
                out,
                "{}  (derived {ata_edges} ATA {plural})",
                label(&crate::token::ASSOCIATED_TOKEN_ID)
            );
        }

        out
    }
}

impl ToMarkdown for AccountIndex {
    fn to_markdown(&self) -> MarkdownBlock {
        // Rendered against well-known names only; callers wanting their own
        // actor/PDA names use `to_tree(&aliases)` directly (the context
        // convenience does exactly that).
        MarkdownBlock::Fenced {
            lang: "text".to_string(),
            body: self.to_tree(&Aliases::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::cpi::model::{AccountRef, Outcome, ResolvedFrame, Root},
        std::str::FromStr,
    };

    fn token_id() -> Pubkey {
        crate::token::SPL_TOKEN_ID
    }
    fn system_id() -> Pubkey {
        Pubkey::from_str("11111111111111111111111111111111").unwrap()
    }

    fn aref(pubkey: Pubkey, is_signer: bool, owner: Pubkey) -> AccountRef {
        AccountRef {
            pubkey,
            is_signer,
            is_writable: true,
            owner: Some(owner),
        }
    }

    /// A one-frame model carrying these accounts. The census ignores program
    /// and nesting, so a single flat frame is enough to drive `build`.
    fn model1(accounts: Vec<AccountRef>) -> CpiModel {
        CpiModel {
            header: None,
            roots: vec![Root {
                signers: Vec::new(),
                frame: ResolvedFrame {
                    program: Pubkey::new_unique(),
                    instruction_name: None,
                    operands: vec![],
                    outcome: Outcome::Success,
                    compute_units: None,
                    accounts,
                    logs: Vec::new(),
                    data: Vec::new(),
                    children: Vec::new(),
                },
            }],
            tx_signers: Vec::new(),
            error: None,
            compute_units: 0,
            fee: 0,
            events: Default::default(),
        }
    }

    #[test]
    fn classifies_owner_and_authority_and_recovers_ata_edges() {
        let alice = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let pool = Pubkey::new_unique(); // a program PDA (owned by `program`)
        let mint_x = Pubkey::new_unique();
        let alice_x = crate::token::associated_token_address(&alice, &mint_x, &crate::token::SPL_TOKEN_ID);
        let pool_x = crate::token::associated_token_address(&pool, &mint_x, &crate::token::SPL_TOKEN_ID);

        // One frame referencing them all: a swap-shaped CPI.
        let model = model1(vec![
            aref(alice, true, system_id()),  // human signer, System-owned wallet
            aref(pool, true, program),       // program-signed PDA, owned by `program`
            aref(mint_x, false, token_id()), // a mint (token-owned, passive)
            aref(alice_x, false, token_id()),
            aref(pool_x, false, token_id()),
        ]);

        let index = AccountIndex::build(&[&model], &[alice]);

        let by = |pk: Pubkey| index.nodes().iter().find(|n| n.pubkey == pk).unwrap();

        // Authority classes.
        assert_eq!(by(alice).authority, AuthorityClass::HumanSigner);
        assert_eq!(by(pool).authority, AuthorityClass::ProgramSigned);
        assert_eq!(by(mint_x).authority, AuthorityClass::Passive);

        // Owners.
        assert_eq!(by(alice).owner, system_id());
        assert_eq!(by(pool).owner, program);
        assert_eq!(by(mint_x).owner, token_id());

        // ATA edges recovered by reverse-derivation.
        assert_eq!(by(alice_x).ata_of, Some((alice, mint_x)));
        assert_eq!(by(pool_x).ata_of, Some((pool, mint_x)));
        // Non-ATA token accounts (the mint) don't reverse-match.
        assert_eq!(by(mint_x).ata_of, None);
    }

    #[test]
    fn tree_nests_atas_under_holders() {
        let alice = Pubkey::new_unique();
        let mint_x = Pubkey::new_unique();
        let alice_x = crate::token::associated_token_address(&alice, &mint_x, &crate::token::SPL_TOKEN_ID);

        let model = model1(vec![
            aref(alice, true, system_id()),
            aref(mint_x, false, token_id()),
            aref(alice_x, false, token_id()),
        ]);

        let index = AccountIndex::build(&[&model], &[alice]);
        let aliases = Aliases::default().with(alice, "Alice").with(mint_x, "X");
        let tree = index.to_tree(&aliases);

        assert!(
            tree.contains("Alice  (human signer, owned by System)"),
            "got:\n{tree}"
        );
        assert!(tree.contains("└── Alice/X  (ATA · mint X)"), "got:\n{tree}");
        // The mint is a root leaf, not nested.
        assert!(
            tree.contains("X  (passive, owned by Token)"),
            "got:\n{tree}"
        );

        // The programs footer defines every "owned by" reference above, plus
        // the ATA program that proves the nesting edge.
        assert!(tree.contains("── programs ──"), "got:\n{tree}");
        assert!(tree.contains("System  (owns Alice)"), "got:\n{tree}");
        assert!(tree.contains("Token  (owns Alice/X, X)"), "got:\n{tree}");
        assert!(
            tree.contains("AssociatedToken  (derived 1 ATA edge)"),
            "got:\n{tree}"
        );
    }

    #[test]
    fn programs_footer_skips_ata_program_when_no_edges_exist() {
        // The vault shape: no token accounts at all, so no ATA program line;
        // owner references still get defined.
        let alice = Pubkey::new_unique();
        let vault_state = Pubkey::new_unique();
        let program = Pubkey::new_unique();

        let model = model1(vec![
            aref(alice, true, system_id()),
            aref(vault_state, false, program),
        ]);

        let index = AccountIndex::build(&[&model], &[alice]);
        let aliases = Aliases::default()
            .with(alice, "Alice")
            .with(vault_state, "VaultState")
            .with(program, "Vault");
        let tree = index.to_tree(&aliases);

        assert!(tree.contains("── programs ──"), "got:\n{tree}");
        assert!(tree.contains("System  (owns Alice)"), "got:\n{tree}");
        assert!(tree.contains("Vault  (owns VaultState)"), "got:\n{tree}");
        assert!(!tree.contains("AssociatedToken"), "got:\n{tree}");
    }

    #[test]
    fn programs_footer_counts_owned_accounts_past_the_listing_limit() {
        // Five System-owned wallets: too many to name, so the footer counts.
        let wallets: Vec<Pubkey> = (0..5).map(|_| Pubkey::new_unique()).collect();

        let model = model1(
            wallets
                .iter()
                .map(|w| aref(*w, false, system_id()))
                .collect(),
        );

        let index = AccountIndex::build(&[&model], &[]);
        let tree = index.to_tree(&Aliases::default());

        assert!(tree.contains("System  (owns 5 accounts)"), "got:\n{tree}");
    }

    #[test]
    fn empty_census_renders_nothing() {
        let index = AccountIndex::default();
        assert_eq!(index.to_tree(&Aliases::default()), "");
    }

    #[test]
    fn program_accounts_are_dropped_from_the_census() {
        // The System program account, passed as an instruction account, is
        // owned by NativeLoader; it must not appear in the index.
        let alice = Pubkey::new_unique();
        let system_program = system_id(); // 11111…; owned by NativeLoader at runtime
        let native_loader =
            Pubkey::from_str("NativeLoader1111111111111111111111111111111").unwrap();

        let model = model1(vec![
            aref(alice, true, system_id()),
            // the System program account itself, loader-owned
            aref(system_program, false, native_loader),
        ]);

        let index = AccountIndex::build(&[&model], &[alice]);
        assert!(index.nodes().iter().any(|n| n.pubkey == alice));
        assert!(
            !index.nodes().iter().any(|n| n.pubkey == system_program),
            "loader-owned program accounts are not state"
        );
    }

    #[test]
    fn pda_heavy_program_has_no_ata_edges() {
        // The vault shape: program PDA + System accounts, no tokens. Every
        // account is a flat root; no ATA edges to recover.
        let alice = Pubkey::new_unique();
        let vault_state = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let program = Pubkey::new_unique();

        let model = model1(vec![
            aref(alice, true, system_id()),
            aref(vault_state, false, program), // Vault-owned data PDA
            aref(vault, true, system_id()),    // System-owned, program-signed
        ]);

        let index = AccountIndex::build(&[&model], &[alice]);
        assert!(index.nodes().iter().all(|n| n.ata_of.is_none()));
        assert_eq!(index.nodes().len(), 3);
    }
}
