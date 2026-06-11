use crate::account::AccountError;
use crate::program::Program;
use anchor_lang::AccountDeserialize;
use litesvm::LiteSVM;
use litesvm_utils::{Aliases, InstructionInfo, TransactionHelpers, TransactionResult};
use solana_sdk::hash::Hash;
use solana_sdk::signer::keypair::Keypair;
use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;

/// Production-compatible testing context for Anchor programs.
///
/// Provides the exact same API as anchor-client but works directly with LiteSVM,
/// eliminating RPC overhead while maintaining identical syntax for tests and production.
pub struct AnchorContext {
    /// Direct access to the underlying LiteSVM instance
    pub svm: LiteSVM,
    /// The Anchor program ID
    pub program_id: Pubkey,
    /// Pubkey-to-friendly-name table used by the context-level
    /// [`send_ok`](Self::send_ok) / [`send_err`](Self::send_err) /
    /// [`send_err_named`](Self::send_err_named) helpers and stashed on
    /// returned [`TransactionResult`]s so chained
    /// `print_logs_structured()` calls read it implicitly. Extend via
    /// [`alias`](Self::alias).
    pub aliases: Aliases,
    /// The payer keypair
    payer: Keypair,
    /// The program instance for instruction building
    program: Program,
}

impl AnchorContext {
    /// Create a new AnchorContext with an existing LiteSVM instance
    ///
    /// Note: This creates a default payer and funds it. For more control,
    /// use AnchorLiteSVM builder.
    ///
    /// # Example
    /// ```no_run
    /// use litesvm::LiteSVM;
    /// use anchor_litesvm::AnchorContext;
    /// use solana_program::pubkey::Pubkey;
    ///
    /// let mut svm = LiteSVM::new();
    /// let program_id = Pubkey::new_unique();
    /// let ctx = AnchorContext::new(svm, program_id);
    /// ```
    pub fn new(mut svm: LiteSVM, program_id: Pubkey) -> Self {
        // Create a default payer and fund it
        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

        let program = Program::new(program_id);

        Self {
            svm,
            program_id,
            aliases: Aliases::default(),
            payer,
            program,
        }
    }

    /// Create a new AnchorContext with a specific payer
    pub(crate) fn new_with_payer(svm: LiteSVM, program_id: Pubkey, payer: Keypair) -> Self {
        let program = Program::new(program_id);

        Self {
            svm,
            program_id,
            aliases: Aliases::default(),
            payer,
            program,
        }
    }

    /// Get a copy of the program instance for building instructions.
    ///
    /// Simplified API for testing without RPC overhead:
    ///
    /// # Example
    /// ```ignore
    /// let ix = ctx.program()
    ///     .accounts(my_program::client::accounts::MyInstruction { ... })
    ///     .args(my_program::client::args::MyInstruction { ... })
    ///     .instruction()?;
    /// ```
    pub fn program(&self) -> Program {
        self.program
    }

    /// Get the payer keypair
    pub fn payer(&self) -> &Keypair {
        &self.payer
    }

    /// Register `pubkey -> label` in the context's alias table. Later
    /// inserts shadow earlier ones, so this also serves as a rename when
    /// an actor's role changes mid-test (e.g. authority rotation).
    /// Feed a `(pubkey, name)` program table into the alias layer: the
    /// consumption end of the `BundledPubkeys` structural rule's generated
    /// `injected_programs()` (and any other table of the same shape), the
    /// way `register_program_instructions` consumes the Discriminator
    /// tables. `ctx.alias_programs(&Make::injected_programs())` and every
    /// injected program renders named with zero per-program registration.
    pub fn alias_programs(&mut self, table: &[(Pubkey, &str)]) -> &mut Self {
        for (pubkey, name) in table {
            self.alias(*pubkey, *name);
        }
        self
    }

    pub fn alias(&mut self, pubkey: Pubkey, label: impl Into<String>) -> &mut Self {
        self.aliases.add(pubkey, label);
        self
    }

    /// Resolve `pubkey` to its registered alias, or a short `<8>…<4>` form
    /// when it isn't aliased. Shorthand for `self.aliases.label(&pubkey)`.
    ///
    /// Built for report rows: alias the accounts a scenario names (actors,
    /// PDAs), then drop `ctx.label(&pk)` straight into a
    /// [`md_table!`](crate::md_table) / [`md_kv!`](crate::md_kv) cell instead
    /// of hand-rolling a pubkey-to-name match.
    pub fn label(&self, pubkey: &Pubkey) -> String {
        self.aliases.label(pubkey)
    }

    /// Start a fluent [`Tx`](crate::tx::Tx) chain: build + send +
    /// expect in one statement, with the success and negative paths
    /// sharing every step up to the terminator. Replaces the per-verb
    /// `_ok`/`_expecting` pair that hand-rolled helpers tend to grow.
    ///
    /// ```ignore
    /// ctx.tx(&[&signer])
    ///    .build(SwapBundle::from((&pool, &user)), instruction::Swap { kind, dir })
    ///    .send_ok()
    ///    .print_logs_structured();
    /// ```
    pub fn tx<'a>(&'a mut self, signers: &'a [&'a Keypair]) -> crate::tx::Tx<'a> {
        crate::tx::Tx::new(self, signers)
    }

    /// Send an ix expected to succeed, with structured-log aliases drawn
    /// from `self.aliases`. Returned [`TransactionResult`] carries the
    /// aliases internally, so `.print_logs_structured()` works with no
    /// argument. Thin wrapper over
    /// [`TransactionHelpers::send_ok`](litesvm_utils::TransactionHelpers::send_ok)
    /// that removes the per-call `&Aliases` thread.
    pub fn send_ok(&mut self, ix: Instruction, signers: &[&Keypair]) -> TransactionResult {
        self.svm.send_ok(ix, signers, &self.aliases)
    }

    /// Send an ix expected to fail (any error). Aliases drawn from
    /// `self.aliases`. Companion to [`send_ok`](Self::send_ok).
    pub fn send_err(&mut self, ix: Instruction, signers: &[&Keypair]) -> TransactionResult {
        self.svm.send_err(ix, signers, &self.aliases)
    }

    /// Send an ix expected to fail with `error_name` (substring matched
    /// against logs and the error field). Aliases drawn from
    /// `self.aliases`. Companion to [`send_ok`](Self::send_ok).
    pub fn send_err_named(
        &mut self,
        ix: Instruction,
        signers: &[&Keypair],
        error_name: &str,
    ) -> TransactionResult {
        self.svm
            .send_err_named(ix, signers, &self.aliases, error_name)
    }

    /// Execute a single instruction using LiteSVM
    ///
    /// This is a convenience method for executing instructions.
    ///
    /// # Example
    /// ```ignore
    /// let ix = ctx.program()
    ///     .request()
    ///     .accounts(...)
    ///     .args(...)
    ///     .instructions()?[0];
    ///
    /// ctx.execute_instruction(ix, &[&signer])?;
    /// ```
    pub fn execute_instruction(
        &mut self,
        instruction: solana_program::instruction::Instruction,
        signers: &[&Keypair],
    ) -> Result<TransactionResult, Box<dyn std::error::Error>> {
        // Determine the payer - use the first signer if provided, otherwise use the context's payer
        let payer_pubkey = if !signers.is_empty() {
            signers[0].pubkey()
        } else {
            self.payer.pubkey()
        };

        // Capture the ix info for the structured-logs header before the
        // transaction below borrows `instruction`. `from_instruction`
        // clones only the data bytes, which is what we need anyway.
        let info = InstructionInfo::from_instruction(&instruction);
        // Build and sign the transaction
        let tx = Transaction::new_signed_with_payer(
            std::slice::from_ref(&instruction),
            Some(&payer_pubkey),
            signers,
            self.svm.latest_blockhash(),
        );
        let message = tx.message.clone();

        // Execute the transaction
        match self.svm.send_transaction(tx) {
            Ok(result) => Ok(TransactionResult::new(result, Some(info), message)),
            Err(failed) => Ok(TransactionResult::new_failed(
                format!("{:?}", failed.err),
                failed.meta,
                Some(info),
                message,
            )),
        }
    }

    /// Execute multiple instructions in a single transaction
    pub fn execute_instructions(
        &mut self,
        instructions: Vec<solana_program::instruction::Instruction>,
        signers: &[&Keypair],
    ) -> Result<TransactionResult, Box<dyn std::error::Error>> {
        // Determine the payer
        let payer_pubkey = if !signers.is_empty() {
            signers[0].pubkey()
        } else {
            self.payer.pubkey()
        };

        // Build and sign the transaction
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer_pubkey),
            signers,
            self.svm.latest_blockhash(),
        );
        let message = tx.message.clone();

        // Execute the transaction
        match self.svm.send_transaction(tx) {
            Ok(result) => Ok(TransactionResult::new(result, None, message)),
            Err(failed) => Ok(TransactionResult::new_failed(
                format!("{:?}", failed.err),
                failed.meta,
                None,
                message,
            )),
        }
    }

    /// Send and confirm a transaction (convenience method)
    pub fn send_and_confirm_transaction(
        &mut self,
        transaction: &Transaction,
    ) -> Result<Signature, Box<dyn std::error::Error>> {
        match self.svm.send_transaction(transaction.clone()) {
            Ok(_) => Ok(transaction.signatures[0]),
            Err(e) => Err(format!("Transaction failed: {:?}", e).into()),
        }
    }

    /// Get an Anchor account from the blockchain
    ///
    /// This fetches and deserializes an Anchor account from the current state.
    ///
    /// # Example
    /// ```no_run
    /// # use anchor_litesvm::AnchorContext;
    /// # use litesvm::LiteSVM;
    /// # use solana_program::pubkey::Pubkey;
    /// # use anchor_lang::AccountDeserialize;
    /// # let svm = LiteSVM::new();
    /// # let program_id = Pubkey::new_unique();
    /// # let ctx = AnchorContext::new(svm, program_id);
    /// # struct MyAccount {}
    /// # impl AccountDeserialize for MyAccount {
    /// #     fn try_deserialize(buf: &mut &[u8]) -> Result<Self, anchor_lang::error::Error> {
    /// #         Ok(MyAccount {})
    /// #     }
    /// #     fn try_deserialize_unchecked(buf: &mut &[u8]) -> Result<Self, anchor_lang::error::Error> {
    /// #         Ok(MyAccount {})
    /// #     }
    /// # }
    /// let account_pubkey = Pubkey::new_unique();
    /// let account: MyAccount = ctx.get_account(&account_pubkey).unwrap();
    /// ```
    pub fn get_account<T>(&self, address: &Pubkey) -> Result<T, AccountError>
    where
        T: AccountDeserialize,
    {
        let account_data = self
            .svm
            .get_account(address)
            .ok_or(AccountError::AccountNotFound(*address))?;

        // Deserialize the account data
        let mut data = account_data.data.as_slice();
        T::try_deserialize(&mut data).map_err(|e| AccountError::DeserializationError(e.to_string()))
    }

    /// Get an Anchor account without discriminator check
    ///
    /// Use this for accounts that don't have the standard Anchor discriminator.
    ///
    /// Note: `try_deserialize_unchecked` handles the discriminator internally,
    /// so we pass the full account data.
    pub fn get_account_unchecked<T>(&self, address: &Pubkey) -> Result<T, AccountError>
    where
        T: AccountDeserialize,
    {
        let account_data = self
            .svm
            .get_account(address)
            .ok_or(AccountError::AccountNotFound(*address))?;

        // Deserialize without discriminator check
        // Note: try_deserialize_unchecked handles the discriminator internally
        let mut data = account_data.data.as_slice();
        T::try_deserialize_unchecked(&mut data)
            .map_err(|e| AccountError::DeserializationError(e.to_string()))
    }

    /// Load an Anchor account, panicking on failure.
    ///
    /// Test-oriented sibling of [`get_account`](Self::get_account): the same fetch
    /// and deserialization, but failures (missing account, wrong discriminator,
    /// deser error) panic with the address and underlying [`AccountError`] in the
    /// message instead of returning a `Result`. Use in tests where a missing or
    /// malformed account is itself a test failure.
    ///
    /// # Example
    /// ```ignore
    /// let escrow: Escrow = ctx.load(&accs.escrow);
    /// assert_eq!(escrow.expiry_utc, Some(expiry));
    /// ```
    pub fn load<T>(&self, address: &Pubkey) -> T
    where
        T: AccountDeserialize,
    {
        self.get_account(address)
            .unwrap_or_else(|e| panic!("failed to load account at {address}: {e}"))
    }

    /// Load an Anchor account without discriminator check, panicking on failure.
    ///
    /// Test-oriented sibling of [`get_account_unchecked`](Self::get_account_unchecked).
    /// Same panic semantics as [`load`](Self::load).
    pub fn load_unchecked<T>(&self, address: &Pubkey) -> T
    where
        T: AccountDeserialize,
    {
        self.get_account_unchecked(address)
            .unwrap_or_else(|e| panic!("failed to load account at {address}: {e}"))
    }

    /// Create a funded account (convenience method)
    pub fn create_funded_account(
        &mut self,
        lamports: u64,
    ) -> Result<Keypair, Box<dyn std::error::Error>> {
        let account = Keypair::new();
        self.svm
            .airdrop(&account.pubkey(), lamports)
            .map_err(|e| format!("Airdrop failed: {:?}", e))?;
        Ok(account)
    }

    /// Airdrop lamports to an account (convenience method)
    pub fn airdrop(
        &mut self,
        pubkey: &Pubkey,
        lamports: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.svm
            .airdrop(pubkey, lamports)
            .map_err(|e| format!("Airdrop failed: {:?}", e))?;
        Ok(())
    }

    /// Get the latest blockhash
    pub fn latest_blockhash(&self) -> Hash {
        self.svm.latest_blockhash()
    }

    /// Check if an account exists
    pub fn account_exists(&self, pubkey: &Pubkey) -> bool {
        self.svm.get_account(pubkey).is_some()
    }
}
