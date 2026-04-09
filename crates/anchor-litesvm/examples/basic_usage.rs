use anchor_litesvm::{AnchorLiteSVM, AssertionHelpers, TestHelpers};
use solana_program::pubkey::Pubkey;
use solana_signer::Signer;

fn main() {
    println!("anchor-litesvm basic_usage");
    println!("This packaged example is compile-oriented and documents the core test flow.");
    println!("See the source for a minimal setup using AnchorLiteSVM and litesvm-utils helpers.");
}

#[allow(dead_code)]
fn compile_only_example() {
    let mut ctx = AnchorLiteSVM::build_with_program(Pubkey::new_unique(), &[]);
    let payer = ctx.svm.create_funded_account(10_000_000_000).unwrap();
    let mint = ctx.svm.create_token_mint(&payer, 9).unwrap();
    let ata = ctx
        .svm
        .create_associated_token_account(&mint.pubkey(), &payer)
        .unwrap();

    ctx.svm
        .mint_to(&mint.pubkey(), &ata, &payer, 1_000_000)
        .unwrap();

    ctx.svm.assert_account_exists(&payer.pubkey());
    ctx.svm.assert_token_balance(&ata, 1_000_000);
}
