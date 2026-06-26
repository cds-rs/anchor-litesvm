//! The instruction interpretation layer: the single place that turns raw
//! instruction bytes into meaning (a name plus typed operands), keyed by program.
//!
//! Engines stay dumb: each maps its native result into the neutral
//! [`InstructionTrace`](crate::trace::InstructionTrace) and the registry
//! interprets it once, in [`Transaction::assemble`](crate::model::Transaction).
//! No interpreter sees an engine; it sees `data` and `accounts`, so fidelity is
//! expressed by which operands come back populated: an operand is present only
//! when the trace carried its bytes/accounts. On a log-only engine (no trace,
//! or an inner CPI whose data/accounts the logs never carried) the operands are
//! simply absent, which reads honestly as "the engine could not say" rather than
//! a fabricated value.
//!
//! Two shapes, two stages. [`InstructionFact`] is the assemble-time/fingerprint
//! shape, typed (`Pubkey`, `Lamports`). [`ResolvedFact`] is the render-time
//! shape: [`resolve_operands`] applies the [`Labeler`](crate::aliases::Labeler)
//! once (a `Pubkey` becomes its alias, `Lamports` its comma-grouped digits), and
//! each renderer's `From<&ResolvedFact>` impl turns that into its own fragment,
//! owning its arrow glyph and escaping. `From` is unary, so the aliasing it
//! cannot reach happens in `resolve_operands` first.

use {
    crate::{aliases::Labeler, frame::with_commas, trace::TracedAccount},
    solana_pubkey::Pubkey,
    std::collections::HashMap,
};

/// A typed instruction operand. Minimal by design: grow the enum as interpreters
/// need it (a `u64` count, raw `Bytes`, a seed list), not ahead of need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    Pubkey(Pubkey),
    Lamports(u64),
}

/// One instruction's decoded meaning. `name` is `None` when the discriminant is
/// unknown; `operands` is empty when the instruction has none, or when the trace
/// did not carry the bytes/accounts to recover them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionFact {
    pub name: Option<String>,
    pub operands: Vec<(String, Operand)>,
}

/// Interprets one instruction of one program. Takes `data` + `accounts`, never an
/// engine: fidelity is which operands come back populated.
pub trait InstructionInterpreter {
    fn interpret(
        &self,
        program_id: &Pubkey,
        data: &[u8],
        accounts: &[TracedAccount],
    ) -> Option<InstructionFact>;
}

/// `program_id` -> interpreter. Pre-seeded with the native builtins; the single
/// native-decode path the assemble step drives.
#[derive(Default)]
pub struct InterpreterRegistry {
    by_program: HashMap<Pubkey, Box<dyn InstructionInterpreter>>,
}

const COMPUTE_BUDGET: Pubkey = solana_pubkey::pubkey!("ComputeBudget111111111111111111111111111111");
const LOADER_UPGRADEABLE: Pubkey =
    solana_pubkey::pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");

impl InterpreterRegistry {
    /// The registry seeded with the native builtins that carry an instruction
    /// discriminant: System (the only operand-bearing one), ComputeBudget, and
    /// the upgradeable Loader (both name-only). Precompiles are omitted on
    /// purpose: their data is signature offsets, not a tag.
    pub fn with_builtins() -> Self {
        let mut by_program: HashMap<Pubkey, Box<dyn InstructionInterpreter>> = HashMap::new();
        by_program.insert(solana_system_interface::program::id(), Box::new(SystemInterpreter));
        by_program.insert(COMPUTE_BUDGET, Box::new(ComputeBudgetInterpreter));
        by_program.insert(LOADER_UPGRADEABLE, Box::new(LoaderInterpreter));
        Self { by_program }
    }

    /// Interpret one instruction, or `None` if no interpreter is registered for
    /// the program or the data does not decode.
    pub fn interpret(
        &self,
        program_id: &Pubkey,
        data: &[u8],
        accounts: &[TracedAccount],
    ) -> Option<InstructionFact> {
        self.by_program
            .get(program_id)?
            .interpret(program_id, data, accounts)
    }
}

/// The System program's instruction discriminant is a u32 little-endian tag.
/// `Transfer` (tag 2) carries its value in the data (`lamports` at `[4..12]`) and
/// its parties in the accounts (`from = [0]`, `to = [1]`); every other tag is
/// name-only. Names track `solana_system_interface::SystemInstruction`.
struct SystemInterpreter;

impl InstructionInterpreter for SystemInterpreter {
    fn interpret(
        &self,
        _program_id: &Pubkey,
        data: &[u8],
        accounts: &[TracedAccount],
    ) -> Option<InstructionFact> {
        let tag = u32::from_le_bytes(data.get(..4)?.try_into().ok()?);
        let name = match tag {
            0 => "CreateAccount",
            1 => "Assign",
            2 => "Transfer",
            3 => "CreateAccountWithSeed",
            4 => "AdvanceNonceAccount",
            5 => "WithdrawNonceAccount",
            6 => "InitializeNonceAccount",
            7 => "AuthorizeNonceAccount",
            8 => "Allocate",
            9 => "AllocateWithSeed",
            10 => "AssignWithSeed",
            11 => "TransferWithSeed",
            12 => "UpgradeNonceAccount",
            _ => return None,
        };
        let mut operands = Vec::new();
        if tag == 2 {
            // Each operand is added only if its source is present, so a partial
            // trace yields a partial fact rather than a fabricated one.
            if let Some(from) = accounts.first().map(|a| a.pubkey) {
                operands.push(("from".to_string(), Operand::Pubkey(from)));
            }
            if let Some(to) = accounts.get(1).map(|a| a.pubkey) {
                operands.push(("to".to_string(), Operand::Pubkey(to)));
            }
            if let Some(lamports) = data
                .get(4..12)
                .and_then(|b| b.try_into().ok())
                .map(u64::from_le_bytes)
            {
                operands.push(("lamports".to_string(), Operand::Lamports(lamports)));
            }
        }
        Some(InstructionFact {
            name: Some(name.to_string()),
            operands,
        })
    }
}

/// ComputeBudget's discriminant is a single leading `u8`. Name-only. Names track
/// `solana_compute_budget_interface::ComputeBudgetInstruction`.
struct ComputeBudgetInterpreter;

impl InstructionInterpreter for ComputeBudgetInterpreter {
    fn interpret(&self, _: &Pubkey, data: &[u8], _: &[TracedAccount]) -> Option<InstructionFact> {
        let name = match data.first()? {
            0 => "RequestUnitsDeprecated",
            1 => "RequestHeapFrame",
            2 => "SetComputeUnitLimit",
            3 => "SetComputeUnitPrice",
            4 => "SetLoadedAccountsDataSizeLimit",
            _ => return None,
        };
        Some(InstructionFact {
            name: Some(name.to_string()),
            operands: Vec::new(),
        })
    }
}

/// The upgradeable BPF loader's discriminant is a u32 LE tag. Name-only. Names
/// track `solana_loader_v3_interface::instruction::UpgradeableLoaderInstruction`.
struct LoaderInterpreter;

impl InstructionInterpreter for LoaderInterpreter {
    fn interpret(&self, _: &Pubkey, data: &[u8], _: &[TracedAccount]) -> Option<InstructionFact> {
        let tag = u32::from_le_bytes(data.get(..4)?.try_into().ok()?);
        let name = match tag {
            0 => "InitializeBuffer",
            1 => "Write",
            2 => "DeployWithMaxDataLen",
            3 => "Upgrade",
            4 => "SetAuthority",
            5 => "Close",
            6 => "ExtendProgram",
            7 => "SetAuthorityChecked",
            _ => return None,
        };
        Some(InstructionFact {
            name: Some(name.to_string()),
            operands: Vec::new(),
        })
    }
}

/// Apply the labeler to every operand: a `Pubkey` becomes its alias (or short
/// key), `Lamports` its comma-grouped digits (the unit stays out so the value
/// reads cleanly in either the pretty form or the generic `k = v` fallback). The
/// one place the labeler touches operands, so the per-renderer `From` impls need
/// no further context.
pub fn resolve_operands(operands: &[(String, Operand)], labeler: &dyn Labeler) -> Vec<(String, String)> {
    operands
        .iter()
        .map(|(key, operand)| {
            let value = match operand {
                Operand::Pubkey(pk) => labeler.label(pk),
                Operand::Lamports(l) => with_commas(*l),
            };
            (key.clone(), value)
        })
        .collect()
}

/// The render-time shape: display strings, with `name` carrying the frame's
/// precedence-resolved name (a logged `Instruction:` line or an IDL name wins
/// over the interpreter's native decode), not the interpreter's own `name`. The
/// per-renderer `From<&ResolvedFact>` impls build the final fragment from this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFact {
    pub name: Option<String>,
    pub operands: Vec<(String, String)>,
}

impl ResolvedFact {
    /// The operand summary, with `arrow` as the caller's glyph (text `->`, mermaid
    /// `→`). The one recognized shape, `from`+`to`(+`lamports`), renders as
    /// `(from ARROW to) N lamports`; anything else yields the empty string. A
    /// generic `(k = v)` rendering is deliberately not committed to until a second
    /// interpreter has operands worth displaying; the values still ride in the
    /// typed operands for the fingerprint regardless.
    pub fn summary(&self, arrow: &str) -> String {
        let get = |key: &str| {
            self.operands
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        match (get("from"), get("to")) {
            (Some(from), Some(to)) => {
                let mut s = format!("({from} {arrow} {to})");
                if let Some(lamports) = get("lamports") {
                    s.push_str(&format!(" {lamports} lamports"));
                }
                s
            }
            _ => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::aliases::Aliases, solana_pubkey::pubkey};

    fn acct(pubkey: Pubkey) -> TracedAccount {
        TracedAccount {
            pubkey,
            is_signer: false,
            is_writable: true,
            owner: solana_system_interface::program::id(),
        }
    }

    fn transfer_data(lamports: u64) -> Vec<u8> {
        let mut data = 2u32.to_le_bytes().to_vec();
        data.extend_from_slice(&lamports.to_le_bytes());
        data
    }

    #[test]
    fn system_interpreter_decodes_transfer_parties_and_lamports() {
        let from = Pubkey::new_unique();
        let to = Pubkey::new_unique();
        let reg = InterpreterRegistry::with_builtins();
        let fact = reg
            .interpret(
                &solana_system_interface::program::id(),
                &transfer_data(1_000_000),
                &[acct(from), acct(to)],
            )
            .expect("System Transfer decodes");
        assert_eq!(fact.name.as_deref(), Some("Transfer"));
        assert_eq!(
            fact.operands,
            vec![
                ("from".to_string(), Operand::Pubkey(from)),
                ("to".to_string(), Operand::Pubkey(to)),
                ("lamports".to_string(), Operand::Lamports(1_000_000)),
            ]
        );
    }

    #[test]
    fn system_interpreter_transfer_without_accounts_keeps_recoverable_operands() {
        // Each operand is sourced independently: lamports live in the data, the
        // parties in the accounts. With no accounts, the value still surfaces and
        // only the parties are absent, rather than fabricating them.
        let reg = InterpreterRegistry::with_builtins();
        let fact = reg
            .interpret(&solana_system_interface::program::id(), &transfer_data(500), &[])
            .expect("name still decodes");
        assert_eq!(fact.name.as_deref(), Some("Transfer"));
        assert_eq!(
            fact.operands,
            vec![("lamports".to_string(), Operand::Lamports(500))]
        );
    }

    #[test]
    fn system_interpreter_non_transfer_tag_is_name_only() {
        let reg = InterpreterRegistry::with_builtins();
        let fact = reg
            .interpret(&solana_system_interface::program::id(), &[0, 0, 0, 0], &[])
            .unwrap();
        assert_eq!(fact.name.as_deref(), Some("CreateAccount"));
        assert!(fact.operands.is_empty());
    }

    #[test]
    fn registry_decodes_compute_budget_and_loader_names() {
        let reg = InterpreterRegistry::with_builtins();
        let cb = reg
            .interpret(&COMPUTE_BUDGET, &[2, 64, 66, 15, 0], &[])
            .unwrap();
        assert_eq!(cb.name.as_deref(), Some("SetComputeUnitLimit"));
        let loader = reg.interpret(&LOADER_UPGRADEABLE, &[3, 0, 0, 0], &[]).unwrap();
        assert_eq!(loader.name.as_deref(), Some("Upgrade"));
    }

    #[test]
    fn registry_returns_none_for_an_unregistered_program() {
        let reg = InterpreterRegistry::with_builtins();
        assert!(reg
            .interpret(&pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"), &[3], &[])
            .is_none());
    }

    #[test]
    fn resolve_operands_aliases_pubkeys_and_commas_lamports() {
        let from = Pubkey::new_unique();
        let to = Pubkey::new_unique();
        let aliases = Aliases::default().with(from, "Vault").with(to, "Player");
        let operands = vec![
            ("from".to_string(), Operand::Pubkey(from)),
            ("to".to_string(), Operand::Pubkey(to)),
            ("lamports".to_string(), Operand::Lamports(1_000_000)),
        ];
        let resolved = resolve_operands(&operands, &aliases);
        assert_eq!(
            resolved,
            vec![
                ("from".to_string(), "Vault".to_string()),
                ("to".to_string(), "Player".to_string()),
                ("lamports".to_string(), "1,000,000".to_string()),
            ]
        );
    }

    #[test]
    fn summary_renders_the_transfer_pattern_with_the_given_arrow() {
        let rf = ResolvedFact {
            name: Some("Transfer".to_string()),
            operands: vec![
                ("from".to_string(), "Vault".to_string()),
                ("to".to_string(), "Player".to_string()),
                ("lamports".to_string(), "1,000,000".to_string()),
            ],
        };
        assert_eq!(rf.summary("->"), "(Vault -> Player) 1,000,000 lamports");
        assert_eq!(rf.summary("→"), "(Vault → Player) 1,000,000 lamports");
    }

    #[test]
    fn summary_is_empty_for_unrecognized_or_no_operands() {
        // Only the from/to(+lamports) shape renders; an operand set without both
        // parties (or none at all) yields the empty string, no generic listing.
        let partial = ResolvedFact {
            name: Some("Transfer".to_string()),
            operands: vec![("lamports".to_string(), "165".to_string())],
        };
        assert_eq!(partial.summary("->"), "");
        let none = ResolvedFact {
            name: Some("CreateAccount".to_string()),
            operands: vec![],
        };
        assert_eq!(none.summary("->"), "");
    }
}
