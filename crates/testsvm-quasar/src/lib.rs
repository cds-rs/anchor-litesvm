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
//! `InstructionTrace`, so the authority and ownership views render in full.
//! This crate's dependency graph carries NO litesvm and NO mollusk: same
//! test, different backend, rebuild.

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

/// quasar-svm surfaces frame failures through the runtime's logs (the CPI tree
/// and the per-frame trace are both derived from its execution record), so the
/// default Anchor `Error Code:` decode applies; no override.
impl model::FailureResolver for QuasarBackend {}

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
        let payer = testsvm::payer_pubkey(signers);
        let message = Message::new(ixs, payer.as_ref());
        let error = result.raw_result.as_ref().err().map(|e| e.to_string());
        let return_data = (!result.return_data.is_empty()).then(|| result.return_data.clone());

        // The per-frame trace maps from quasar's own execution trace; the owner
        // (the one field quasar's trace omits, read back post-execution from the
        // committed store) is the single engine-specific input, passed in so the
        // mapping stays a pure, testable function (see `trace_from_execution`).
        let trace = trace_from_execution(&result.execution_trace.instructions, |pk| {
            self.svm.get_account(pk).map(|a| a.owner).unwrap_or_default()
        });

        // Frames come from the neutral trace (shape) with content from the logs.
        let frames = testsvm::frame::frames_from_trace(&trace, &logs);

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
            self,
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

    fn configure(&mut self, config: &testsvm::EnvironmentConfig) {
        let testsvm::EnvironmentConfig { compute_unit_limit } = config;
        self.svm.compute_budget.compute_unit_limit = u64::from(*compute_unit_limit);
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

    /// Self-CPI events (the program emits an event as an inner instruction whose
    /// data is `prefix ++ payload`, with no `Program data:` log) decode off the
    /// traced frame, same as litesvm. quasar already captures the inner frame
    /// and its data (`per_frame_trace`), so this is the same registry write the
    /// litesvm backend does; without it the frame renders but the event does not.
    fn register_cpi_event(
        &mut self,
        program_id: &Pubkey,
        prefix: &[u8],
        name: &str,
        decode: testsvm::events::EventDecoder,
    ) {
        self.events
            .register_cpi(*program_id, prefix.to_vec(), name, decode);
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
            structured_cpi: true, // frames sourced from the structured execution trace
            atomic_send: true,     // one sanitized message, shared budget
            fees: false,
            instant_reset: true,
            fork: false,
        }
    }
}

/// Map quasar's execution trace to the engine-neutral per-frame
/// [`InstructionTrace`](testsvm::trace::InstructionTrace). Pure: `owner_of`
/// supplies each account's owner (the one field quasar's trace omits), so the
/// engine-specific store lookup stays out of the mapping and the function is
/// unit-testable on synthetic input.
pub(crate) fn trace_from_execution(
    instructions: &[quasar_svm::ExecutedInstruction],
    owner_of: impl Fn(&solana_pubkey::Pubkey) -> solana_pubkey::Pubkey,
) -> testsvm::trace::InstructionTrace {
    testsvm::trace::InstructionTrace(
        instructions
            .iter()
            .map(|ei| testsvm::trace::TracedInstruction {
                program_id: ei.instruction.program_id,
                // quasar's stack depth is 0-based (0 = top); the neutral height
                // follows Agave's 1-based convention, so add one.
                stack_height: ei.stack_depth as usize + 1,
                accounts: ei
                    .instruction
                    .accounts
                    .iter()
                    .map(|m| testsvm::trace::TracedAccount {
                        pubkey: m.pubkey,
                        is_signer: m.is_signer,
                        is_writable: m.is_writable,
                        owner: owner_of(&m.pubkey),
                    })
                    .collect(),
                data: ei.instruction.data.clone(),
            })
            .collect(),
    )
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

        // The payoff: with the trace filled, the relocated authority renderer
        // (now in testsvm, reachable on the neutral transaction) draws a full
        // graph for a quasar record, not the degraded mollusk-style one. A
        // System transfer is `payer --signs--> System --writes--> dest`.
        let authority = tx.authority_graph_string();
        assert!(
            authority.contains("-->|signs|") && authority.contains("-->|writes|"),
            "quasar's transaction renders a full authority graph:\n{authority}"
        );
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
