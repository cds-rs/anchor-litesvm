//! Token-format casts: the SPL packing the core trait leaves to the caller
//! ([`prop`](crate::TestSVM::prop)'s contract), done once for every engine.
//!
//! Hand-serialized on purpose. `testsvm` must sit in any engine's dependency
//! graph (litesvm's solana-3.4 line, mollusk's agave-4.0 pins) without forcing a
//! version on either, so it carries no `spl-token` dependency; the mint layout
//! is a stable 82 bytes, so owning the serialization costs less than the version
//! conflicts a token crate would drag in. The bytes are cross-checked against
//! `spl_token::state::Mint::unpack` in the adapter tests.

use {crate::TestSVM, solana_account::Account, solana_pubkey::Pubkey};

/// The canonical SPL Token program id (`Tokenkeg…`).
pub const SPL_TOKEN_ID: Pubkey =
    Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

/// The Associated Token Account program id (`ATokenGP…`).
pub const ASSOCIATED_TOKEN_ID: Pubkey =
    Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// Serialized size of an SPL mint account.
pub const MINT_LEN: usize = 82;

/// Serialized size of an SPL token account.
pub const TOKEN_ACCOUNT_LEN: usize = 165;

/// Derive the associated token account address for `(owner, mint)` under
/// `token_program`. Dependency-free (the ATA seeds are stable), so it costs
/// nothing for a crate that must avoid forcing a token-crate version.
pub fn associated_token_address(owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    // `find_program_address` resolves only under the workspace's unified feature
    // set, which pins `solana-address` to 1.x; a standalone `cargo test -p testsvm`
    // picks 2.x, where this is renamed `derive_address`, and fails to compile.
    // The crate is built in-workspace, so this holds today; standalone builds are
    // the follow-up (see NOTES/2026-06-20-code-review-findings.md).
    Pubkey::find_program_address(
        &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ASSOCIATED_TOKEN_ID,
    )
    .0
}

/// Cast helpers for SPL token state, blanket-implemented for every [`TestSVM`]
/// engine. Import the trait to call them:
///
/// ```ignore
/// use testsvm::token::TokenTestSVM;
/// let mint = backend.prop_mint("USDC", 6, &authority);
/// ```
pub trait TokenTestSVM: TestSVM {
    /// Cast an SPL mint owned by the canonical Token program. Use
    /// [`prop_mint_owned`](Self::prop_mint_owned) to choose the program
    /// (Token-2022, or a Pinocchio token reimplementation).
    fn prop_mint(&mut self, name: &str, decimals: u8, mint_authority: &Pubkey) -> Pubkey {
        self.prop_mint_owned(name, decimals, mint_authority, &SPL_TOKEN_ID)
    }

    /// Cast an SPL mint owned by `token_program`: fabricate an initialized mint
    /// (82-byte layout, hand-packed) at a deterministic named address, and alias
    /// it. The mint has the given `mint_authority`, supply 0, and no freeze
    /// authority. The token analog of [`prop`](crate::TestSVM::prop), and it
    /// shares the cast-name uniqueness guard.
    fn prop_mint_owned(
        &mut self,
        name: &str,
        decimals: u8,
        mint_authority: &Pubkey,
        token_program: &Pubkey,
    ) -> Pubkey {
        // SPL Mint layout: COption<Pubkey> mint_authority (4-byte tag + 32),
        // u64 supply, u8 decimals, bool is_initialized, COption<Pubkey>
        // freeze_authority (4 + 32). Zero-initialized, so supply / None tags /
        // freeze authority are already correct.
        let mut data = vec![0u8; MINT_LEN];
        data[0..4].copy_from_slice(&1u32.to_le_bytes()); // mint_authority = COption::Some
        data[4..36].copy_from_slice(mint_authority.as_ref());
        data[44] = decimals;
        data[45] = 1; // is_initialized = true
        self.prop(
            name,
            Account {
                // (128 storage overhead + 82 mint bytes) * 3480 lamports/byte-year * 2 years
                lamports: 1_461_600,
                data,
                owner: *token_program,
                executable: false,
                rent_epoch: 0,
            },
        )
    }

    /// Cast a funded holder: an initialized SPL token account at the canonical
    /// ATA of `(owner, mint)`, holding `amount`. See
    /// [`prop_token_account_owned`](Self::prop_token_account_owned) to choose the
    /// token program.
    fn prop_token_account(
        &mut self,
        name: &str,
        mint: &Pubkey,
        owner: &Pubkey,
        amount: u64,
    ) -> Pubkey {
        self.prop_token_account_owned(name, mint, owner, amount, &SPL_TOKEN_ID)
    }

    /// Cast a funded holder owned by `token_program`: fabricate an initialized
    /// token account (165-byte layout, hand-packed) at the `(owner, mint)` ATA
    /// and alias it. The fabrication analog of creating-and-minting an ATA: a
    /// holder with `amount` in one call, no real CPI. No delegate, not native,
    /// no close authority. Shares the cast-name guard.
    fn prop_token_account_owned(
        &mut self,
        name: &str,
        mint: &Pubkey,
        owner: &Pubkey,
        amount: u64,
        token_program: &Pubkey,
    ) -> Pubkey {
        let ata = associated_token_address(owner, mint, token_program);
        // SPL token account layout: Pubkey mint, Pubkey owner, u64 amount,
        // COption<Pubkey> delegate, u8 state, COption<u64> is_native, u64
        // delegated_amount, COption<Pubkey> close_authority. Zero-initialized,
        // so the three COption::None tags and delegated_amount are correct.
        let mut data = vec![0u8; TOKEN_ACCOUNT_LEN];
        data[0..32].copy_from_slice(mint.as_ref());
        data[32..64].copy_from_slice(owner.as_ref());
        data[64..72].copy_from_slice(&amount.to_le_bytes());
        data[108] = 1; // state = AccountState::Initialized
        self.prop_at(
            name,
            &ata,
            Account {
                // (128 storage overhead + 165 account bytes) * 3480 lamports/byte-year * 2 years
                lamports: 2_039_280,
                data,
                owner: *token_program,
                executable: false,
                rent_epoch: 0,
            },
        )
    }

    /// Alias the `(owner, mint)` ATA under the composed name `"<owner>/<mint>"`,
    /// drawn from the alias table, and return its address. Name the leaves first
    /// (the owner and the mint), then compose each token-account name off them,
    /// so a rendered trace reads `Alice/USDC` rather than a raw key. Naming only:
    /// for an account the program (or [`prop_token_account`](Self::prop_token_account))
    /// creates. Canonical SPL Token program; see
    /// [`alias_ata_owned`](Self::alias_ata_owned) to choose the program.
    fn alias_ata(&mut self, owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        self.alias_ata_owned(owner, mint, &SPL_TOKEN_ID)
    }

    /// [`alias_ata`](Self::alias_ata) for a chosen `token_program`.
    fn alias_ata_owned(&mut self, owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
        let name = format!("{}/{}", self.label(owner), self.label(mint));
        let ata = associated_token_address(owner, mint, token_program);
        self.register_alias(&ata, &name);
        ata
    }
}

impl<T: TestSVM + ?Sized> TokenTestSVM for T {}
