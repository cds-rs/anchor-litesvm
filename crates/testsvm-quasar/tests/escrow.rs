//! Dogfood: Quasar's own `escrow` example program, driven through the
//! `testsvm` vocabulary on the quasar engine. The suite plays the escrow
//! lifecycle as scenario verbs: `open` (make), then `take` or `refund`, each a
//! real chained flow where the state make wrote is the state take/refund reads.
//!
//! The point is to prove the port reaches a real third-framework program: cast
//! the actors, fabricate the SPL state as props, send each verb, and read the
//! engine-neutral CPI tree from `model::Transaction`. We use the
//! "existing token accounts" variants throughout, so the escrow PDA is the only
//! account the program creates (via `invoke_signed`); the token accounts are
//! pre-fabricated. That sidesteps caller-signed account creation, which
//! testsvm's cast vocabulary (actor = funded signer, prop = non-signer) has no
//! primitive for.
//!
//! Instructions are hand-built from their flat layouts (make = `[0]` ++ LE
//! deposit ++ LE receive; take = `[1]`; refund = `[2]`) rather than via
//! `quasar-escrow-client`: that client pins `solana-address = "=2.2.0"` exactly
//! and won't co-resolve with a solana-3.1 graph (see Cargo.toml).

use {
    quasar_svm::token::{create_keyed_mint_account, create_keyed_token_account},
    solana_instruction::{AccountMeta, Instruction},
    solana_keypair::Keypair,
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    spl_token_interface::state::{Account as TokenAccount, AccountState, Mint},
    std::str::FromStr,
    testsvm::TestSVM,
    testsvm_quasar::QuasarBackend,
};

const ESCROW_SO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quasar_escrow.so");

/// The SPL token account amount field lives at bytes 64..72 (mint[32] ++
/// owner[32] ++ amount[8]).
const TOKEN_AMOUNT: std::ops::Range<usize> = 64..72;

/// Quasar's escrow `declare_id!("2222…")`. The program asserts its own id and
/// derives the escrow PDA against it, so the deploy address must match exactly.
fn program_id() -> Pubkey {
    Pubkey::from_str("22222222222222222222222222222222222222222222").unwrap()
}

/// Read an SPL token account's `amount`.
fn token_amount(backend: &QuasarBackend, address: &Pubkey) -> u64 {
    let account = backend.get_account(address).expect("token account exists");
    u64::from_le_bytes(account.data[TOKEN_AMOUNT].try_into().unwrap())
}

// --- events ----------------------------------------------------------------
// Quasar's `#[event(discriminator = N)]` emits `sol_log_data` with a 1-byte
// leading discriminator then the fields. These impls teach the registry that
// 1-byte scheme (Anchor's is 8 bytes); the field layout mirrors the program's
// `events.rs`.

fn pk_field(body: &[u8], offset: usize) -> String {
    Pubkey::new_from_array(body[offset..offset + 32].try_into().unwrap()).to_string()
}
fn u64_field(body: &[u8], offset: usize) -> String {
    u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap()).to_string()
}

struct MakeEvent;
impl testsvm::events::DecodableEvent for MakeEvent {
    const DISCRIMINATOR: &'static [u8] = &[0];
    fn name() -> &'static str {
        "MakeEvent"
    }
    fn decode(body: &[u8]) -> Option<Vec<(String, String)>> {
        (body.len() >= 144).then(|| {
            vec![
                ("escrow".into(), pk_field(body, 0)),
                ("maker".into(), pk_field(body, 32)),
                ("mint_a".into(), pk_field(body, 64)),
                ("mint_b".into(), pk_field(body, 96)),
                ("deposit".into(), u64_field(body, 128)),
                ("receive".into(), u64_field(body, 136)),
            ]
        })
    }
}

struct TakeEvent;
impl testsvm::events::DecodableEvent for TakeEvent {
    const DISCRIMINATOR: &'static [u8] = &[1];
    fn name() -> &'static str {
        "TakeEvent"
    }
    fn decode(body: &[u8]) -> Option<Vec<(String, String)>> {
        (body.len() >= 32).then(|| vec![("escrow".into(), pk_field(body, 0))])
    }
}

struct RefundEvent;
impl testsvm::events::DecodableEvent for RefundEvent {
    const DISCRIMINATOR: &'static [u8] = &[2];
    fn name() -> &'static str {
        "RefundEvent"
    }
    fn decode(body: &[u8]) -> Option<Vec<(String, String)>> {
        (body.len() >= 32).then(|| vec![("escrow".into(), pk_field(body, 0))])
    }
}

// --- account fabrication ---------------------------------------------------

/// Fabricate an SPL mint through Quasar's own helper, handed to the port as a
/// plain `(address, account)` prop.
fn mint(backend: &mut QuasarBackend, name: &str, address: Pubkey, authority: Pubkey) -> Pubkey {
    let account = create_keyed_mint_account(
        &address,
        &Mint {
            mint_authority: Some(authority).into(),
            supply: 1_000_000_000,
            decimals: 9,
            is_initialized: true,
            freeze_authority: None.into(),
        },
    );
    backend.prop_at(name, &address, account.to_pair().1)
}

/// Fabricate an SPL token account (mint, owner, amount) as a prop.
fn token(
    backend: &mut QuasarBackend,
    name: &str,
    address: Pubkey,
    mint: Pubkey,
    owner: Pubkey,
    amount: u64,
) -> Pubkey {
    let account = create_keyed_token_account(
        &address,
        &TokenAccount {
            mint,
            owner,
            amount,
            state: AccountState::Initialized,
            ..TokenAccount::default()
        },
    );
    backend.prop_at(name, &address, account.to_pair().1)
}

// --- instruction layouts ---------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn make_ix(
    id: Pubkey,
    maker: Pubkey,
    escrow: Pubkey,
    mint_a: Pubkey,
    mint_b: Pubkey,
    maker_ta_a: Pubkey,
    maker_ta_b: Pubkey,
    vault_ta_a: Pubkey,
    rent: Pubkey,
    token_program: Pubkey,
    system_program: Pubkey,
    deposit: u64,
    receive: u64,
) -> Instruction {
    let accounts = vec![
        AccountMeta::new(maker, true),
        AccountMeta::new(escrow, false),
        AccountMeta::new_readonly(mint_a, false),
        AccountMeta::new_readonly(mint_b, false),
        AccountMeta::new(maker_ta_a, false),
        AccountMeta::new(maker_ta_b, false),
        AccountMeta::new(vault_ta_a, false),
        AccountMeta::new_readonly(rent, false),
        AccountMeta::new_readonly(token_program, false),
        AccountMeta::new_readonly(system_program, false),
    ];
    let mut data = vec![0u8];
    data.extend_from_slice(&deposit.to_le_bytes());
    data.extend_from_slice(&receive.to_le_bytes());
    Instruction {
        program_id: id,
        accounts,
        data,
    }
}

#[allow(clippy::too_many_arguments)]
fn take_ix(
    id: Pubkey,
    taker: Pubkey,
    escrow: Pubkey,
    maker: Pubkey,
    mint_a: Pubkey,
    mint_b: Pubkey,
    taker_ta_a: Pubkey,
    taker_ta_b: Pubkey,
    maker_ta_b: Pubkey,
    vault_ta_a: Pubkey,
    rent: Pubkey,
    token_program: Pubkey,
    system_program: Pubkey,
) -> Instruction {
    let accounts = vec![
        AccountMeta::new(taker, true),
        AccountMeta::new(escrow, false),
        AccountMeta::new(maker, false),
        AccountMeta::new_readonly(mint_a, false),
        AccountMeta::new_readonly(mint_b, false),
        AccountMeta::new(taker_ta_a, false),
        AccountMeta::new(taker_ta_b, false),
        AccountMeta::new(maker_ta_b, false),
        AccountMeta::new(vault_ta_a, false),
        AccountMeta::new_readonly(rent, false),
        AccountMeta::new_readonly(token_program, false),
        AccountMeta::new_readonly(system_program, false),
    ];
    Instruction {
        program_id: id,
        accounts,
        data: vec![1u8],
    }
}

#[allow(clippy::too_many_arguments)]
fn refund_ix(
    id: Pubkey,
    maker: Pubkey,
    escrow: Pubkey,
    mint_a: Pubkey,
    maker_ta_a: Pubkey,
    vault_ta_a: Pubkey,
    rent: Pubkey,
    token_program: Pubkey,
    system_program: Pubkey,
) -> Instruction {
    let accounts = vec![
        AccountMeta::new(maker, true),
        AccountMeta::new(escrow, false),
        AccountMeta::new_readonly(mint_a, false),
        AccountMeta::new(maker_ta_a, false),
        AccountMeta::new(vault_ta_a, false),
        AccountMeta::new_readonly(rent, false),
        AccountMeta::new_readonly(token_program, false),
        AccountMeta::new_readonly(system_program, false),
    ];
    Instruction {
        program_id: id,
        accounts,
        data: vec![2u8],
    }
}

// --- the world -------------------------------------------------------------

const DEPOSIT: u64 = 1337;
const RECEIVE: u64 = 1337;
const MAKER_START_A: u64 = 1_000_000;

/// Everything an opened escrow leaves on the table: the maker, the PDA, the
/// mints, and the maker-side token accounts. `take` and `refund` read from here.
struct Opened {
    maker: Keypair,
    escrow: Pubkey,
    bump: u8,
    mint_a: Pubkey,
    mint_b: Pubkey,
    maker_ta_a: Pubkey,
    maker_ta_b: Pubkey,
    vault_ta_a: Pubkey,
    rent: Pubkey,
    token_program: Pubkey,
    system_program: Pubkey,
}

/// The `open` verb: cast the maker and the SPL state, run make, and assert the
/// escrow PDA now carries the program's state with the deposit in the vault.
fn open_escrow(backend: &mut QuasarBackend, id: Pubkey) -> Opened {
    backend.register_event::<MakeEvent>();
    backend.register_event::<TakeEvent>();
    backend.register_event::<RefundEvent>();

    let maker = backend.actor("maker", 1_000_000_000);
    let maker_pk = maker.pubkey();
    let (escrow, bump) = Pubkey::find_program_address(&[b"escrow", maker_pk.as_ref()], &id);
    backend.register_alias(&escrow, "Escrow");

    let mint_a = mint(backend, "MintA", Pubkey::new_from_array([3; 32]), maker_pk);
    let mint_b = mint(backend, "MintB", Pubkey::new_from_array([4; 32]), maker_pk);
    let maker_ta_a = token(
        backend,
        "MakerTokenA",
        Pubkey::new_from_array([5; 32]),
        mint_a,
        maker_pk,
        MAKER_START_A,
    );
    let maker_ta_b = token(
        backend,
        "MakerTokenB",
        Pubkey::new_from_array([6; 32]),
        mint_b,
        maker_pk,
        0,
    );
    // The vault is owned by the escrow PDA: where the maker's deposit lands.
    let vault_ta_a = token(
        backend,
        "Vault",
        Pubkey::new_from_array([7; 32]),
        mint_a,
        escrow,
        0,
    );

    let rent = Pubkey::new_from_array(quasar_svm::solana_sdk_ids::sysvar::rent::ID.to_bytes());
    let token_program = Pubkey::new_from_array(quasar_svm::SPL_TOKEN_PROGRAM_ID.to_bytes());
    let system_program = Pubkey::new_from_array(quasar_svm::system_program::ID.to_bytes());

    let ix = make_ix(
        id, maker_pk, escrow, mint_a, mint_b, maker_ta_a, maker_ta_b, vault_ta_a, rent,
        token_program, system_program, DEPOSIT, RECEIVE,
    );
    let tx = backend.send(&[ix], &[&maker]);
    let tree = tx.pretty_cpi_tree();
    println!("\n=== make ===\n{tree}");
    assert!(tx.error.is_none(), "make should succeed: {:?}", tx.error);
    assert!(
        tree.contains(
            "🔔 MakeEvent { escrow: Escrow, maker: maker, mint_a: MintA, \
             mint_b: MintB, deposit: 1337, receive: 1337 }"
        ),
        "the make event decodes with alias-resolved pubkey fields:\n{tree}"
    );

    let escrow_acct = backend.get_account(&escrow).expect("make created the PDA");
    assert_eq!(escrow_acct.owner, id, "escrow is owned by the program");
    assert_eq!(escrow_acct.data[0], 1, "account discriminator");
    assert_eq!(token_amount(backend, &vault_ta_a), DEPOSIT, "deposit in vault");
    assert_eq!(
        token_amount(backend, &maker_ta_a),
        MAKER_START_A - DEPOSIT,
        "deposit debited from the maker"
    );

    Opened {
        maker,
        escrow,
        bump,
        mint_a,
        mint_b,
        maker_ta_a,
        maker_ta_b,
        vault_ta_a,
        rent,
        token_program,
        system_program,
    }
}

/// An escrow PDA is closed when its lamports return to the maker: a 0-lamport
/// account is the post-state quasar commits.
fn assert_closed(backend: &QuasarBackend, escrow: &Pubkey) {
    let lamports = backend.get_account(escrow).map(|a| a.lamports).unwrap_or(0);
    assert_eq!(lamports, 0, "escrow PDA closed (lamports returned to maker)");
}

// --- the scenarios ---------------------------------------------------------

#[test]
fn escrow_open() {
    let mut backend = QuasarBackend::new();
    let id = program_id();
    backend.deploy_from_file(&id, ESCROW_SO, "escrow");

    let e = open_escrow(&mut backend, id);
    // The make frame nests the PDA create and the deposit transfer; the stored
    // bump round-trips.
    let escrow_acct = backend.get_account(&e.escrow).unwrap();
    assert_eq!(escrow_acct.data[137], e.bump, "stored bump");
}

#[test]
fn escrow_take() {
    let mut backend = QuasarBackend::new();
    let id = program_id();
    backend.deploy_from_file(&id, ESCROW_SO, "escrow");

    let e = open_escrow(&mut backend, id);

    // The taker arrives with mint_b to pay, and an empty mint_a account to
    // receive the vault's deposit.
    let taker = backend.actor("taker", 1_000_000_000);
    let taker_pk = taker.pubkey();
    let taker_ta_a = token(
        &mut backend,
        "TakerTokenA",
        Pubkey::new_from_array([8; 32]),
        e.mint_a,
        taker_pk,
        0,
    );
    let taker_ta_b = token(
        &mut backend,
        "TakerTokenB",
        Pubkey::new_from_array([9; 32]),
        e.mint_b,
        taker_pk,
        10_000,
    );

    let ix = take_ix(
        id, taker_pk, e.escrow, e.maker.pubkey(), e.mint_a, e.mint_b, taker_ta_a, taker_ta_b,
        e.maker_ta_b, e.vault_ta_a, e.rent, e.token_program, e.system_program,
    );
    let tx = backend.send(&[ix], &[&taker]);
    let tree = tx.pretty_cpi_tree();
    println!("\n=== take ===\n{tree}");
    assert!(tx.error.is_none(), "take should succeed: {:?}", tx.error);
    assert!(
        tree.contains("🔔 TakeEvent { escrow: Escrow }"),
        "the take event decodes with the escrow alias:\n{tree}"
    );

    // The deposit moved to the taker; the maker received the asked-for amount;
    // the taker paid it; the escrow closed.
    assert_eq!(token_amount(&backend, &taker_ta_a), DEPOSIT, "taker got the deposit");
    assert_eq!(token_amount(&backend, &e.maker_ta_b), RECEIVE, "maker got paid");
    assert_eq!(
        token_amount(&backend, &taker_ta_b),
        10_000 - RECEIVE,
        "taker paid the receive amount"
    );
    assert_closed(&backend, &e.escrow);
}

#[test]
fn escrow_refund() {
    let mut backend = QuasarBackend::new();
    let id = program_id();
    backend.deploy_from_file(&id, ESCROW_SO, "escrow");

    let e = open_escrow(&mut backend, id);

    let ix = refund_ix(
        id, e.maker.pubkey(), e.escrow, e.mint_a, e.maker_ta_a, e.vault_ta_a, e.rent,
        e.token_program, e.system_program,
    );
    let tx = backend.send(&[ix], &[&e.maker]);
    let tree = tx.pretty_cpi_tree();
    println!("\n=== refund ===\n{tree}");
    assert!(tx.error.is_none(), "refund should succeed: {:?}", tx.error);
    assert!(
        tree.contains("🔔 RefundEvent { escrow: Escrow }"),
        "the refund event decodes with the escrow alias:\n{tree}"
    );

    // The deposit returned to the maker whole, and the escrow closed.
    assert_eq!(
        token_amount(&backend, &e.maker_ta_a),
        MAKER_START_A,
        "deposit refunded to the maker"
    );
    assert_closed(&backend, &e.escrow);
}
