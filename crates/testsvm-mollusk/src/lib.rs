//! The mollusk adapter: mollusk's instruction harness behind the `TestSVM`
//! port. State persists in mollusk's own `AccountStore` (`MolluskContext`
//! over a `HashMap`); logs come from an installed `LogCollector` and feed
//! the canonical vendored parser in `testsvm::frame`. Multi-instruction
//! sends run as a chain (state threads, but neither atomic nor
//! budget-shared), so `atomic_send: false`. No fees. The per-frame trace
//! maps from the fork's `InstructionResult::execution_trace`, so the
//! authority and ownership graphs render for a mollusk run. This crate's
//! dependency graph carries NO litesvm: same test, different backend,
//! rebuild.

use {
    mollusk_svm::{account_store::AccountStore, Mollusk, MolluskContext},
    solana_account::Account,
    solana_clock::Clock,
    solana_instruction::Instruction,
    solana_keypair::Keypair,
    solana_message::Message,
    solana_pubkey::Pubkey,
    solana_svm_log_collector::LogCollector,
    std::{cell::RefCell, collections::HashMap, rc::Rc},
    testsvm::{model, Capabilities, TestSVM},
};

pub struct MolluskBackend {
    ctx: MolluskContext<HashMap<Pubkey, Account>>,
    aliases: testsvm::aliases::Aliases,
    instruction_names: testsvm::instructions::InstructionNames,
    error_names: testsvm::errors::ErrorNames,
}

impl Default for MolluskBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MolluskBackend {
    /// Borrow the underlying `MolluskContext` (escape hatch for engine-native
    /// setup the trait does not cover, e.g. `mollusk-svm-programs-token`'s
    /// `add_program`, or reading `sysvars` directly). Mirrors
    /// `LiteSvmBackend::svm()`.
    pub fn ctx(&self) -> &MolluskContext<HashMap<Pubkey, Account>> {
        &self.ctx
    }

    /// Mutably borrow the underlying `MolluskContext`.
    pub fn ctx_mut(&mut self) -> &mut MolluskContext<HashMap<Pubkey, Account>> {
        &mut self.ctx
    }

    pub fn new() -> Self {
        let ctx = MolluskContext {
            mollusk: Mollusk::default(),
            account_store: Rc::new(RefCell::new(HashMap::new())),
            // Keep sysvar accounts OUT of the store: a stored clock account
            // would shadow `mollusk.sysvars` on the next load and freeze
            // time. With hydration off, sysvars resolve fresh from
            // `mollusk.sysvars` on every send, so the warp levers work.
            hydrate_store: false,
        };
        Self {
            ctx,
            aliases: testsvm::aliases::Aliases::with_well_known(),
            instruction_names: testsvm::instructions::InstructionNames::new(),
            error_names: testsvm::errors::ErrorNames::new(),
        }
    }
}

/// Mollusk surfaces frame failures through the runtime's logs (the CPI tree is
/// parsed from them), so the default Anchor `Error Code:` decode applies; no
/// override.
impl model::FailureResolver for MolluskBackend {}

impl TestSVM for MolluskBackend {
    fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> model::Transaction {
        // Fresh collector per send, so logs do not accumulate across sends.
        let collector = LogCollector::new_ref();
        self.ctx.mollusk.logger = Some(collector.clone());

        let result = match ixs {
            [ix] => self.ctx.process_instruction(ix),
            many => self.ctx.process_instruction_chain(many),
        };

        let logs = collector.borrow().get_recorded_content().to_vec();
        self.ctx.mollusk.logger = None;

        // The model wants the legacy Message for top-level signer/writable
        // facts; build it from the instructions (signers[0] = fee payer).
        let payer = testsvm::payer_pubkey(signers);
        let message = Message::new(ixs, payer.as_ref());
        let error = result.raw_result.as_ref().err().map(|e| e.to_string());
        let return_data = (!result.return_data.is_empty()).then(|| result.return_data.clone());

        // The per-frame trace maps from the fork's execution trace; the owner
        // (the one field the executor's trace omits, read back post-execution
        // from the committed store) is the single engine-specific input, passed
        // in so the mapping stays a pure, testable function.
        let trace = trace_from_execution(&result.execution_trace, |pk| {
            self.get_account(pk).map(|a| a.owner).unwrap_or_default()
        });

        // Frames come from the structured trace (shape) with content from the
        // logs, so the CPI tree survives any log gap and is sourced from facts
        // rather than the parse alone (`structured_cpi`).
        let frames = testsvm::frame::frames_from_trace(&trace, &logs);

        model::Transaction::assemble(
            frames,
            message,
            logs,
            error,
            result.compute_units_consumed,
            None,
            Some(trace),
            return_data,
            &self.instruction_names,
            &self.error_names,
            self,
            self.aliases.clone(),
            // Mollusk's instruction-level harness doesn't surface events; the
            // socket default is a no-op, so the registry rides empty.
            testsvm::events::EventRegistry::new(),
        )
    }

    fn set_account(&mut self, address: &Pubkey, account: Account) {
        self.ctx
            .account_store
            .borrow_mut()
            .store_account(*address, account);
    }

    fn fund_sol(&mut self, address: &Pubkey, lamports: u64) {
        self.ctx.account_store.borrow_mut().store_account(
            *address,
            Account {
                lamports,
                owner: solana_system_interface::program::id(),
                ..Account::default()
            },
        );
    }

    fn get_account(&self, pubkey: &Pubkey) -> Option<Account> {
        self.ctx.account_store.borrow().get_account(pubkey)
    }

    fn deploy_program(&mut self, program_id: Pubkey, bytes: &[u8]) {
        self.ctx.mollusk.add_program_with_loader_and_elf(
            &program_id,
            &mollusk_svm::program::loader_keys::LOADER_V3,
            bytes,
        );
    }

    fn warp_to_slot(&mut self, slot: u64) {
        self.ctx.mollusk.warp_to_slot(slot);
    }

    fn warp_to_timestamp(&mut self, unix_timestamp: i64) {
        self.ctx.mollusk.sysvars.clock.unix_timestamp = unix_timestamp;
    }

    fn clock(&self) -> Clock {
        self.ctx.mollusk.sysvars.clock.clone()
    }

    fn configure(&mut self, config: &testsvm::EnvironmentConfig) {
        let testsvm::EnvironmentConfig { compute_unit_limit } = config;
        self.ctx.mollusk.compute_budget.compute_unit_limit = u64::from(*compute_unit_limit);
    }

    fn register_instruction_name(&mut self, program_id: &Pubkey, prefix: &[u8], name: &str) {
        self.instruction_names.register(*program_id, prefix, name);
    }

    fn register_error_name(&mut self, program_id: &Pubkey, code: u32, name: &str) {
        self.error_names.register(*program_id, code, name);
    }

    /// Recorded in this backend's table and stamped onto every sent
    /// `model::Transaction`, so `pretty_cpi_tree` names it. Mollusk has no
    /// endpoint-side render to push to; recording is the whole job here.
    fn register_alias(&mut self, pubkey: &Pubkey, name: &str) {
        self.aliases.add(*pubkey, name);
    }

    fn register_cast_name(&mut self, name: &str) -> bool {
        self.aliases.register_cast(name)
    }

    fn aliases(&self) -> &testsvm::aliases::Aliases {
        &self.aliases
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            per_frame_trace: true, // mapped from the fork's execution trace
            structured_cpi: true,  // frames sourced from the structured trace
            atomic_send: false,    // chain threads state, not atomicity
            fees: false,
            instant_reset: true,
            fork: false,
        }
    }
}

/// Map the fork's `execution_trace` to the engine-neutral per-frame
/// [`InstructionTrace`](testsvm::trace::InstructionTrace). Pure: `owner_of`
/// supplies each account's owner (the one field the executor's trace omits),
/// so the engine-specific store lookup stays out of the mapping and the
/// function is unit-testable on synthetic input.
pub(crate) fn trace_from_execution(
    instructions: &[mollusk_svm::result::ExecutedInstruction],
    owner_of: impl Fn(&Pubkey) -> Pubkey,
) -> testsvm::trace::InstructionTrace {
    testsvm::trace::InstructionTrace(
        instructions
            .iter()
            .map(|ei| testsvm::trace::TracedInstruction {
                program_id: ei.program_id,
                // The fork's stack height already follows Agave's 1-based
                // convention (top-level == 1), so it carries over directly.
                stack_height: ei.stack_height as usize,
                accounts: ei
                    .accounts
                    .iter()
                    .map(|a| testsvm::trace::TracedAccount {
                        pubkey: a.pubkey,
                        is_signer: a.is_signer,
                        is_writable: a.is_writable,
                        owner: owner_of(&a.pubkey),
                    })
                    .collect(),
                data: ei.data.clone(),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use {super::*, solana_signer::Signer};

    #[test]
    fn mollusk_backend_sends_a_system_transfer() {
        let mut backend = MolluskBackend::new();
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
        assert!(!tx.logs.is_empty(), "the log collector captured the run");
        assert!(
            !tx.frames.is_empty(),
            "frames parsed from the collected logs"
        );
        assert_eq!(
            backend.account_owner(&dest),
            Some(solana_system_interface::program::id()),
        );
        assert_eq!(
            backend.get_account(&dest).map(|a| a.lamports),
            Some(2_000_000)
        );

        // The per-frame trace maps from the fork's execution trace: one
        // top-level System frame; the payer signs and is writable; the
        // destination is written; both owners come back from the committed
        // store (the field the executor's trace omits, filled via get_account).
        let trace = tx
            .trace
            .as_ref()
            .expect("the fork fills the privilege trace");
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

        let caps = backend.capabilities();
        assert!(!caps.atomic_send && !caps.fees && caps.per_frame_trace);

        // The payoff: with the trace filled, the authority renderer draws a
        // full graph for a mollusk record, not the degraded log-only one. A
        // System transfer is `payer --signs--> System --writes--> dest`.
        let authority = tx.authority_graph_string();
        assert!(
            authority.contains("-->|signs|") && authority.contains("-->|writes|"),
            "mollusk's transaction renders a full authority graph:\n{authority}"
        );
    }

    #[test]
    fn trace_from_execution_maps_frames_and_backfills_owner() {
        use mollusk_svm::result::{ExecutedAccount, ExecutedInstruction};

        let program = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let passive = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let instructions = vec![ExecutedInstruction {
            stack_height: 2, // a CPI frame; carries over 1-based, unchanged
            program_id: program,
            accounts: vec![
                ExecutedAccount {
                    pubkey: signer,
                    is_signer: true,
                    is_writable: true,
                },
                ExecutedAccount {
                    pubkey: passive,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            data: vec![7, 8, 9],
        }];

        // The owner is the one field the executor's trace omits; the mapper
        // takes it from the store lookup, here a constant.
        let trace = trace_from_execution(&instructions, |_| owner);

        assert_eq!(trace.0.len(), 1);
        let frame = &trace.0[0];
        assert_eq!(frame.program_id, program);
        assert_eq!(frame.stack_height, 2);
        assert_eq!(frame.data, vec![7, 8, 9]);
        assert_eq!(
            frame.accounts,
            vec![
                testsvm::trace::TracedAccount {
                    pubkey: signer,
                    is_signer: true,
                    is_writable: true,
                    owner,
                },
                testsvm::trace::TracedAccount {
                    pubkey: passive,
                    is_signer: false,
                    is_writable: false,
                    owner,
                },
            ]
        );
    }

    #[test]
    fn conformance_on_mollusk() {
        let mut backend = MolluskBackend::new();
        testsvm::conformance::scenario(&mut backend);
    }

    #[test]
    fn mollusk_clock_levers_work() {
        let mut backend = MolluskBackend::new();
        backend.warp_to_slot(500);
        assert_eq!(backend.clock().slot, 500);
        backend.warp_to_timestamp(1_700_000_000);
        assert_eq!(backend.clock().unix_timestamp, 1_700_000_000);
    }
}
