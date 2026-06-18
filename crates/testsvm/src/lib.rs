//! The test vocabulary: what a test is written in, independent of which
//! program framework implements the contract (Anchor, Pinocchio) and which
//! engine evaluates it (litesvm, mollusk, an RPC surfnet).
//!
//! Adapter crates implement [`TestSVM`] per engine and specialize their
//! engine's native execution structure into [`model::Transaction`]. Where an
//! engine exports no structured view, the adapter uses the vendored log
//! parser in [`frame`]. See NOTES/2026-06-10-testsvm-extraction-design.md.

pub mod actors;
pub mod aliases;
pub mod conformance;
pub mod cpi;
pub mod errors;
pub mod events;
pub mod frame;
pub mod instructions;
pub mod model;
pub mod token;
pub mod trace;

use {
    solana_account::Account, solana_clock::Clock, solana_instruction::Instruction,
    solana_keypair::Keypair, solana_pubkey::Pubkey,
};

/// What a backend can populate, so a report can annotate degraded output
/// instead of silently rendering a partial diagram.
// ANCHOR: capabilities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Whether `model::Transaction::trace` is populated (the authority
    /// diagram needs it).
    pub per_frame_trace: bool,
    /// Whether `frames` came from structured facts (vs the canonical log parse).
    pub structured_cpi: bool,
    /// Whether a multi-ix `send` is one atomic, budget-shared transaction.
    pub atomic_send: bool,
    /// Whether the engine models fees (`fee: Some(..)`).
    pub fees: bool,
    /// Whether a full per-test reset is cheap (rebuild the VM) vs requiring
    /// namespacing on a shared endpoint.
    pub instant_reset: bool,
    /// Whether the endpoint forks live cluster state.
    pub fork: bool,
}
// ANCHOR_END: capabilities

/// The SVM a test executes against. One trait; one adapter crate per engine.
// ANCHOR: trait-core
pub trait TestSVM {
    /// Build a transaction from `ixs` (`signers[0]` is the fee payer), send
    /// it, and return what the engine witnessed. One atomic transaction: all
    /// ixs succeed or none persist, shared CU budget; an engine that cannot
    /// honor atomicity must say so in `capabilities()`. A program-level
    /// failure is carried in [`model::Transaction::error`]; only a malformed
    /// build panics.
    fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> model::Transaction;

    /// Fund an address with lamports (airdrop in memory; a faucet/cheatcode
    /// or real transfer over RPC).
    fn fund_sol(&mut self, address: &Pubkey, lamports: u64);

    /// Write an account's full state (lamports, data, owner): the state
    /// fabrication lever. Every engine has it natively (litesvm
    /// `set_account`, mollusk's account store, surfpool's
    /// `surfnet_setAccount` cheatcode); tests use it to fabricate state a
    /// real flow would have built elsewhere (a mint, a foreign PDA).
    fn set_account(&mut self, address: &Pubkey, account: Account);

    /// Post-execution owner of an account (drives the ownership graph).
    fn account_owner(&self, pubkey: &Pubkey) -> Option<Pubkey>;

    /// Read an account's full state.
    fn get_account(&self, pubkey: &Pubkey) -> Option<Account>;

    /// Make a program available at `program_id`.
    fn deploy_program(&mut self, program_id: Pubkey, bytes: &[u8]);

    /// Move the clock so the next execution sees at least this slot. On
    /// litesvm/mollusk the clock moves only when warped; surfpool's ticks in
    /// real time, so this is a floor, not a freeze.
    fn warp_to_slot(&mut self, slot: u64);

    /// Set the Clock sysvar's `unix_timestamp` (seconds); other Clock fields
    /// unchanged.
    fn warp_to_timestamp(&mut self, unix_timestamp: i64);

    /// The clock the engine will present to the next execution.
    fn clock(&self) -> Clock;

    fn capabilities(&self) -> Capabilities;

    /// The engine's alias table, read access. The socket the naming default
    /// methods build on ([`label`](Self::label), and the token aliasing in
    /// [`TokenTestSVM`](crate::token::TokenTestSVM)): every adapter already
    /// holds an [`Aliases`](crate::aliases::Aliases) for `register_alias`, so it
    /// returns a borrow and inherits the naming surface for free.
    fn aliases(&self) -> &crate::aliases::Aliases;
    // ANCHOR_END: trait-core

    /// Declare an actor: one call resolves the common idiom in one place —
    /// a DETERMINISTIC keypair derived from the name (so rendered trees and
    /// snapshot files stay byte-reproducible across runs), the alias
    /// registered, and the account funded. Default method, written entirely
    /// in trait verbs: every engine gets it.
    fn actor(&mut self, name: &str, lamports: u64) -> Keypair {
        assert!(
            self.register_cast_name(name),
            "cast name {name:?} already used in this scenario; cast names seed \
             keypairs and register aliases, so a duplicate would alias two casts \
             to one identity. Give this cast a distinct name."
        );
        let keypair = crate::actors::deterministic_keypair("testsvm", name);
        let pubkey = {
            use solana_signer::Signer;
            keypair.pubkey()
        };
        self.register_alias(&pubkey, name);
        self.fund_sol(&pubkey, lamports);
        keypair
    }

    /// Deploy a program from its `.so` file and alias it, in one declaration.
    /// Panics with a diagnosis when the file is missing or is a stub: an ELF
    /// under 4 KiB almost certainly built without its entrypoint (a
    /// feature-gated `entrypoint!` and a plain `cargo build-sbf` yield an
    /// ~896-byte shell that fails to load as `EntrypointOutOfBounds`).
    fn deploy_from_file(&mut self, program_id: &Pubkey, path: &str, name: &str) {
        let elf = std::fs::read(path).unwrap_or_else(|e| {
            panic!("deploy_from_file: read {path}: {e} (build the program first)")
        });
        assert!(
            elf.len() >= 4096,
            "deploy_from_file: {path} is {} bytes — likely an entrypoint-less stub;              check the program's build features (e.g. `--features sbf`)",
            elf.len()
        );
        self.deploy_program(*program_id, &elf);
        self.register_alias(program_id, name);
    }

    /// Declare a prop: fabricated state under a deterministic, named address.
    /// The counterpart of [`TestSVM::actor`] for non-signing accounts: one
    /// declaration resolves the address (derived from the name, so snapshots
    /// stay byte-reproducible), writes the account, and registers the alias.
    /// Format-specific packing (an SPL mint, a stake account) stays with the
    /// caller; this owns the address + state + name plumbing.
    fn prop(&mut self, name: &str, account: Account) -> Pubkey {
        let address =
            Pubkey::new_from_array(crate::actors::seed_bytes(&format!("testsvm:prop:{name}")));
        self.prop_at(name, &address, account)
    }

    /// [`prop`](Self::prop) at a caller-chosen `address` instead of the
    /// name-derived one: for state whose address is fixed by derivation
    /// elsewhere (an associated token account, a program PDA). Same cast-name
    /// guard, same aliasing; only the address origin differs.
    fn prop_at(&mut self, name: &str, address: &Pubkey, account: Account) -> Pubkey {
        assert!(
            self.register_cast_name(name),
            "cast name {name:?} already used in this scenario; cast names seed \
             addresses and register aliases, so a duplicate would alias two casts \
             to one identity. Give this cast a distinct name."
        );
        self.set_account(address, account);
        self.register_alias(address, name);
        *address
    }

    /// Register `discriminator-prefix -> name` for a program's instructions,
    /// so frames render `program::Name` instead of the bare program. The
    /// adapter records it and names every matching top-level frame at send
    /// time (CPI frames need the structured inner-instruction path, a known
    /// follow-up). A `#[derive(Discriminator)]` Pinocchio program gets the
    /// table for free from its generated `instruction_names()`.
    fn register_instruction_name(&mut self, _program_id: &Pubkey, _prefix: &[u8], _name: &str) {}

    /// Bulk-register a program's instruction-name table: the socket for the
    /// `#[derive(Discriminator)]` / `define_instruction_set!` generated
    /// `instruction_names()` (one-byte discriminators, the Pinocchio
    /// invariant). Sugar over [`TestSVM::register_instruction_name`].
    fn register_program_instructions(&mut self, program_id: &Pubkey, names: &[(u8, &str)]) {
        for (disc, name) in names {
            self.register_instruction_name(program_id, &[*disc], name);
        }
    }

    /// Register `error code -> name` for a program, so a failed frame renders
    /// `InvalidAmount (0x7)` instead of the bare code. Adapters record it and
    /// resolve failed frames at send time. Default no-op for an adapter
    /// without storage.
    fn register_error_name(&mut self, _program_id: &Pubkey, _code: u32, _name: &str) {}

    /// Bulk-register a program's error-name table: the socket for
    /// `define_error_set!`'s generated `error_names()`. Sugar over
    /// [`TestSVM::register_error_name`].
    fn register_program_errors(&mut self, program_id: &Pubkey, names: &[(u32, &str)]) {
        for (code, name) in names {
            self.register_error_name(program_id, *code, name);
        }
    }

    /// Register a `pubkey -> name` alias. Adapters record it in their own
    /// table (stamped onto every sent [`model::Transaction`], so the model's
    /// render names it) and push it to the endpoint's output where one
    /// exists (surfpool's `surfnet_registerAlias`). The provided default
    /// body is a no-op; every shipped adapter overrides it (see each
    /// adapter's impl doc for what its engine does).
    fn register_alias(&mut self, _pubkey: &Pubkey, _name: &str) {}

    /// Register a decoder for a *logged* event (Anchor's `emit!`): the 8-byte
    /// discriminator maps to a display name and a field decoder, so the
    /// structured views render `🔔 Name { .. }` instead of the raw `Program
    /// data:` blob. Adapters that hold an [`EventRegistry`](crate::events::EventRegistry)
    /// store it and stamp it onto every sent [`model::Transaction`] (the peer of
    /// `register_alias` / `register_program_instructions` for events). The
    /// default body is a no-op; an engine that doesn't surface events ignores it.
    fn register_event_decoder(
        &mut self,
        discriminator: [u8; 8],
        name: &str,
        decode: crate::events::EventDecoder,
    ) {
        // The fixed-8-byte spelling is a special case of the variable-width
        // socket; route there so an adapter need only override one of them.
        self.register_logged_event(&discriminator, name, decode);
    }

    /// Register a logged-event decoder by a leading-byte discriminator of ANY
    /// width: Anchor's 8-byte name hash, Quasar's single byte, Shank's one byte.
    /// The general socket behind [`register_event`](Self::register_event) and the
    /// fixed-width [`register_event_decoder`](Self::register_event_decoder).
    /// Adapters that hold an [`EventRegistry`](crate::events::EventRegistry) store
    /// it via [`register_logged`](crate::events::EventRegistry::register_logged);
    /// the default is a no-op.
    fn register_logged_event(
        &mut self,
        _prefix: &[u8],
        _name: &str,
        _decode: crate::events::EventDecoder,
    ) {
    }

    /// Register a typed logged event from its [`DecodableEvent`](crate::events::DecodableEvent)
    /// impl: the discriminator width, the name, and the field decoder all come
    /// off `E`, so neither the adapter nor the test restates the scheme. Sugar
    /// over [`register_logged_event`](Self::register_logged_event).
    fn register_event<E: crate::events::DecodableEvent>(&mut self)
    where
        Self: Sized,
    {
        let decode: fn(&[u8]) -> Option<Vec<(String, String)>> = E::decode;
        self.register_logged_event(E::DISCRIMINATOR, E::name(), std::sync::Arc::new(decode));
    }

    /// Register a decoder for a *self-CPI* event (`emit_cpi!`, and compatible
    /// hand-rolled engines): for `program_id`, the `tag ++ disc` byte prefix maps
    /// to a name and a field decoder. The event leaves no log; its payload is the
    /// inner instruction's data, which the trace carries onto the frame. Keyed by
    /// program (like [`register_instruction_name`](Self::register_instruction_name))
    /// because the tag is shared across anchor-compatible programs. Default no-op,
    /// overridden by adapters that hold an [`EventRegistry`](crate::events::EventRegistry).
    fn register_cpi_event(
        &mut self,
        _program_id: &Pubkey,
        _prefix: &[u8],
        _name: &str,
        _decode: crate::events::EventDecoder,
    ) {
    }

    /// Record `name` as a freshly cast identity, returning `false` if it was
    /// already cast on this instance: the duplicate-name guard the cast helpers
    /// ([`actor`](Self::actor) / [`prop`](Self::prop)) share. Every shipped
    /// adapter overrides it to track names in its alias table
    /// ([`Aliases::register_cast`](crate::aliases::Aliases::register_cast)); the
    /// default is a no-op guard (always `true`) so a stateless mock still works.
    fn register_cast_name(&mut self, _name: &str) -> bool {
        true
    }

    /// Resolve `pubkey` to its registered alias, or a short `<8>…<4>` form when
    /// it isn't aliased. The trait-level naming primitive: a report or table
    /// built against any engine names accounts the same way the structured tree
    /// does. Default method over [`aliases`](Self::aliases).
    fn label(&self, pubkey: &Pubkey) -> String {
        self.aliases().label(pubkey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recording mock: the derive-socket sugars are default methods, so a
    /// minimal impl proves they fan out correctly with no engine in sight.
    #[derive(Default)]
    struct Recorder {
        instructions: Vec<(Pubkey, Vec<u8>, String)>,
        errors: Vec<(Pubkey, u32, String)>,
        aliases: crate::aliases::Aliases,
    }

    impl TestSVM for Recorder {
        fn send(&mut self, _: &[Instruction], _: &[&Keypair]) -> model::Transaction {
            unimplemented!("not exercised")
        }
        fn fund_sol(&mut self, _: &Pubkey, _: u64) {}
        fn account_owner(&self, _: &Pubkey) -> Option<Pubkey> {
            None
        }
        fn get_account(&self, _: &Pubkey) -> Option<Account> {
            None
        }
        fn set_account(&mut self, _: &Pubkey, _: Account) {}
        fn deploy_program(&mut self, _: Pubkey, _: &[u8]) {}
        fn warp_to_slot(&mut self, _: u64) {}
        fn warp_to_timestamp(&mut self, _: i64) {}
        fn clock(&self) -> Clock {
            Clock::default()
        }
        fn capabilities(&self) -> Capabilities {
            unimplemented!("not exercised")
        }
        fn register_instruction_name(&mut self, program_id: &Pubkey, prefix: &[u8], name: &str) {
            self.instructions
                .push((*program_id, prefix.to_vec(), name.to_string()));
        }
        fn register_error_name(&mut self, program_id: &Pubkey, code: u32, name: &str) {
            self.errors.push((*program_id, code, name.to_string()));
        }
        fn aliases(&self) -> &crate::aliases::Aliases {
            &self.aliases
        }
    }

    #[test]
    fn derive_tables_fan_out_through_the_sockets() {
        // The exact shapes #[derive(Discriminator)] and define_error_set!
        // generate: &[(u8, &str)] and &[(u32, &str)].
        const IXS: &[(u8, &str)] = &[(0, "Make"), (1, "Take"), (2, "Cancel")];
        const ERRS: &[(u32, &str)] = &[(6, "Expired"), (7, "InvalidAmount")];

        let program = Pubkey::new_unique();
        let mut backend = Recorder::default();
        backend.register_program_instructions(&program, IXS);
        backend.register_program_errors(&program, ERRS);

        assert_eq!(backend.instructions.len(), 3);
        assert_eq!(
            backend.instructions[1],
            (program, vec![1], "Take".to_string())
        );
        assert_eq!(backend.errors.len(), 2);
        assert_eq!(backend.errors[1], (program, 7, "InvalidAmount".to_string()));
    }
}
