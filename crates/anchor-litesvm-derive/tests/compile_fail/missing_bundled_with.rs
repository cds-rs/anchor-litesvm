use anchor_litesvm_derive::BundledPubkeys;

#[derive(BundledPubkeys)]
struct Make {
    pub maker: anchor_lang::prelude::Pubkey,
}

fn main() {}
