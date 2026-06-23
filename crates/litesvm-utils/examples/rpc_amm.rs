//! Live proof: drive the deployed AMM's `initialize` through `RpcBackend` against
//! a running surfnet and render the multi-CPI tree it produces.
//!
//! Requires a surfnet with the `02-amm` program deployed (program id below) and
//! a mainnet datasource (for the USDC/USDT mints):
//! ```sh
//! cargo run -p litesvm-utils --features rpc --example rpc_amm
//! ```
//! `initialize` creates the pool `config` PDA, the LP-mint PDA, and three vault
//! ATAs via CPI to the token + ATA programs, so the tree is genuinely nested.

#[cfg(not(feature = "rpc"))]
fn main() {
    eprintln!("rebuild with `--features rpc`");
}

#[cfg(feature = "rpc")]
fn main() {
    use {
        litesvm::cpi_tree::{cpi_tree, format_cpi_tree},
        litesvm_utils::{Aliases, RpcBackend, TestSVM, TransactionResult},
        solana_keypair::Keypair,
        solana_program::{
            instruction::{AccountMeta, Instruction},
            pubkey::Pubkey,
        },
        solana_signer::Signer,
        spl_associated_token_account::get_associated_token_address,
        std::str::FromStr,
    };

    let pk = |s: &str| Pubkey::from_str(s).unwrap();
    let amm = pk("5aDxxnPDGeVEuLXerisV4GF5f8tcwTjbMK7Bn1dYUXSi");
    let usdc = pk("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    let usdt = pk("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB");
    let token_program = pk("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
    let ata_program = pk("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
    let system_program = pk("11111111111111111111111111111111");

    let url = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".to_string());
    println!("connecting to surfnet at {url}");
    let mut backend = RpcBackend::new(url);

    // Use the surfnet's pre-funded deploy payer (genesis-airdropped) rather than
    // request_airdrop, which can be rate-limited.
    let home = std::env::var("HOME").unwrap();
    let raw = std::fs::read_to_string(format!("{home}/.config/solana/id.json")).unwrap();
    let bytes: Vec<u8> = raw
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|b| b.trim().parse::<u8>().unwrap())
        .collect();
    let payer = Keypair::try_from(&bytes[..]).expect("parse ~/.config/solana/id.json");

    // PDAs the program derives; ATAs it `init`s. A fresh seed per run gives a
    // distinct `config` PDA so re-runs don't collide on the shared surfnet.
    let seed: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let (config, _) = Pubkey::find_program_address(&[b"config", &seed.to_le_bytes()], &amm);
    let (mint_lp, _) = Pubkey::find_program_address(&[b"lp", config.as_ref()], &amm);
    let vault_x = get_associated_token_address(&config, &usdc);
    let vault_y = get_associated_token_address(&config, &usdt);
    let lp_vault = get_associated_token_address(&config, &mint_lp);

    // initialize(seed: u64, fee_bps: u16, authority: Option<Pubkey>)
    let mut data = vec![175, 175, 109, 31, 13, 152, 155, 237]; // anchor discriminator
    data.extend_from_slice(&seed.to_le_bytes());
    data.extend_from_slice(&30u16.to_le_bytes()); // fee_bps
    data.push(0); // authority: Option<Pubkey> = None (renounce)

    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true), // initializer
        AccountMeta::new_readonly(usdc, false), // mint_x
        AccountMeta::new_readonly(usdt, false), // mint_y
        AccountMeta::new(mint_lp, false),       // mint_lp (PDA, init)
        AccountMeta::new(vault_x, false),       // vault_x (ATA, init)
        AccountMeta::new(vault_y, false),       // vault_y (ATA, init)
        AccountMeta::new(lp_vault, false),      // lp_vault (ATA, init)
        AccountMeta::new(config, false),        // config (PDA, init)
        AccountMeta::new_readonly(token_program, false),
        AccountMeta::new_readonly(ata_program, false),
        AccountMeta::new_readonly(system_program, false),
    ];
    let ix = Instruction {
        program_id: amm,
        accounts,
        data,
    };

    let record = backend.send(&[ix], &[&payer]);
    println!("error:         {:?}", record.error);
    println!("compute_units: {}", record.compute_units);

    // Bare litesvm render: raw pubkeys (litesvm owns the vocabulary, not naming).
    println!("\n=== bare litesvm render (format_cpi_tree, raw pubkeys) ===");
    let frames = cpi_tree(&record.logs);
    println!("{}", format_cpi_tree("AMM initialize", &frames));

    // anchor-litesvm render: the SAME record lifted into TransactionResult and run
    // through the aliased renderer. Well-known program names come for free; the
    // pool accounts get the names we register. This is the consumer augmenting the
    // executor's base payload.
    println!("=== anchor-litesvm render (aliased) ===");
    let aliases = Aliases::with_well_known()
        .with(amm, "AMM")
        .with(config, "config")
        .with(mint_lp, "mint_lp")
        .with(vault_x, "vault_x")
        .with(vault_y, "vault_y")
        .with(lp_vault, "lp_vault")
        .with(usdc, "USDC")
        .with(usdt, "USDT");
    let result: TransactionResult = record.into();
    println!("{}", result.with_aliases(aliases).logs_structured_string());
}
