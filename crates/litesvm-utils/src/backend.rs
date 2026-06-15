//! `TestSVM`: the SVM endpoint a scenario executes against.
//!
//! Today `AnchorContext` is welded to an in-process [`LiteSVM`]. This trait is
//! the seam that lets the *same* scenario run in memory or against an RPC
//! endpoint (surfpool, or any cluster). It produces a [`model::Transaction`]:
//! the structured facts about one transaction that the renderers and
//! assertions consume. The CPI tree is parsed from the logs on either
//! endpoint; the per-frame `InstructionTrace` is the one endpoint-asymmetric
//! field (present in memory, absent over generic RPC until the server
//! surfaces it).
//!
//! See `NOTES/2026-06-10-testsvm-extraction-design.md`: one trait, one unified
//! model, one adapter crate per engine ([`LiteSvmBackend`] + `RpcBackend`
//! here in the litesvm graph; `MolluskBackend` in `testsvm-mollusk`, whose
//! graph carries no litesvm).

use {
    crate::transaction::{TraceHandle, TraceRecorder, TransactionHelpers, TransactionResult},
    litesvm::LiteSVM,
    solana_account::Account,
    solana_keypair::Keypair,
    solana_program::{clock::Clock, instruction::Instruction, pubkey::Pubkey},
    testsvm::{model, Capabilities, TestSVM},
};

/// Lift any engine's transaction into the rich result type, so the aliased
/// renderers (tree, mermaid, authority) work on every engine's output alike.
/// `inner_instructions` stays empty (the CPI structure is in `frames`); the
/// top-level `instruction` is `None` (no decode without the original ix).
impl From<model::Transaction> for TransactionResult {
    fn from(tx: model::Transaction) -> Self {
        let meta = litesvm::types::TransactionMetadata {
            logs: tx.logs,
            compute_units_consumed: tx.compute_units,
            fee: tx.fee.unwrap_or(0),
            ..Default::default()
        };
        let result = match tx.error {
            Some(err) => TransactionResult::new_failed(err, meta, None, tx.message),
            None => TransactionResult::new(meta, None, tx.message),
        };
        // Carry the backend's vocabulary onto the rich result, so a scenario that
        // registered aliases / instruction names / errors / events on the backend
        // gets them in the rendered tree, mermaid, and authority views with no
        // per-result re-attachment. (`register_*` is the single source.)
        result
            .with_instruction_trace(tx.trace)
            .with_aliases(tx.aliases)
            .with_instruction_names(tx.instruction_names)
            .with_error_names(tx.error_names)
            .with_event_registry(tx.events)
    }
}

impl TransactionResult {
    /// Build the engine-neutral [`model::Transaction`] from this litesvm result:
    /// the shared litesvm extraction (CPI tree to frames, then
    /// [`assemble`](model::Transaction::assemble)), so the two litesvm-based
    /// senders ([`LiteSvmBackend`]'s and `AnchorContext`'s `TestSVM::send`)
    /// produce their record one way instead of two. `trace` is the per-frame
    /// privilege trace the sender's recorder captured (logs cannot carry it);
    /// `aliases` and the name tables come from the sender.
    pub fn into_model(
        &self,
        trace: Option<crate::transaction::InstructionTrace>,
        instruction_names: &testsvm::instructions::InstructionNames,
        error_names: &testsvm::errors::ErrorNames,
        aliases: testsvm::aliases::Aliases,
        events: testsvm::events::EventRegistry,
    ) -> model::Transaction {
        model::Transaction::assemble(
            to_frames(litesvm::cpi_tree::cpi_tree(self.logs())),
            self.message.clone(),
            self.logs().to_vec(),
            self.error().cloned(),
            self.compute_units(),
            Some(self.fee()),
            trace,
            None,
            instruction_names,
            error_names,
            aliases,
            events,
        )
    }
}

/// litesvm's native cpi_tree output, converted to the vocabulary. The
/// adapter owns this conversion; litesvm's parser is litesvm's.
fn to_frames(frames: Vec<litesvm::cpi_tree::CpiFrame>) -> Vec<testsvm::frame::Frame> {
    frames
        .into_iter()
        .map(|f| testsvm::frame::Frame {
            program_id: Pubkey::new_from_array(f.program_id.to_bytes()),
            outcome: match f.outcome {
                litesvm::cpi_tree::CpiOutcome::Success => testsvm::frame::Outcome::Success,
                litesvm::cpi_tree::CpiOutcome::Failed { message } => {
                    testsvm::frame::Outcome::Failed { message }
                }
                litesvm::cpi_tree::CpiOutcome::Truncated => testsvm::frame::Outcome::Truncated,
            },
            compute_units: f.compute_units.map(|cu| testsvm::frame::ComputeUnits {
                consumed: cu.consumed,
                available_at_start: cu.available_at_start,
            }),
            instruction_name: f.instruction_name,
            logs: f
                .logs
                .into_iter()
                .map(|l| match l {
                    litesvm::cpi_tree::FrameLog::Msg(s) => testsvm::frame::FrameLog::Msg(s),
                    litesvm::cpi_tree::FrameLog::Data(s) => testsvm::frame::FrameLog::Data(s),
                })
                .collect(),
            children: to_frames(f.children),
        })
        .collect()
}

/// The in-memory adapter: a `LiteSVM` with the inspect-hook trace recorder
/// installed, so every send captures the full execution record (trace included).
pub struct LiteSvmBackend {
    inner: LiteSVM,
    trace: TraceHandle,
    aliases: testsvm::aliases::Aliases,
    instruction_names: testsvm::instructions::InstructionNames,
    error_names: testsvm::errors::ErrorNames,
    events: testsvm::events::EventRegistry,
}

impl LiteSvmBackend {
    /// Wrap a `LiteSVM` and install the trace recorder on its inspect hook.
    pub fn new(mut svm: LiteSVM) -> Self {
        let trace = TraceRecorder::install(&mut svm);
        Self {
            inner: svm,
            trace,
            aliases: testsvm::aliases::Aliases::with_well_known(),
            instruction_names: testsvm::instructions::InstructionNames::new(),
            error_names: testsvm::errors::ErrorNames::new(),
            events: testsvm::events::EventRegistry::new(),
        }
    }

    /// Borrow the inner `LiteSVM` (escape hatch during migration; the consuming
    /// helpers like `create_token_mint` still take `&mut LiteSVM`).
    pub fn svm(&self) -> &LiteSVM {
        &self.inner
    }

    /// Mutably borrow the inner `LiteSVM`.
    pub fn svm_mut(&mut self) -> &mut LiteSVM {
        &mut self.inner
    }
}

impl TestSVM for LiteSvmBackend {
    fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> model::Transaction {
        let result = self
            .inner
            .send_instructions(ixs, signers)
            .expect("LiteSvmBackend::send: transaction build failed");
        let trace = self.trace.take_latest();
        result.into_model(
            trace,
            &self.instruction_names,
            &self.error_names,
            self.aliases.clone(),
            self.events.clone(),
        )
    }

    fn fund_sol(&mut self, address: &Pubkey, lamports: u64) {
        self.inner
            .airdrop(address, lamports)
            .expect("LiteSvmBackend::fund_sol: airdrop failed");
    }

    fn set_account(&mut self, address: &Pubkey, account: Account) {
        self.inner
            .set_account(*address, account)
            .expect("LiteSvmBackend::set_account failed");
    }

    fn account_owner(&self, pubkey: &Pubkey) -> Option<Pubkey> {
        self.inner.get_account(pubkey).map(|a| a.owner)
    }

    fn get_account(&self, pubkey: &Pubkey) -> Option<Account> {
        self.inner.get_account(pubkey)
    }

    fn deploy_program(&mut self, program_id: Pubkey, bytes: &[u8]) {
        self.inner
            .add_program(program_id, bytes)
            .expect("LiteSvmBackend::deploy_program: add_program failed");
    }

    fn warp_to_slot(&mut self, slot: u64) {
        self.inner.warp_to_slot(slot);
    }

    fn warp_to_timestamp(&mut self, unix_timestamp: i64) {
        let mut clock = self.inner.get_sysvar::<Clock>();
        clock.unix_timestamp = unix_timestamp;
        self.inner.set_sysvar(&clock);
    }

    fn clock(&self) -> Clock {
        self.inner.get_sysvar::<Clock>()
    }

    fn register_instruction_name(&mut self, program_id: &Pubkey, prefix: &[u8], name: &str) {
        self.instruction_names.register(*program_id, prefix, name);
    }

    fn register_error_name(&mut self, program_id: &Pubkey, code: u32, name: &str) {
        self.error_names.register(*program_id, code, name);
    }

    /// Recorded in this backend's table and stamped onto every sent
    /// `model::Transaction`. In-memory litesvm has no endpoint-side render;
    /// the consumer-side render (the model's, and the aliased
    /// `TransactionResult` renderers) is where the name shows up.
    fn register_alias(&mut self, pubkey: &Pubkey, name: &str) {
        self.aliases.add(*pubkey, name);
    }

    fn register_event_decoder(
        &mut self,
        discriminator: [u8; 8],
        name: &str,
        decode: testsvm::events::EventDecoder,
    ) {
        self.events.register(discriminator, name, decode);
    }

    fn register_cpi_event(
        &mut self,
        program_id: &Pubkey,
        prefix: &[u8],
        name: &str,
        decode: testsvm::events::EventDecoder,
    ) {
        self.events.register_cpi(*program_id, prefix.to_vec(), name, decode);
    }

    fn register_cast_name(&mut self, name: &str) -> bool {
        self.aliases.register_cast(name)
    }

    fn aliases(&self) -> &testsvm::aliases::Aliases {
        &self.aliases
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            per_frame_trace: true,
            // litesvm's frames go through its own parser, which it owns:
            // structured at the source.
            structured_cpi: true,
            atomic_send: true,
            fees: true,
            instant_reset: false,
            fork: false,
        }
    }
}

// --- RPC adapter (feature `rpc`) -------------------------------------------

#[cfg(feature = "rpc")]
pub use rpc_backend::RpcBackend;

/// The RPC adapter: talks to a surfnet (or any cluster) over JSON-RPC. The CPI
/// tree comes from the returned logs, so it works here exactly as in memory;
/// `trace` stays `None` (a stock RPC never witnessed the per-frame trace,
/// surfpool can surface it later and this flips to `Some`). The surfnet lifecycle
/// (start, deploy) is the caller's job, e.g. via `surfpool-sdk`'s `Surfnet`; this
/// backend only needs the URL.
#[cfg(feature = "rpc")]
mod rpc_backend {
    use {
        solana_account::Account,
        solana_commitment_config::CommitmentConfig,
        solana_keypair::Keypair,
        solana_program::{clock::Clock, instruction::Instruction, pubkey::Pubkey},
        solana_rpc_client::rpc_client::RpcClient,
        solana_signer::Signer,
        solana_transaction::Transaction,
        testsvm::{model, Capabilities, TestSVM},
    };

    pub struct RpcBackend {
        client: RpcClient,
        aliases: testsvm::aliases::Aliases,
        instruction_names: testsvm::instructions::InstructionNames,
        error_names: testsvm::errors::ErrorNames,
        events: testsvm::events::EventRegistry,
    }

    impl RpcBackend {
        /// Connect to a surfnet/cluster at `url` (confirmed commitment).
        pub fn new(url: impl Into<String>) -> Self {
            Self {
                client: RpcClient::new_with_commitment(url.into(), CommitmentConfig::confirmed()),
                aliases: testsvm::aliases::Aliases::with_well_known(),
                instruction_names: testsvm::instructions::InstructionNames::new(),
                error_names: testsvm::errors::ErrorNames::new(),
                events: testsvm::events::EventRegistry::new(),
            }
        }

        /// Borrow the underlying RPC client for reads the trait does not cover.
        pub fn client(&self) -> &RpcClient {
            &self.client
        }
    }

    impl TestSVM for RpcBackend {
        fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> model::Transaction {
            let payer = signers[0];
            let blockhash = self
                .client
                .get_latest_blockhash()
                .expect("RpcBackend::send: get_latest_blockhash");
            let tx =
                Transaction::new_signed_with_payer(ixs, Some(&payer.pubkey()), signers, blockhash);
            let message = tx.message.clone();

            // Simulate to capture logs (the CPI-tree source), compute, and outcome
            // in one call; then, when it would succeed, send-and-confirm so a
            // multi-step scenario sees the persisted effects. (v1: double-exec;
            // a later cut can switch to send + getTransaction for the real meta.)
            let sim = self
                .client
                .simulate_transaction(&tx)
                .expect("RpcBackend::send: simulate_transaction")
                .value;
            let logs = sim.logs.unwrap_or_default();
            let compute_units = sim.units_consumed.unwrap_or(0);
            let error = sim.err.map(|e| e.to_string());

            if error.is_none() {
                self.client
                    .send_and_confirm_transaction(&tx)
                    .expect("RpcBackend::send: send_and_confirm_transaction");
            }

            model::Transaction::assemble(
                testsvm::frame::frames_from_logs(&logs),
                message,
                logs,
                error,
                compute_units,
                // Fee is not surfaced by simulate; absent, not zero. A later cut
                // reads it from getTransaction.
                None,
                // No per-frame trace over a stock RPC; surfpool surfacing the record
                // is what flips this to `Some` (and `capabilities().per_frame_trace`).
                None,
                None,
                &self.instruction_names,
                &self.error_names,
                self.aliases.clone(),
                self.events.clone(),
            )
        }

        fn fund_sol(&mut self, address: &Pubkey, lamports: u64) {
            self.client
                .request_airdrop(address, lamports)
                .expect("RpcBackend::fund_sol: request_airdrop");
        }

        fn set_account(&mut self, address: &Pubkey, account: Account) {
            use {serde_json::Value, solana_rpc_client_api::request::RpcRequest};
            // surfpool's surfnet_setAccount cheatcode; `data` is a HEX string.
            // Best-effort like register_alias: a non-surfnet RPC won't know it.
            let hex: String = account.data.iter().map(|b| format!("{b:02x}")).collect();
            let _ = self.client.send::<Value>(
                RpcRequest::Custom {
                    method: "surfnet_setAccount",
                },
                serde_json::json!([
                    address.to_string(),
                    {
                        "lamports": account.lamports,
                        "data": hex,
                        "owner": account.owner.to_string(),
                        "executable": account.executable,
                        "rentEpoch": account.rent_epoch,
                    }
                ]),
            );
        }

        fn account_owner(&self, pubkey: &Pubkey) -> Option<Pubkey> {
            self.client.get_account(pubkey).ok().map(|a| a.owner)
        }

        fn get_account(&self, pubkey: &Pubkey) -> Option<Account> {
            self.client.get_account(pubkey).ok()
        }

        fn deploy_program(&mut self, _program_id: Pubkey, _bytes: &[u8]) {
            // Deploying over RPC (BPF loader writes, or a surfpool cheatcode) is the
            // caller's job for v1, e.g. surfpool-sdk `cheatcodes().deploy_program`
            // before constructing this backend. Kept out of the generic adapter.
            unimplemented!(
                "RpcBackend::deploy_program: deploy via the surfnet/cluster first \
                 (e.g. surfpool-sdk cheatcodes), then point RpcBackend at its URL"
            )
        }

        fn warp_to_slot(&mut self, slot: u64) {
            use {serde_json::Value, solana_rpc_client_api::request::RpcRequest};
            // surfpool's TimeTravelConfig, externally tagged camelCase.
            // Best-effort like register_alias: a non-surfnet RPC won't know it.
            let _ = self.client.send::<Value>(
                RpcRequest::Custom {
                    method: "surfnet_timeTravel",
                },
                serde_json::json!([{ "absoluteSlot": slot }]),
            );
        }

        fn warp_to_timestamp(&mut self, unix_timestamp: i64) {
            use {serde_json::Value, solana_rpc_client_api::request::RpcRequest};
            // surfpool's AbsoluteTimestamp is in MILLISECONDS (it divides by
            // 1000 into the clock's unix_timestamp); the trait speaks seconds.
            let _ = self.client.send::<Value>(
                RpcRequest::Custom {
                    method: "surfnet_timeTravel",
                },
                serde_json::json!([{ "absoluteTimestamp": (unix_timestamp as u64) * 1000 }]),
            );
        }

        fn clock(&self) -> Clock {
            // The Clock sysvar account's data is bincode-encoded.
            self.client
                .get_account(&solana_program::sysvar::clock::id())
                .ok()
                .and_then(|a| bincode::deserialize(&a.data).ok())
                .unwrap_or_default()
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities {
                per_frame_trace: false,
                structured_cpi: false,
                atomic_send: true,
                fees: false,
                instant_reset: false,
                fork: true,
            }
        }

        fn register_instruction_name(&mut self, program_id: &Pubkey, prefix: &[u8], name: &str) {
            self.instruction_names.register(*program_id, prefix, name);
        }

        fn register_error_name(&mut self, program_id: &Pubkey, code: u32, name: &str) {
            self.error_names.register(*program_id, code, name);
        }

        fn register_event_decoder(
            &mut self,
            discriminator: [u8; 8],
            name: &str,
            decode: testsvm::events::EventDecoder,
        ) {
            self.events.register(discriminator, name, decode);
        }

        fn register_cpi_event(
            &mut self,
            program_id: &Pubkey,
            prefix: &[u8],
            name: &str,
            decode: testsvm::events::EventDecoder,
        ) {
            self.events.register_cpi(*program_id, prefix.to_vec(), name, decode);
        }

        /// Recorded in this backend's table (stamped onto every sent
        /// `model::Transaction`) AND pushed to the surfnet's own render via
        /// `surfnet_registerAlias`, best-effort: a non-surfnet RPC simply
        /// ignores the unknown method.
        fn register_alias(&mut self, pubkey: &Pubkey, name: &str) {
            use {serde_json::Value, solana_rpc_client_api::request::RpcRequest};
            // Record for the model's own render...
            self.aliases.add(*pubkey, name);
            // ...and push to the endpoint's render too.
            // Push to surfpool's render alias map; best-effort (ignore the result,
            // a non-surfnet RPC simply won't know the method).
            let _ = self.client.send::<Value>(
                RpcRequest::Custom {
                    method: "surfnet_registerAlias",
                },
                Value::Array(vec![
                    Value::String(pubkey.to_string()),
                    Value::String(name.to_string()),
                ]),
            );
        }

        fn register_cast_name(&mut self, name: &str) -> bool {
            self.aliases.register_cast(name)
        }

        fn aliases(&self) -> &testsvm::aliases::Aliases {
            &self.aliases
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, solana_signer::Signer};

    #[test]
    fn distinct_casts_share_one_namespace() {
        // actor and prop draw from the same cast-name guard; distinct names are fine.
        let mut backend = LiteSvmBackend::new(LiteSVM::new());
        backend.actor("alice", 1_000_000_000);
        backend.actor("bob", 1_000_000_000);
        backend.prop("mint", solana_account::Account::default());
    }

    #[test]
    #[should_panic(expected = "already used in this scenario")]
    fn actor_rejects_a_duplicate_cast_name() {
        let mut backend = LiteSvmBackend::new(LiteSVM::new());
        backend.actor("alice", 1_000_000_000);
        backend.actor("alice", 1_000_000_000);
    }

    #[test]
    #[should_panic(expected = "already used in this scenario")]
    fn prop_collides_with_an_actor_name() {
        // The guard is shared across the cast vocabulary, not per-method.
        let mut backend = LiteSvmBackend::new(LiteSVM::new());
        backend.actor("shared", 1_000_000_000);
        backend.prop("shared", solana_account::Account::default());
    }

    #[test]
    fn prop_mint_packs_bytes_that_spl_token_reads() {
        use {
            solana_program::{program_option::COption, program_pack::Pack},
            spl_token::state::Mint,
            testsvm::token::TokenTestSVM,
        };
        let mut backend = LiteSvmBackend::new(LiteSVM::new());
        let authority = Pubkey::new_unique();
        let mint = backend.prop_mint("USDC", 6, &authority);

        let account = backend.get_account(&mint).expect("mint was cast");
        assert_eq!(account.owner, testsvm::token::SPL_TOKEN_ID);
        // The dependency-free hand-packing must round-trip through spl-token's
        // own reader: that cross-check is what lets testsvm own the layout.
        let parsed = Mint::unpack(&account.data).expect("valid SPL mint layout");
        assert_eq!(parsed.decimals, 6);
        assert!(parsed.is_initialized);
        assert_eq!(parsed.supply, 0);
        assert_eq!(parsed.mint_authority, COption::Some(authority));
        assert_eq!(parsed.freeze_authority, COption::None);
    }

    #[test]
    fn prop_token_account_packs_bytes_that_spl_token_reads() {
        use {
            solana_program::program_pack::Pack,
            spl_token::state::{Account as TokenAccount, AccountState},
            testsvm::token::{associated_token_address, TokenTestSVM, SPL_TOKEN_ID},
        };
        let mut backend = LiteSvmBackend::new(LiteSVM::new());
        let mint = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let ata = backend.prop_token_account("alice.usdc", &mint, &owner, 1_000_000);

        // Cast at the canonical ATA, not a name-derived address.
        assert_eq!(ata, associated_token_address(&owner, &mint, &SPL_TOKEN_ID));
        let account = backend.get_account(&ata).expect("holder was cast");
        let parsed = TokenAccount::unpack(&account.data).expect("valid SPL token account");
        assert_eq!(parsed.mint, mint);
        assert_eq!(parsed.owner, owner);
        assert_eq!(parsed.amount, 1_000_000);
        assert_eq!(parsed.state, AccountState::Initialized);
    }

    #[test]
    fn label_and_alias_ata_name_accounts_at_the_trait() {
        use testsvm::token::{associated_token_address, TokenTestSVM, SPL_TOKEN_ID};
        let mut backend = LiteSvmBackend::new(LiteSVM::new());
        let alice = backend.actor("Alice", 1_000_000_000);
        let mint = backend.prop_mint("USDC", 6, &alice.pubkey());

        // label() resolves a registered name, and falls back to a short form otherwise.
        assert_eq!(backend.label(&alice.pubkey()), "Alice");
        assert_ne!(backend.label(&Pubkey::new_unique()), "Alice");

        // alias_ata composes the holder name off the leaves.
        let ata = backend.alias_ata(&alice.pubkey(), &mint);
        assert_eq!(
            ata,
            associated_token_address(&alice.pubkey(), &mint, &SPL_TOKEN_ID)
        );
        assert_eq!(backend.label(&ata), "Alice/USDC");
    }

    #[test]
    fn litesvm_backend_records_a_send_with_trace() {
        let mut backend = LiteSvmBackend::new(LiteSVM::new());
        let payer = Keypair::new();
        backend.fund_sol(&payer.pubkey(), 1_000_000_000);

        // One top-level System transfer: a frame the trace and logs both witness.
        let dest = Pubkey::new_unique();
        // Above the rent-exemption minimum so the destination account persists.
        let ix = solana_system_interface::instruction::transfer(&payer.pubkey(), &dest, 2_000_000);
        let record = backend.send(&[ix], &[&payer]);

        assert!(
            record.error.is_none(),
            "transfer should succeed: {:?}",
            record.error
        );
        assert!(
            record.trace.is_some(),
            "the in-memory backend captures the per-frame trace"
        );
        assert!(record.compute_units > 0, "a real send consumes compute");
        assert_eq!(
            backend.account_owner(&dest),
            Some(solana_system_interface::program::id()),
            "the funded destination is owned by the system program"
        );
        assert!(backend.capabilities().per_frame_trace);
    }

    #[test]
    fn identical_sends_are_fresh_by_default() {
        // The repeated-send pattern (rate limits, spend caps): an identical
        // instruction resent in a loop. The helpers refresh the blockhash
        // before signing, so no send collides with its predecessor's
        // signature and nobody performs the expire_blockhash ritual.
        let mut backend = LiteSvmBackend::new(LiteSVM::new());
        let payer = Keypair::new();
        backend.fund_sol(&payer.pubkey(), 10_000_000_000);
        let dest = Pubkey::new_unique();
        for i in 0..3 {
            let ix =
                solana_system_interface::instruction::transfer(&payer.pubkey(), &dest, 2_000_000);
            let tx = backend.send(&[ix], &[&payer]);
            assert!(
                tx.error.is_none(),
                "send #{i} should be fresh: {:?}",
                tx.error
            );
        }
    }

    #[test]
    fn clock_levers_move_the_clock_cross_engine() {
        let mut backend = LiteSvmBackend::new(LiteSVM::new());
        backend.warp_to_slot(500);
        assert_eq!(backend.clock().slot, 500);
        backend.warp_to_timestamp(1_700_000_000);
        assert_eq!(backend.clock().unix_timestamp, 1_700_000_000);
    }

    #[test]
    fn litesvm_capabilities_are_declared() {
        let backend = LiteSvmBackend::new(LiteSVM::new());
        let caps = backend.capabilities();
        assert!(caps.per_frame_trace);
        assert!(caps.structured_cpi);
        assert!(caps.atomic_send);
        assert!(caps.fees);
    }

    #[test]
    fn record_renders_through_the_aliased_consumer() {
        use crate::{Aliases, TransactionResult};

        let mut backend = LiteSvmBackend::new(LiteSVM::new());
        let payer = Keypair::new();
        backend.fund_sol(&payer.pubkey(), 1_000_000_000);
        let dest = Pubkey::new_unique();
        let ix = solana_system_interface::instruction::transfer(&payer.pubkey(), &dest, 2_000_000);
        let record = backend.send(&[ix], &[&payer]);

        // The bridge: any backend's transaction -> TransactionResult -> the aliased
        // renderer. The System program renders by its well-known name, not the raw
        // pubkey, proving consumer naming reaches the backend's output.
        let result: TransactionResult = record.into();
        let rendered = result
            .with_aliases(Aliases::with_well_known())
            .logs_structured_string();
        assert!(
            rendered.contains("System"),
            "aliased render should name the System program:\n{rendered}"
        );
    }
}
