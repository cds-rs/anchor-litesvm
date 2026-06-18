//! The rich CPI renders, as methods on the engine-neutral
//! [`Transaction`](crate::model::Transaction). Each builds a
//! [`CpiModel`](super::model::CpiModel) from the transaction's `frames` and
//! `trace` (via [`from_transaction`](super::model::from_transaction)) and runs
//! the matching renderer, so *every* engine's record produces them, not just a
//! litesvm result. An engine that fills `trace` (litesvm, quasar) gets full
//! authority/ownership graphs; one that does not degrades to account-less inner
//! frames.

use super::{
    authority::AuthorityGraph,
    mermaid::{self, MermaidRenderer},
    model::from_transaction,
    ownership::OwnershipGraph,
    renderer::Renderer,
    style::Style,
    tree::TreeRenderer,
};

impl crate::model::Transaction {
    /// The structured CPI tree: the `── program::ix ──` section header, the
    /// box-drawing body with per-frame CU, and the legend. The rich sibling of
    /// [`pretty_cpi_tree`](crate::model::Transaction::pretty_cpi_tree).
    pub fn logs_structured_string(&self) -> String {
        TreeRenderer {
            style: Style::detect(),
        }
        .render(&from_transaction(self), &self.aliases)
    }

    /// The CPI invocation tree as a Mermaid `sequenceDiagram`, plain arrows.
    pub fn mermaid_string(&self) -> String {
        self.render_mermaid(mermaid::Mode::Plain)
    }

    /// The Mermaid sequence diagram with activation lifelines (paired call /
    /// return arrows), so the synchronous CPI nesting is visible.
    pub fn mermaid_string_with_lifelines(&self) -> String {
        self.render_mermaid(mermaid::Mode::Lifelines)
    }

    fn render_mermaid(&self, mode: mermaid::Mode) -> String {
        MermaidRenderer {
            mode,
            include_logs: mermaid::detect_include_logs(),
        }
        .render(&from_transaction(self), &self.aliases)
    }

    /// The authority graph: a Mermaid flowchart of who signs what and which
    /// accounts each program writes. Inner-frame privilege comes from `trace`.
    pub fn authority_graph_string(&self) -> String {
        AuthorityGraph.render(&from_transaction(self), &self.aliases)
    }

    /// The ownership graph: a Mermaid flowchart of which program owns each
    /// written account. Owners come from the per-frame `trace` (no svm lookup).
    pub fn ownership_graph_string(&self) -> String {
        OwnershipGraph.render(&from_transaction(self), &self.aliases)
    }
}
