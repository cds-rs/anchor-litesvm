//! End-to-end: drive the AMM through `AnchorContext` against a live surfnet.
//!
//! `ctx.with_backend(RpcBackend::new(url))` routes the context's `send_*` through
//! JSON-RPC; the returned `TransactionResult` renders with the context's aliases.
//! No manual `from_record`: the rewire makes the port load-bearing, the *same*
//! `ctx.send_instructions(...)` call would run in-memory if no backend were set.
//!
//! ```sh
//! surfpool start --no-tui          # in ~/sol/02-amm
//! cargo run -p anchor-litesvm --features rpc --example ctx_rpc_amm
//! ```

#[cfg(not(feature = "rpc"))]
fn main() {
    eprintln!("rebuild with `--features rpc`");
}

#[cfg(feature = "rpc")]
fn main() {
    use {
        anchor_litesvm::AnchorContext,
        litesvm::LiteSVM,
        litesvm_utils::RpcBackend,
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
    println!("AnchorContext over surfnet at {url}");

    // A context with a throwaway in-memory svm (unused for sends), routed through
    // the RpcBackend. Aliases are registered on the context, exactly as for an
    // in-memory scenario.
    let mut ctx =
        AnchorContext::new(LiteSVM::new(), amm).with_backend(Box::new(RpcBackend::new(url)));
    ctx.alias(amm, "AMM");
    ctx.alias(usdc, "USDC");
    ctx.alias(usdt, "USDT");
    // Push the well-known program ids too, so surfpool's own render names every
    // node (these are free on the anchor-litesvm side via its well-known table,
    // but surfpool only knows what clients push it).
    ctx.alias(system_program, "System");
    ctx.alias(token_program, "Token");
    ctx.alias(ata_program, "AssociatedToken");

    // The surfnet's pre-funded deploy payer.
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

    // Fresh pool per run.
    let seed: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let (config, _) = Pubkey::find_program_address(&[b"config", &seed.to_le_bytes()], &amm);
    let (mint_lp, _) = Pubkey::find_program_address(&[b"lp", config.as_ref()], &amm);
    let vault_x = get_associated_token_address(&config, &usdc);
    let vault_y = get_associated_token_address(&config, &usdt);
    let lp_vault = get_associated_token_address(&config, &mint_lp);
    ctx.alias(config, "config");
    ctx.alias(mint_lp, "mint_lp");

    // initialize(seed: u64, fee_bps: u16, authority: Option<Pubkey> = None)
    let mut data = vec![175, 175, 109, 31, 13, 152, 155, 237];
    data.extend_from_slice(&seed.to_le_bytes());
    data.extend_from_slice(&30u16.to_le_bytes());
    data.push(0);
    let accounts = vec![
        AccountMeta::new(payer.pubkey(), true),
        AccountMeta::new_readonly(usdc, false),
        AccountMeta::new_readonly(usdt, false),
        AccountMeta::new(mint_lp, false),
        AccountMeta::new(vault_x, false),
        AccountMeta::new(vault_y, false),
        AccountMeta::new(lp_vault, false),
        AccountMeta::new(config, false),
        AccountMeta::new_readonly(token_program, false),
        AccountMeta::new_readonly(ata_program, false),
        AccountMeta::new_readonly(system_program, false),
    ];
    let ix = Instruction {
        program_id: amm,
        accounts,
        data,
    };

    // The whole point: send THROUGH the context. It routes to the RpcBackend and
    // renders with the context's aliases. Identical call shape to in-memory.
    println!("AMM initialize (config {config}):\n");
    ctx.send_instructions(&[ix], &[&payer])
        .print_logs_structured();
}
