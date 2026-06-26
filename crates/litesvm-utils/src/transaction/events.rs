//! The event-decode vocabulary now lives in `testsvm` (beside the instruction
//! and error tables) so the [`TestSVM`](testsvm::TestSVM) trait can expose event
//! registration as a backend socket. This module re-exports it so the
//! `litesvm_utils::transaction::{EventInfo, EventRegistry}` paths keep resolving
//! for the book and dogfooders. The `Program data:` base64-framing strip that
//! used to live here moved onto the registry itself
//! ([`EventRegistry::decode_logged`](testsvm::events::EventRegistry::decode_logged)),
//! so every engine's renderer decodes logged events identically. See
//! [`testsvm::events`].

pub use testsvm::events::{EventInfo, EventRegistry};
