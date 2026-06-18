use super::core::Report;

/// The verdict for a completed (or aborted) report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Pass,
    Failed(usize),
    RedExpected(String),
    Aborted,
}

impl Status {
    /// The heading word the markdown header shows.
    pub(super) fn label(&self) -> String {
        match self {
            Status::Pass => "PASS",
            Status::Failed(_) => "FAIL",
            Status::RedExpected(_) => "RED (expected)",
            Status::Aborted => "ABORTED",
        }
        .to_string()
    }
}

/// A strategy for serializing a [`Report`] to a string.
pub(super) trait ReportRenderer {
    fn render(&self, report: &Report, status: Status) -> String;
}
