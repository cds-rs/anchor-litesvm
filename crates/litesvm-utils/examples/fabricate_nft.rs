//! Fabricate a *complete* NFT by writing account bytes directly: the mint, its
//! Metaplex Token Metadata, and a holder token account, with no minting
//! transaction.
//!
//! This is the realistic recipe the fabrication helpers exist for. When a test
//! needs an NFT to simply *exist* for the program under test to inspect (a
//! marketplace validating a listing, a vault checking a deposit), it doesn't
//! need to drive the real SPL Token + Metaplex instructions; it writes the
//! finished accounts and moves on. Classic-SPL here; the same `TokenProgram`
//! switch fabricates the Token-2022 base layout.
//!
//! Run: `cargo run -p litesvm-utils --example fabricate_nft`

use litesvm_utils::{
    Creator, LiteSVM, MetadataArgs, MetaplexHelpers, Pubkey, TestHelpers, TokenFabrication,
    TokenProgram, TokenStandard, MPL_TOKEN_METADATA_ID,
};
use solana_program_pack::Pack;
use spl_token::state::Mint;

fn main() {
    let mut svm = LiteSVM::new();

    let owner = Pubkey::new_unique();
    let creator = Pubkey::new_unique();
    let mint = Pubkey::new_unique();

    // 1. The NFT mint: supply 1, decimals 0, owned by the SPL Token program.
    svm.fabricate_nft_mint(&mint, TokenProgram::Spl);

    // 2. The Token Metadata account, at its canonical PDA, owned by the Token
    //    Metadata program. Hand-serialized, so no `mpl-token-metadata` dependency.
    let metadata = svm.fabricate_metadata(
        &mint,
        &MetadataArgs {
            name: "Genesis #1".into(),
            symbol: "GEN".into(),
            uri: "https://example.com/1.json".into(),
            seller_fee_basis_points: 500,
            creators: vec![Creator { address: creator, verified: true, share: 100 }],
            token_standard: Some(TokenStandard::NonFungible),
            ..Default::default()
        },
    );

    // 3. A holder: `owner`'s token account carrying the single NFT.
    let holder = Pubkey::new_unique();
    svm.fabricate_token_account(&holder, TokenProgram::Spl, &mint, &owner, 1);

    // Read it all back: a program would now see a fully-formed NFT.
    let mint_acct = svm.get_account(&mint).expect("mint exists");
    let m = Mint::unpack(&mint_acct.data).expect("unpacks as an SPL Mint");
    assert_eq!((m.supply, m.decimals), (1, 0), "the canonical NFT shape");

    let meta_acct = svm.get_account(&metadata).expect("metadata exists");
    assert_eq!(meta_acct.owner, MPL_TOKEN_METADATA_ID, "owned by Token Metadata");
    assert_eq!(meta_acct.data[0], 4, "MetadataV1 key discriminator");

    assert_eq!(svm.token_balance(&holder), Some(1), "the holder owns the NFT");

    println!("fabricated a complete NFT (no minting transaction):");
    println!("  mint      {mint}  (supply {}, decimals {})", m.supply, m.decimals);
    println!("  metadata  {metadata}  (Token Metadata PDA, key={})", meta_acct.data[0]);
    println!("  holder    {holder}  (balance {})", svm.token_balance(&holder).unwrap());
    println!("all assertions passed.");
}
