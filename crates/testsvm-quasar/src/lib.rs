//! The quasar adapter: Blueshift's `quasar-svm` behind the `TestSVM` port.
//! `quasar-svm` is an Agave-native local SVM (it builds straight on
//! `solana-program-runtime` + `solana-bpf-loader-program`), a peer engine to
//! litesvm and mollusk rather than a wrapper over either. State lives in the
//! engine's own account `HashMap`; logs come back in-band on every
//! `ExecutionResult` (the engine drains its own `LogCollector`), so unlike the
//! mollusk adapter there is no collector to install. Those logs feed the
//! canonical vendored parser in `testsvm::frame`.
//!
//! A multi-instruction `send` compiles to one sanitized message and runs under
//! a shared budget, so `atomic_send: true` (where mollusk chains state without
//! atomicity). No fees (quasar is signature-less), but quasar records its own
//! per-frame execution trace, which this adapter maps to the neutral
//! `InstructionTrace`, so the authority and ownership views render in full
//! rather than degrading like mollusk's. This crate's dependency graph carries
//! NO litesvm and NO mollusk: same test, different backend, rebuild.

use {
    quasar_svm::{loader_keys, Account as QuasarAccount, QuasarSvm},
    solana_account::Account,
    solana_clock::Clock,
    solana_instruction::Instruction,
    solana_keypair::Keypair,
    solana_message::Message,
    solana_pubkey::Pubkey,
    testsvm::{model, Capabilities, TestSVM},
};

pub struct QuasarBackend {
    svm: QuasarSvm,
    aliases: testsvm::aliases::Aliases,
    instruction_names: testsvm::instructions::InstructionNames,
    error_names: testsvm::errors::ErrorNames,
    events: testsvm::events::EventRegistry,
}

impl Default for QuasarBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl QuasarBackend {
    /// Borrow the underlying `QuasarSvm` (escape hatch for engine-native setup
    /// the trait does not cover, e.g. `set_token_balance`, `create_account`, or
    /// reading `sysvars` directly). Mirrors `LiteSvmBackend::svm()` and
    /// `MolluskBackend::ctx()`.
    pub fn svm(&self) -> &QuasarSvm {
        &self.svm
    }

    /// Mutably borrow the underlying `QuasarSvm`.
    pub fn svm_mut(&mut self) -> &mut QuasarSvm {
        &mut self.svm
    }

    pub fn new() -> Self {
        Self {
            // `new()` preloads the SPL token / token-2022 / ATA programs, the
            // same well-known set litesvm and mollusk seed.
            svm: QuasarSvm::new(),
            aliases: testsvm::aliases::Aliases::with_well_known(),
            instruction_names: testsvm::instructions::InstructionNames::new(),
            error_names: testsvm::errors::ErrorNames::new(),
            events: testsvm::events::EventRegistry::new(),
        }
    }
}

impl TestSVM for QuasarBackend {
    fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> model::Transaction {
        // quasar-svm only deconstructs and commits accounts that were in its
        // input set (stored accounts + the explicit `accounts` arg): a brand-new
        // account gets a default inside the transaction context and the
        // execution sees it, but it is never written back. So pre-register every
        // WRITABLE account an instruction touches that the store doesn't already
        // hold. A default account is zero-lamports and system-owned, and a
        // program is never writable, so this can't shadow a cached program. This
        // is the adapter bridging quasar's caller-provides-accounts model to the
        // port's "the engine owns the state" contract (mollusk's AccountStore
        // does this auto-persist for us; quasar does not).
        let mut seen = std::collections::HashSet::new();
        let new_writable: Vec<Pubkey> = ixs
            .iter()
            .flat_map(|ix| ix.accounts.iter())
            .filter(|meta| meta.is_writable && seen.insert(meta.pubkey))
            .map(|meta| meta.pubkey)
            .filter(|pk| self.svm.get_account(pk).is_none())
            .collect();
        for pk in new_writable {
            self.svm
                .set_account(QuasarAccount::from_pair(pk, Account::default()));
        }

        // State lives in the engine's own account store; pass no extra accounts
        // to merge. A single ix or many compile to one sanitized message either
        // way (atomic, budget-shared), so the chain entry covers both. Signers
        // are not consulted: like mollusk, sigverify is off.
        let result = self.svm.process_instruction_chain(ixs, &[]);
        let logs = result.logs.clone();

        // The model wants the legacy Message for top-level signer/writable
        // facts; build it from the instructions (signers[0] = fee payer).
        let payer = signers.first().map(|kp| {
            use solana_signer::Signer;
            kp.pubkey()
        });
        let message = Message::new(ixs, payer.as_ref());
        let frames = testsvm::frame::frames_from_logs(&logs);
        let error = result.raw_result.as_ref().err().map(|e| e.to_string());
        let return_data = (!result.return_data.is_empty()).then(|| result.return_data.clone());

        // quasar-svm already records a per-frame execution trace (stack depth,
        // program, per-account signer/writable, instruction data) off its Agave
        // `TransactionContext`; map it to the engine-neutral `InstructionTrace`
        // so the authority view lights up. The one field quasar's trace omits is
        // each account's owner, which the ownership view needs: read it back
        // from the committed store (the post-execution owner, exactly what
        // `fill_owners` reads on litesvm). quasar's stack depth is 0-based
        // (0 = top level); the neutral trace follows Agave's 1-based stack
        // height, so add one.
        let trace = testsvm::trace::InstructionTrace(
            result
                .execution_trace
                .instructions
                .iter()
                .map(|ei| testsvm::trace::TracedInstruction {
                    program_id: ei.instruction.program_id,
                    stack_height: ei.stack_depth as usize + 1,
                    accounts: ei
                        .instruction
                        .accounts
                        .iter()
                        .map(|meta| testsvm::trace::TracedAccount {
                            pubkey: meta.pubkey,
                            is_signer: meta.is_signer,
                            is_writable: meta.is_writable,
                            owner: self
                                .svm
                                .get_account(&meta.pubkey)
                                .map(|a| a.owner)
                                .unwrap_or_default(),
                        })
                        .collect(),
                    data: ei.instruction.data.clone(),
                })
                .collect(),
        );

        model::Transaction::assemble(
            frames,
            message,
            logs,
            error,
            result.compute_units_consumed,
            None, // quasar-svm is signature-less; it models no fee
            Some(trace),
            return_data,
            &self.instruction_names,
            &self.error_names,
            self.aliases.clone(),
            // quasar-svm emits events as `Program data:` logs (sol_log_data);
            // the registry the test populated rides along so the rendered tree
            // decodes them to `🔔 Name { .. }`.
            self.events.clone(),
        )
    }

    fn set_account(&mut self, address: &Pubkey, account: Account) {
        self.svm
            .set_account(QuasarAccount::from_pair(*address, account));
    }

    fn fund_sol(&mut self, address: &Pubkey, lamports: u64) {
        self.svm.airdrop(address, lamports);
    }

    fn get_account(&self, pubkey: &Pubkey) -> Option<Account> {
        self.svm.get_account(pubkey).map(|a| a.to_pair().1)
    }

    fn account_owner(&self, pubkey: &Pubkey) -> Option<Pubkey> {
        self.get_account(pubkey).map(|a| a.owner)
    }

    fn deploy_program(&mut self, program_id: Pubkey, bytes: &[u8]) {
        // The runtime loader takes `&self` (the program cache is interior-mutable),
        // so a foreign program drops in without rebuilding the VM. LOADER_V3 is
        // the upgradeable loader quasar's own `with_program` convenience uses.
        self.svm
            .add_program(&program_id, &loader_keys::LOADER_V3, bytes);
    }

    fn warp_to_slot(&mut self, slot: u64) {
        // No `warp_to_slot` lever on the engine, but `sysvars` is public: the
        // Clock sysvar is resolved fresh from here on every send.
        self.svm.sysvars.clock.slot = slot;
    }

    fn warp_to_timestamp(&mut self, unix_timestamp: i64) {
        self.svm.warp_to_timestamp(unix_timestamp);
    }

    fn clock(&self) -> Clock {
        self.svm.sysvars.clock.clone()
    }

    fn register_instruction_name(&mut self, program_id: &Pubkey, prefix: &[u8], name: &str) {
        self.instruction_names.register(*program_id, prefix, name);
    }

    fn register_error_name(&mut self, program_id: &Pubkey, code: u32, name: &str) {
        self.error_names.register(*program_id, code, name);
    }

    /// Recorded in this backend's table and stamped onto every sent
    /// `model::Transaction`, so `pretty_cpi_tree` names it. quasar-svm has no
    /// endpoint-side render to push to; recording is the whole job here.
    fn register_alias(&mut self, pubkey: &Pubkey, name: &str) {
        self.aliases.add(*pubkey, name);
    }

    fn register_logged_event(
        &mut self,
        prefix: &[u8],
        name: &str,
        decode: testsvm::events::EventDecoder,
    ) {
        self.events.register_logged(prefix.to_vec(), name, decode);
    }

    fn register_cast_name(&mut self, name: &str) -> bool {
        self.aliases.register_cast(name)
    }

    fn aliases(&self) -> &testsvm::aliases::Aliases {
        &self.aliases
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            per_frame_trace: true, // mapped from quasar-svm's own ExecutionTrace
            structured_cpi: false, // v1: frames via the canonical log parse
            atomic_send: true,     // one sanitized message, shared budget
            fees: false,
            instant_reset: true,
            fork: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, solana_signer::Signer};

    #[test]
    fn quasar_backend_sends_a_system_transfer() {
        let mut backend = QuasarBackend::new();
        let payer = Keypair::new();
        backend.fund_sol(&payer.pubkey(), 1_000_000_000);

        let dest = Pubkey::new_unique();
        let ix = solana_system_interface::instruction::transfer(&payer.pubkey(), &dest, 2_000_000);
        let tx = backend.send(&[ix], &[&payer]);

        assert!(
            tx.error.is_none(),
            "transfer should succeed: {:?}",
            tx.error
        );
        assert!(!tx.logs.is_empty(), "the engine returned its drained logs");
        assert!(
            !tx.frames.is_empty(),
            "frames parsed from the returned logs"
        );

        // The per-frame trace is mapped from quasar's own ExecutionTrace: the
        // top frame is the System program, the payer signs and is writable, the
        // destination is written, and both owners come back from the committed
        // store (the field quasar's native trace omits, filled via get_account).
        let trace = tx.trace.as_ref().expect("quasar fills the privilege trace");
        let top = trace.0.first().expect("one top-level frame for a transfer");
        assert_eq!(top.program_id, solana_system_interface::program::id());
        let payer_acct = top
            .accounts
            .iter()
            .find(|a| a.pubkey == payer.pubkey())
            .expect("payer is a traced account");
        assert!(payer_acct.is_signer && payer_acct.is_writable);
        assert_eq!(payer_acct.owner, solana_system_interface::program::id());
        let dest_acct = top
            .accounts
            .iter()
            .find(|a| a.pubkey == dest)
            .expect("dest is a traced account");
        assert!(dest_acct.is_writable && !dest_acct.is_signer);
        assert_eq!(dest_acct.owner, solana_system_interface::program::id());

        assert_eq!(
            backend.account_owner(&dest),
            Some(solana_system_interface::program::id()),
        );
        assert_eq!(
            backend.get_account(&dest).map(|a| a.lamports),
            Some(2_000_000)
        );
        let caps = backend.capabilities();
        assert!(caps.atomic_send && !caps.fees && caps.per_frame_trace);
    }

    #[test]
    fn conformance_on_quasar() {
        let mut backend = QuasarBackend::new();
        testsvm::conformance::scenario(&mut backend);
    }

    #[test]
    fn quasar_clock_levers_work() {
        let mut backend = QuasarBackend::new();
        backend.warp_to_slot(500);
        assert_eq!(backend.clock().slot, 500);
        backend.warp_to_timestamp(1_700_000_000);
        assert_eq!(backend.clock().unix_timestamp, 1_700_000_000);
    }
}
