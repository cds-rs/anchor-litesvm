//! The mollusk adapter: mollusk's instruction harness behind the `TestSVM`
//! port. State persists in mollusk's own `AccountStore` (`MolluskContext`
//! over a `HashMap`); logs come from an installed `LogCollector` and feed
//! the canonical vendored parser in `testsvm::frame`. Multi-instruction
//! sends run as a chain (state threads, but neither atomic nor
//! budget-shared), so `atomic_send: false`. No fees; no per-frame trace in
//! v1. This crate's dependency graph carries NO litesvm: same test,
//! different backend, rebuild.

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
        let payer = signers.first().map(|kp| {
            use solana_signer::Signer;
            kp.pubkey()
        });
        let message = Message::new(ixs, payer.as_ref());
        let frames = testsvm::frame::frames_from_logs(&logs);
        let error = result.raw_result.as_ref().err().map(|e| e.to_string());
        let return_data = (!result.return_data.is_empty()).then(|| result.return_data.clone());
        model::Transaction::assemble(
            frames,
            message,
            logs,
            error,
            result.compute_units_consumed,
            None,
            None,
            return_data,
            &self.instruction_names,
            &self.error_names,
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

    fn account_owner(&self, pubkey: &Pubkey) -> Option<Pubkey> {
        self.get_account(pubkey).map(|a| a.owner)
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
            per_frame_trace: false,
            structured_cpi: false, // v1: frames via the canonical log parse
            atomic_send: false,    // chain threads state, not atomicity
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
        let caps = backend.capabilities();
        assert!(!caps.atomic_send && !caps.fees && !caps.per_frame_trace);
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
