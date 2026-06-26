//! The per-frame privilege trace, as DATA. For every executed instruction,
//! top-level or CPI: which accounts the frame presented as signers and
//! writables, and who owned each account after execution. This is what the
//! authority renderer draws from.
//!
//! The data types are re-exported here from `svm-witness` (their definitions
//! moved there, the version-free contract crate). *Recording* a trace is
//! engine-specific by nature (litesvm's inspect callback, mollusk's register
//! tracing) and lives in each engine's adapter crate.

pub use svm_witness::{InstructionTrace, TracedAccount, TracedInstruction};
