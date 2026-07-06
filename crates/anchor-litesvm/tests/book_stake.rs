//! Raw-instruction client for the vendored `staking` program (Task 12,
//! `examples/staking/`). The program is anchor 0.31 (mpl-core dependency),
//! so its IDL can't feed the host's anchor-1.0 `declare_program!` /
//! `bundles_from_idl!`; instead we hand-build each `Instruction` from the
//! vendored `#[derive(Accounts)]` structs directly.
//!
//! No tests live here yet: Tasks 14/15 drive these builders through actual
//! scenarios. Until then several of these functions are unused, hence the
//! blanket allow below rather than peppering every helper with one.
#![allow(dead_code)]

mod common;

use anchor_lang::prelude::Pubkey;
use sha2::{Digest, Sha256};
use solana_instruction::{AccountMeta, Instruction};

const STAKING_ID: Pubkey = Pubkey::from_str_const("GoZYUCqeKxN2TXNcAnSm8aGfWSpqzBgSqackvDzzFAMg");
const MPL_CORE_ID: Pubkey = Pubkey::from_str_const("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
const SYSTEM_ID: Pubkey = Pubkey::from_str_const("11111111111111111111111111111111");
const TOKEN_ID: Pubkey = Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ATA_ID: Pubkey = Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// Anchor 0.31 8-byte instruction discriminator: `sha256("global:<name>")[..8]`.
fn disc(name: &str) -> [u8; 8] {
    let h = Sha256::digest(format!("global:{name}").as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h[..8]);
    d
}

/// Borsh-encode a `String`: 4-byte LE length prefix + UTF-8 bytes.
fn push_str(data: &mut Vec<u8>, s: &str) {
    data.extend_from_slice(&(s.len() as u32).to_le_bytes());
    data.extend_from_slice(s.as_bytes());
}

// Seeds mirror `constants.rs`: SEED_CONFIG / SEED_UPDATE_AUTHORITY / SEED_REWARDS_MINT.
fn config_pda(collection: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"config", collection.as_ref()], &STAKING_ID)
}
fn update_authority_pda(collection: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"update_authority", collection.as_ref()], &STAKING_ID)
}
fn rewards_mint_pda(config: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"rewards_mint", config.as_ref()], &STAKING_ID)
}
fn ata(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[owner.as_ref(), TOKEN_ID.as_ref(), mint.as_ref()], &ATA_ID).0
}

/// Mirrors `instructions/initialize.rs::Initialize`.
fn ix_initialize(
    admin: &Pubkey,
    collection: &Pubkey,
    rewards_bps: u16,
    freeze_period: u16,
) -> Instruction {
    let (config, _) = config_pda(collection);
    let (ua, _) = update_authority_pda(collection);
    let (rewards_mint, _) = rewards_mint_pda(&config);
    let mut data = disc("initialize").to_vec();
    data.extend_from_slice(&rewards_bps.to_le_bytes());
    data.extend_from_slice(&freeze_period.to_le_bytes());
    Instruction {
        program_id: STAKING_ID,
        accounts: vec![
            AccountMeta::new(*admin, true),
            AccountMeta::new(config, false),
            AccountMeta::new_readonly(*collection, false),
            AccountMeta::new(ua, false),
            AccountMeta::new(rewards_mint, false),
            AccountMeta::new_readonly(SYSTEM_ID, false),
            AccountMeta::new_readonly(TOKEN_ID, false),
        ],
        data,
    }
}

/// Mirrors `instructions/create_collection.rs::CreateCollection`. `collection`
/// is a fresh signer here (a new mpl-core collection asset), not the PDA-owned
/// account it becomes an input to in every other instruction.
fn ix_create_collection(payer: &Pubkey, collection: &Pubkey, name: &str, uri: &str) -> Instruction {
    let (ua, _) = update_authority_pda(collection);
    let mut data = disc("create_collection").to_vec();
    push_str(&mut data, name);
    push_str(&mut data, uri);
    Instruction {
        program_id: STAKING_ID,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(*collection, true),
            AccountMeta::new_readonly(ua, false),
            AccountMeta::new_readonly(SYSTEM_ID, false),
            AccountMeta::new_readonly(MPL_CORE_ID, false),
        ],
        data,
    }
}

/// Mirrors `instructions/mint_asset.rs::MintAsset`.
fn ix_mint_asset(
    user: &Pubkey,
    asset: &Pubkey,
    collection: &Pubkey,
    name: &str,
    uri: &str,
) -> Instruction {
    let (ua, _) = update_authority_pda(collection);
    let mut data = disc("mint_asset").to_vec();
    push_str(&mut data, name);
    push_str(&mut data, uri);
    Instruction {
        program_id: STAKING_ID,
        accounts: vec![
            AccountMeta::new(*user, true),
            AccountMeta::new(*asset, true),
            AccountMeta::new(*collection, false),
            AccountMeta::new_readonly(ua, false),
            AccountMeta::new_readonly(SYSTEM_ID, false),
            AccountMeta::new_readonly(MPL_CORE_ID, false),
        ],
        data,
    }
}

/// Mirrors `instructions/stake.rs::Stake`.
fn ix_stake(owner: &Pubkey, asset: &Pubkey, collection: &Pubkey) -> Instruction {
    let (config, _) = config_pda(collection);
    let (ua, _) = update_authority_pda(collection);
    Instruction {
        program_id: STAKING_ID,
        accounts: vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(config, false),
            AccountMeta::new(*asset, false),
            AccountMeta::new(*collection, false),
            AccountMeta::new_readonly(ua, false),
            AccountMeta::new_readonly(SYSTEM_ID, false),
            AccountMeta::new_readonly(MPL_CORE_ID, false),
        ],
        data: disc("stake").to_vec(),
    }
}

/// Mirrors `instructions/unstake.rs::Unstake`.
fn ix_unstake(owner: &Pubkey, asset: &Pubkey, collection: &Pubkey) -> Instruction {
    let (config, _) = config_pda(collection);
    let (ua, _) = update_authority_pda(collection);
    let (rewards_mint, _) = rewards_mint_pda(&config);
    let user_rewards_ata = ata(owner, &rewards_mint);
    Instruction {
        program_id: STAKING_ID,
        accounts: vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(config, false),
            AccountMeta::new(*asset, false),
            AccountMeta::new(*collection, false),
            AccountMeta::new_readonly(ua, false),
            AccountMeta::new(rewards_mint, false),
            AccountMeta::new(user_rewards_ata, false),
            AccountMeta::new_readonly(TOKEN_ID, false),
            AccountMeta::new_readonly(ATA_ID, false),
            AccountMeta::new_readonly(SYSTEM_ID, false),
            AccountMeta::new_readonly(MPL_CORE_ID, false),
        ],
        data: disc("unstake").to_vec(),
    }
}

/// Mirrors `instructions/claim_rewards.rs::ClaimRewards` (identical account
/// shape to `Unstake`).
fn ix_claim_rewards(owner: &Pubkey, asset: &Pubkey, collection: &Pubkey) -> Instruction {
    let (config, _) = config_pda(collection);
    let (ua, _) = update_authority_pda(collection);
    let (rewards_mint, _) = rewards_mint_pda(&config);
    let user_rewards_ata = ata(owner, &rewards_mint);
    Instruction {
        program_id: STAKING_ID,
        accounts: vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new_readonly(config, false),
            AccountMeta::new(*asset, false),
            AccountMeta::new(*collection, false),
            AccountMeta::new_readonly(ua, false),
            AccountMeta::new(rewards_mint, false),
            AccountMeta::new(user_rewards_ata, false),
            AccountMeta::new_readonly(TOKEN_ID, false),
            AccountMeta::new_readonly(ATA_ID, false),
            AccountMeta::new_readonly(SYSTEM_ID, false),
            AccountMeta::new_readonly(MPL_CORE_ID, false),
        ],
        data: disc("claim_rewards").to_vec(),
    }
}
