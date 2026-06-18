//! `Report`: a test recorder that renders a Markdown narrative (to a file) and a
//! styled console view (to stdout). See module docs in the submodules.
//!
//! Two channels, interleaved in author order:
//!   - intent    (`note`, `step`): prose; what regime we're in and why.
//!   - structure (`snapshot`, `check`): values observed from the running test.
//!
//! The structural channel is the trustworthy one: every number in it is the
//! same number the test asserted on, so the report can't quietly disagree with
//! the code the way a stale comment can. Prose can drift; observed values can't.
//!
//! This complements `print_markdown_pair` (the per-transaction renderer on the
//! litesvm `TransactionResult`) rather than replacing it. That method renders
//! one *transaction* (its instruction + logs); a `Report` renders one
//! *scenario*: the intent, the before/after state, and the pass/fail checks, as
//! a single committable document per test.
//!
//! This module is domain-agnostic: it knows nothing about your program's
//! accounts. Domain types earn a place in a report by implementing
//! [`ToMarkdown`]; assemble the per-scenario files (one per test, named by a
//! slug of the title) into a single document with a `just`-style concat step.

mod block;
mod core;
mod markdown;
mod render;

pub use block::{MarkdownBlock, ToMarkdown};
pub use core::{ActBuilder, Report};
// `md_kv!` / `md_table!` are `#[macro_export]`, so they land at the crate root
// regardless of which submodule defines them; no re-export needed here.
//
// `Status` is `pub(super)` (visible within the `report` module tree); tests
// that need it import it as `super::render::Status` from within the tree.
