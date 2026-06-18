//! The CPI model and its renderers: resolve a [`model::CpiModel`] from the
//! engine-neutral [`crate::model::Transaction`] (frames + trace), then render
//! it as a tree, a Mermaid sequence diagram, an authority graph, or an
//! ownership graph. It lives here in `testsvm` so every engine adapter's
//! transaction gets the rich renders, not just litesvm.
//!
//! See `docs/design/cpi-rendering.md` for the architecture.

// Re-export the vocabulary modules so the renderers' `super::X` paths resolve
// to testsvm's own modules (these were facade shims in litesvm-utils).
pub(crate) use crate::{aliases, events, trace};
pub(crate) use crate::{events::EventRegistry, instructions::InstructionNames};

mod account_index;
mod authority;
mod authority_story;
mod graph;
mod mermaid;
pub mod model;
mod ownership;
mod render_api;
mod renderer;
mod signers;
mod style;
mod tree;

// The per-test renderers (a census of accounts, a cross-submit authority
// story): engine-neutral, built from the same `model::Transaction` records, so
// they live on the spine beside the per-transaction renders.
pub use account_index::{AccountIndex, AccountNode, AuthorityClass};
pub use authority_story::AuthorityStory;
