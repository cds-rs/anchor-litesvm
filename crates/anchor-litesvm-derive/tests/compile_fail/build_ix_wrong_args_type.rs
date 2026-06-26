//! Calls `Program::build_ix` with an args type that doesn't have a
//! `BuildableIx<Bundle>` impl. The `#[diagnostic::on_unimplemented]`
//! attribute on `BuildableIx` should make the failure point at the
//! missing impl and suggest the derive form, instead of dropping a bare
//! "trait bound not satisfied" message.

use anchor_lang::prelude::Pubkey;

#[derive(Copy, Clone, Debug)]
pub struct Bundle {
    pub auth: Pubkey,
}

// No `BuildableIx<Bundle>` impl: derive was forgotten, hand-impl wasn't
// written, or the user is just experimenting.
#[derive(anchor_lang::AnchorSerialize, anchor_lang::AnchorDeserialize)]
pub struct DoSomething {
    pub seed: u64,
}
impl anchor_lang::Discriminator for DoSomething {
    const DISCRIMINATOR: &'static [u8] = &[1, 2, 3, 4, 5, 6, 7, 8];
}
impl anchor_lang::InstructionData for DoSomething {}

fn main() {
    let program = anchor_litesvm::Program::new(Pubkey::new_unique());
    let bundle = Bundle {
        auth: Pubkey::new_unique(),
    };
    let _ix = program.build_ix(bundle, DoSomething { seed: 1 });
}
