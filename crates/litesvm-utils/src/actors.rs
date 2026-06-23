//! Facade: the actor vocabulary lives in `testsvm` (deterministic keypairs
//! and the registry are engine-neutral). Old paths keep resolving here.

pub use testsvm::actors::{deterministic_keypair, seed_bytes, ActorRegistry};
