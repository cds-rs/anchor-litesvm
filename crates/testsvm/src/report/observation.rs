//! Capture vocabulary and the `record` emit step: what one test observed,
//! written as one lossless JSON file the Reporter later folds.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What a scenario is declared to do. `passed` is the verdict (this vs the
/// actual transaction error), not the transaction's own success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Expect {
    Succeeds,
    Rejects,
}

impl Expect {
    pub fn succeeds(&self) -> bool {
        matches!(self, Expect::Succeeds)
    }
}

/// The test fn's source span, for a `#L` anchor. `end` is `None` for a single
/// line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub start: u32,
    pub end: Option<u32>,
}

impl Anchor {
    /// `"L12"` or `"L12-L20"`.
    pub fn label(&self) -> String {
        match self.end {
            Some(end) if end != self.start => format!("L{}-L{}", self.start, end),
            _ => format!("L{}", self.start),
        }
    }

    /// The 1-based span of `fn <test_fn>(...)` in `manifest_dir/test_file`: the
    /// start line, and the end line found by scanning to the line whose closing
    /// brace sits at the `fn`'s indentation. `None` if the file or fn is absent.
    /// `manifest_dir` is the consuming crate's `CARGO_MANIFEST_DIR` (this code
    /// lives in the framework, so it cannot read the caller's manifest dir).
    pub fn lookup(manifest_dir: &str, test_file: &str, test_fn: &str) -> Option<Anchor> {
        let src = std::fs::read_to_string(format!("{manifest_dir}/{test_file}")).ok()?;
        let needle = format!("fn {test_fn}(");
        let lines: Vec<&str> = src.lines().collect();
        let start_idx = lines.iter().position(|l| l.contains(&needle))?;
        let indent: String = lines[start_idx].chars().take_while(|c| *c == ' ').collect();
        let close = format!("{indent}}}");
        let end_idx = lines[start_idx + 1..]
            .iter()
            .position(|l| *l == close)
            .map(|offset| start_idx + 1 + offset);
        Some(Anchor {
            start: (start_idx + 1) as u32,
            end: end_idx.map(|i| (i + 1) as u32),
        })
    }
}

/// A frame projected for serialization and fingerprinting: the program as a
/// base58 string (role-mapped later, by the Reporter, which holds the alias
/// table), the dispatched instruction name, the outcome as a stable tag, the
/// consumed CU (kept here, dropped during normalize), and the CPI children.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactFrame {
    pub program: String,
    pub instruction_name: Option<String>,
    pub outcome: String,
    pub compute_units: Option<u64>,
    pub children: Vec<FactFrame>,
}

impl FactFrame {
    fn from_frame(frame: &crate::frame::Frame) -> FactFrame {
        use crate::frame::Outcome;
        let outcome = match &frame.outcome {
            Outcome::Success => "success".to_string(),
            Outcome::Failed { message: Some(m) } => format!("failed: {m}"),
            Outcome::Failed { message: None } => "failed".to_string(),
            Outcome::Truncated => "truncated".to_string(),
        };
        FactFrame {
            program: frame.program_id.to_string(),
            instruction_name: frame.instruction_name.clone(),
            outcome,
            compute_units: frame.compute_units.map(|cu| cu.consumed),
            children: frame.children.iter().map(FactFrame::from_frame).collect(),
        }
    }
}

/// The serializable, fingerprint-relevant projection of one execution: the
/// transaction-level error, total CU (dropped during normalize), and the frame
/// tree. Account-role/ownership facts are a v2 enrichment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionFacts {
    pub error: Option<String>,
    pub compute_units: u64,
    pub frames: Vec<FactFrame>,
}

impl ExecutionFacts {
    pub fn from_tx(tx: &crate::model::Transaction) -> ExecutionFacts {
        ExecutionFacts {
            error: tx.error.clone(),
            compute_units: tx.compute_units,
            frames: tx.frames.iter().map(FactFrame::from_frame).collect(),
        }
    }
}

pub const SCHEMA_VERSION: u32 = 1;

/// One test's lossless record. `aliases` carries the table so the Reporter can
/// map `facts` program ids to role labels with no live engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportRecord {
    pub schema_version: u32,
    pub group: String,
    pub title: String,
    pub test_name: String,
    pub test_file: String,
    pub anchor: Anchor,
    pub expect: Expect,
    pub facts: ExecutionFacts,
    pub aliases: Vec<(String, String)>,
}

/// The arguments to `record`: identity, source, declared intent, and the live
/// transaction the test produced.
pub struct Observation<'a> {
    pub group: &'a str,
    pub title: &'a str,
    pub test_name: &'a str,
    pub test_file: &'a str,
    pub manifest_dir: &'a str,
    pub expect: Expect,
    pub tx: &'a crate::model::Transaction,
}

/// Stable slug for a test's record file: `<test_file_slug>__<test_name>` where
/// `/` and `.` in `test_file` are replaced with `-`.
pub fn record_slug(test_file: &str, test_name: &str) -> String {
    format!("{}__{}", test_file.replace(['/', '.'], "-"), test_name)
}

/// Build a record from an observation and write it to
/// `<manifest_dir>/target/test-results/<test_file_slug>__<test_name>.json`.
/// Returns the path. Each consumer crate has its own `target/`, so the corpus
/// dir is simply `manifest_dir` joined with `target/test-results`.
pub fn record(obs: Observation) -> PathBuf {
    let anchor = Anchor::lookup(obs.manifest_dir, obs.test_file, obs.test_name)
        .unwrap_or(Anchor { start: 0, end: None });
    // `entries()` iterates a HashMap, so its order is nondeterministic; sort the
    // stringified pairs so the serialized corpus JSON is byte-stable run to run.
    let mut aliases: Vec<(String, String)> = obs
        .tx
        .aliases
        .entries()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    aliases.sort();
    let rec = ReportRecord {
        schema_version: SCHEMA_VERSION,
        group: obs.group.to_string(),
        title: obs.title.to_string(),
        test_name: obs.test_name.to_string(),
        test_file: obs.test_file.to_string(),
        anchor,
        expect: obs.expect,
        facts: ExecutionFacts::from_tx(obs.tx),
        aliases,
    };
    let dir = PathBuf::from(obs.manifest_dir).join("target/test-results");
    std::fs::create_dir_all(&dir).expect("create target/test-results");
    let slug = record_slug(obs.test_file, obs.test_name);
    let path = dir.join(format!("{slug}.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(&rec).expect("serialize record"))
        .expect("write record");
    path
}

/// `"passed"` when the actual outcome matches the declared intent.
pub fn verdict(expect: Expect, error: &Option<String>) -> &'static str {
    if error.is_none() == expect.succeeds() {
        "passed"
    } else {
        "failed"
    }
}

/// `"succeeded"`, or `"rejected: <first line of the error>"`.
pub fn summary(error: &Option<String>) -> String {
    match error {
        None => "succeeded".to_string(),
        Some(e) => format!("rejected: {}", e.lines().next().unwrap_or(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_label_single_and_range() {
        assert_eq!(Anchor { start: 12, end: None }.label(), "L12");
        assert_eq!(Anchor { start: 12, end: Some(12) }.label(), "L12");
        assert_eq!(Anchor { start: 12, end: Some(20) }.label(), "L12-L20");
    }

    #[test]
    fn verdict_separates_intent_from_outcome() {
        // A correct rejection PASSES; it is not a bare misleading `false`.
        assert_eq!(verdict(Expect::Rejects, &Some("boom".into())), "passed");
        assert_eq!(verdict(Expect::Succeeds, &None), "passed");
        assert_eq!(verdict(Expect::Succeeds, &Some("boom".into())), "failed");
        assert_eq!(verdict(Expect::Rejects, &None), "failed");
    }

    #[test]
    fn summary_takes_the_first_error_line() {
        assert_eq!(summary(&None), "succeeded");
        assert_eq!(summary(&Some("Invalid account owner\n  at frame 2".into())), "rejected: Invalid account owner");
    }

    #[test]
    fn facts_project_the_frame_tree_and_outcome() {
        use crate::frame::{Frame, Outcome};
        use solana_pubkey::Pubkey;
        let child = Frame {
            program_id: Pubkey::new_from_array([2u8; 32]),
            outcome: Outcome::Success,
            compute_units: None,
            instruction_name: Some("Inner".into()),
            logs: vec![],
            children: vec![],
        };
        let root = Frame {
            program_id: Pubkey::new_from_array([1u8; 32]),
            outcome: Outcome::Failed { message: Some("boom".into()) },
            compute_units: None,
            instruction_name: Some("Outer".into()),
            logs: vec![],
            children: vec![child],
        };
        let facts = ExecutionFacts {
            error: Some("boom".into()),
            compute_units: 0,
            frames: vec![FactFrame::from_frame(&root)],
        };
        assert_eq!(facts.frames[0].outcome, "failed: boom");
        assert_eq!(facts.frames[0].children[0].instruction_name.as_deref(), Some("Inner"));
        assert_eq!(facts.frames[0].children.len(), 1);
    }

    #[test]
    fn lookup_finds_this_test_fns_span() {
        // This test file is the fixture: find `fn lookup_finds_this_test_fns_span(`.
        let a = Anchor::lookup(env!("CARGO_MANIFEST_DIR"), "src/report/observation.rs", "lookup_finds_this_test_fns_span")
            .expect("should find itself");
        assert!(a.end.unwrap() > a.start, "a multi-line fn yields a range");
    }

    #[test]
    fn record_round_trips_through_disk() {
        // A minimal Transaction with one frame and one alias.
        use crate::frame::{Frame, Outcome};
        use solana_pubkey::Pubkey;
        let prog = Pubkey::new_from_array([1u8; 32]);
        let mut tx = crate::model::Transaction {
            frames: vec![Frame {
                program_id: prog,
                outcome: Outcome::Success,
                compute_units: None,
                instruction_name: Some("Go".into()),
                logs: vec![],
                children: vec![],
            }],
            account_keys: vec![],
            logs: vec![],
            error: None,
            compute_units: 5,
            fee: None,
            message: solana_message::Message::default(),
            trace: None,
            return_data: None,
            aliases: Default::default(),
            instruction_names: Default::default(),
            error_names: Default::default(),
            events: Default::default(),
        };
        tx.aliases.add(prog, "MyProg");
        let obs = Observation {
            group: "Core",
            title: "It goes",
            test_name: "record_round_trips_through_disk",
            test_file: "src/report/observation.rs",
            manifest_dir: env!("CARGO_MANIFEST_DIR"),
            expect: Expect::Succeeds,
            tx: &tx,
        };
        let path = record(obs);
        let back: ReportRecord =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(back.schema_version, 1);
        assert_eq!(back.group, "Core");
        assert_eq!(back.facts.frames[0].program, prog.to_string());
        assert!(back
            .aliases
            .iter()
            .any(|(k, v)| k == &prog.to_string() && v == "MyProg"));
        std::fs::remove_file(path).ok();
    }
}
