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
//! [`ToBlock`]; assemble the per-scenario files (one per test, named by a
//! slug of the title) into a single document with a `just`-style concat step.

mod block;
mod canonical;
mod console;
mod core;
mod fingerprint;
mod markdown;
mod normalize;
mod observation;
mod render;
mod reporter;
mod scenario;

pub use block::ToBlock;
// The frood-guide vocabulary a report is built from, re-exported so a consumer
// reaches `Block` / `Cell` / `TableModel` through this crate's report surface
// (and, transitively, through anchor-litesvm) without a direct frood-guide dep.
pub use frood_guide::{Block, Cell, TableModel};
pub use canonical::{canonical_json, fingerprint};
pub use fingerprint::{diff, merkle, Change, ChangeKind, Manifest};
pub use core::{ActBuilder, Report};
pub use normalize::{normalize_default, NormalFrame, NormalRecord};
pub use observation::{record, record_slug, summary, verdict, Anchor, ExecutionFacts, Expect, FactFrame, Observation, ReportRecord, SCHEMA_VERSION};
pub use reporter::Reporter;
pub use scenario::{render_index, render_scenario};
// A domain value reaches a report as a `frood_guide::Block`: either built
// directly and handed to `Report::block`, or via a `ToBlock` impl for
// `Report::snapshot` / `Report::authority`. The report emits the assembled
// blocks through frood-guide's Markdown renderer.
//
// `Status` is `pub(super)` (visible within the `report` module tree); tests
// that need it import it as `super::render::Status` from within the tree.

#[cfg(feature = "fingerprint-baseline")]
mod baseline;
#[cfg(feature = "fingerprint-baseline")]
pub use baseline::{baseline_diff, diff_record_maps, render_explain, write_baseline, RecordChange, RecordDiff, BASELINE_FILE};
#[cfg(feature = "fingerprint-baseline")]
mod cli;
#[cfg(feature = "fingerprint-baseline")]
pub use cli::run_cli;

#[cfg(test)]
mod proptests;
