//! Facade: the alias vocabulary lives in `testsvm` (naming is engine-neutral;
//! every adapter's model render uses it). Old paths keep resolving here.

pub use testsvm::aliases::Aliases;
pub(crate) use testsvm::aliases::{is_well_known_name, short_pubkey};
