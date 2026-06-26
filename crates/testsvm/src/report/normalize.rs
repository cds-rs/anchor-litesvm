//! The default semantic transform: a `ReportRecord` to a `NormalRecord`, with
//! the universal canonicalizations every Solana program needs (program ids to
//! role labels via the captured alias table). CU is retained: it is
//! deterministic in this setup (pinned `.so` + locked VM + deterministic keys)
//! so a shift is a real signal. Program-specific shaping is a free-function
//! override registered with the Reporter, not here.

use {
    crate::report::observation::{summary, verdict, FactFrame, ReportRecord},
    serde::Serialize,
    std::collections::HashMap,
};

/// A frame with the program mapped to its role label. CU is retained so a
/// compute-unit shift is caught by the fingerprint gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalFrame {
    pub program: String,
    pub instruction: Option<String>,
    /// The decoded operands, role-mapped: a `Pubkey` operand becomes its role
    /// (like `program`), a `Lamports` its numeric value. In the behavioral hash,
    /// so a wrong-recipient or wrong-amount mutant changes the fingerprint.
    pub operands: Vec<(String, String)>,
    pub outcome: String,
    pub compute_units: Option<u64>,
    pub children: Vec<NormalFrame>,
}

/// The normalized projection of one record: role-mapped, with CU retained.
/// Location fields (`anchor`, `title`, `test_file`) are present for index
/// display but are NOT included in the behavioral hash (see `behavioral()`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalRecord {
    pub group: String,
    pub title: String,
    pub test_name: String,
    pub test_file: String,
    pub anchor: String,
    pub verdict: String,
    pub summary: String,
    pub frames: Vec<NormalFrame>,
}

/// The behavioral signature hashed by the fingerprint: outcome + execution,
/// with location (`anchor`/`title`/`test_file`) deliberately excluded so a
/// code move or rename cannot churn the gate. CU stays in (it is deterministic
/// here and a real signal).
#[derive(serde::Serialize)]
pub struct BehavioralView<'a> {
    pub verdict: &'a str,
    pub summary: &'a str,
    pub frames: &'a [NormalFrame],
}

impl NormalRecord {
    pub fn behavioral(&self) -> BehavioralView<'_> {
        BehavioralView { verdict: &self.verdict, summary: &self.summary, frames: &self.frames }
    }
}

fn role_of(program: &str, aliases: &HashMap<String, String>) -> String {
    aliases.get(program).cloned().unwrap_or_else(|| program.to_string())
}

/// Role-map one decoded operand: a `Pubkey` becomes its role (the instruction
/// counterpart to [`role_of`], so the signature reads in role terms and stays
/// stable while catching a wrong-recipient mutant); a `Lamports` becomes its
/// numeric value (a wrong-amount mutant still changes the hash).
fn normalize_operand(
    operand: &crate::report::observation::FactOperand,
    aliases: &HashMap<String, String>,
) -> String {
    use crate::report::observation::FactOperand;
    match operand {
        FactOperand::Pubkey(key) => role_of(key, aliases),
        FactOperand::Lamports(l) => l.to_string(),
    }
}

fn normalize_frame(f: &FactFrame, aliases: &HashMap<String, String>) -> NormalFrame {
    NormalFrame {
        program: role_of(&f.program, aliases),
        instruction: f.instruction_name.clone(),
        operands: f
            .operands
            .iter()
            .map(|(k, v)| (k.clone(), normalize_operand(v, aliases)))
            .collect(),
        outcome: f.outcome.clone(),
        compute_units: f.compute_units,
        children: f.children.iter().map(|c| normalize_frame(c, aliases)).collect(),
    }
}

/// The default transform. Maps program ids to role labels, retains CU, projects
/// the verdict and summary.
pub fn normalize_default(record: &ReportRecord) -> NormalRecord {
    let aliases: HashMap<String, String> = record.aliases.iter().cloned().collect();
    NormalRecord {
        group: record.group.clone(),
        title: record.title.clone(),
        test_name: record.test_name.clone(),
        test_file: record.test_file.clone(),
        anchor: record.anchor.label(),
        verdict: verdict(record.expect, &record.facts.error).to_string(),
        summary: summary(&record.facts.error),
        frames: record.facts.frames.iter().map(|f| normalize_frame(f, &aliases)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::observation::{Anchor, ExecutionFacts, Expect, SCHEMA_VERSION};

    fn rec_with(program: &str, cu: Option<u64>, alias: Option<(&str, &str)>) -> ReportRecord {
        ReportRecord {
            schema_version: SCHEMA_VERSION,
            group: "Core".into(),
            title: "t".into(),
            test_name: "n".into(),
            test_file: "tests/x.rs".into(),
            anchor: Anchor { start: 1, end: None },
            expect: Expect::Succeeds,
            facts: ExecutionFacts {
                error: None,
                compute_units: 999,
                frames: vec![FactFrame {
                    program: program.into(),
                    instruction_name: Some("Go".into()),
                    operands: vec![],
                    outcome: "success".into(),
                    compute_units: cu,
                    children: vec![],
                }],
            },
            aliases: alias.map(|(k, v)| vec![(k.into(), v.into())]).unwrap_or_default(),
        }
    }

    #[test]
    fn keeps_cu_and_maps_program_to_role() {
        let a = normalize_default(&rec_with("PROG", Some(10), Some(("PROG", "Token"))));
        let b = normalize_default(&rec_with("PROG", Some(99), Some(("PROG", "Token"))));
        // Role IS mapped in both cases.
        assert_eq!(a.frames[0].program, "Token");
        // CU is retained, so the two records differ (10 vs 99).
        assert_ne!(a, b);
        assert_eq!(a.frames[0].compute_units, Some(10));
        assert_eq!(b.frames[0].compute_units, Some(99));
    }

    #[test]
    fn an_unaliased_program_keeps_its_id() {
        let n = normalize_default(&rec_with("PROG", None, None));
        assert_eq!(n.frames[0].program, "PROG");
    }

    #[test]
    fn role_maps_pubkey_operands_and_keeps_lamports_numeric() {
        // A decoded Transfer carries typed operands; the behavioral view maps the
        // party pubkeys to roles (stable, and a wrong-recipient mutant shifts the
        // role) and keeps the lamports numeric (a wrong-amount mutant shifts it).
        use crate::report::observation::FactOperand;
        let mut rec = rec_with("SYS", None, Some(("SYS", "System")));
        rec.aliases.push(("FROMKEY".into(), "Vault".into()));
        rec.aliases.push(("TOKEY".into(), "Player".into()));
        rec.facts.frames[0].instruction_name = Some("Transfer".into());
        rec.facts.frames[0].operands = vec![
            ("from".into(), FactOperand::Pubkey("FROMKEY".into())),
            ("to".into(), FactOperand::Pubkey("TOKEY".into())),
            ("lamports".into(), FactOperand::Lamports(1_000_000)),
        ];

        let n = normalize_default(&rec);

        assert_eq!(n.frames[0].instruction.as_deref(), Some("Transfer"));
        assert_eq!(
            n.frames[0].operands,
            vec![
                ("from".to_string(), "Vault".to_string()),
                ("to".to_string(), "Player".to_string()),
                ("lamports".to_string(), "1000000".to_string()),
            ]
        );
    }
}
