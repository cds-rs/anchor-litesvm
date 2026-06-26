//! The Reporter: read a `target/test-results/` corpus, normalize each record
//! (default, or a registered group override), render a sectioned index, and
//! fold the Merkle fingerprint. A library API here; a consumer crate drives it
//! from a small binary after its suite has written the corpus.

use {
    crate::report::{
        fingerprint::{merkle, Manifest},
        normalize::{normalize_default, NormalRecord},
        observation::ReportRecord,
    },
    std::collections::HashMap,
};

pub struct Reporter {
    records: Vec<ReportRecord>,
    order: Vec<String>,
    overrides: HashMap<String, fn(&ReportRecord) -> NormalRecord>,
}

impl Reporter {
    /// Load every `*.json` record under `dir`, rejecting a foreign schema.
    pub fn from_dir(dir: &str) -> Reporter {
        let mut records = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {dir}: {e}")) {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let rec: ReportRecord =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("parse {path:?}: {e}"));
            assert_eq!(rec.schema_version, 1, "record {path:?} has an unsupported schema");
            records.push(rec);
        }
        Reporter { records, order: Vec::new(), overrides: HashMap::new() }
    }

    /// Section order for the index (groups not listed fall to the end,
    /// alphabetically).
    pub fn group_order(mut self, order: &[&str]) -> Self {
        self.order = order.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Register a program-specific normalize for one group.
    pub fn override_records(mut self, group: &str, f: fn(&ReportRecord) -> NormalRecord) -> Self {
        self.overrides.insert(group.to_string(), f);
        self
    }

    fn normalize(&self, rec: &ReportRecord) -> NormalRecord {
        match self.overrides.get(&rec.group) {
            Some(f) => f(rec),
            None => normalize_default(rec),
        }
    }

    /// Groups as `(name, Vec<(anchor_start, NormalRecord)>)`, in `group_order`
    /// then alphabetical.
    fn grouped(&self) -> Vec<(String, Vec<(u32, NormalRecord)>)> {
        let mut by_group: HashMap<String, Vec<(u32, NormalRecord)>> = HashMap::new();
        for rec in &self.records {
            by_group
                .entry(rec.group.clone())
                .or_default()
                .push((rec.anchor.start, self.normalize(rec)));
        }
        let mut names: Vec<String> = by_group.keys().cloned().collect();
        names.sort();
        names.sort_by_key(|n| self.order.iter().position(|o| o == n).unwrap_or(usize::MAX));
        names
            .into_iter()
            .map(|n| {
                let mut members = by_group.remove(&n).unwrap();
                members.sort_by_key(|(line, _)| *line);
                (n, members)
            })
            .collect()
    }

    pub fn manifest(&self) -> Manifest {
        merkle(&self.grouped())
    }

    /// The sectioned index: heading, intro, count, then a table per group
    /// (verdict as a glyph, the summary, a source link).
    pub fn index(&self, heading: &str, intro: &str) -> String {
        let grouped = self.grouped();
        let count: usize = grouped.iter().map(|(_, m)| m.len()).sum();
        let mut md = format!("# {heading}\n\n{intro}\n\n**{count} scenarios.**\n");
        for (group, members) in &grouped {
            md.push_str(&format!("\n## {group}\n\n| Scenario | Verdict | Summary | Source |\n|---|---|---|---|\n"));
            for (_, rec) in members {
                let glyph = if rec.verdict == "passed" { "✅ passed" } else { "❌ failed" };
                let source = format!(
                    "[{}::{}](../{}\u{23}{})",
                    rec.test_file, rec.test_name, rec.test_file, rec.anchor
                );
                md.push_str(&format!("| {} | {} | {} | {} |\n", rec.title, glyph, rec.summary, source));
            }
        }
        md
    }

    pub fn write(&self, out_dir: &str, heading: &str, intro: &str) {
        std::fs::create_dir_all(out_dir).unwrap();
        std::fs::write(format!("{out_dir}/index.md"), self.index(heading, intro)).unwrap();
        std::fs::write(format!("{out_dir}/fingerprint.txt"), self.manifest().render()).unwrap();
    }

    /// Diff this corpus's fresh fingerprint against a committed manifest.
    pub fn verify(
        &self,
        committed: &crate::report::fingerprint::Manifest,
    ) -> Vec<crate::report::fingerprint::Change> {
        crate::report::fingerprint::diff(committed, &self.manifest())
    }

    /// A developer-facing report of the changes: each names its group and test,
    /// and for an Added or Changed test embeds the captured observation JSON so
    /// the developer sees exactly what the run produced.
    pub fn render_changes(&self, changes: &[crate::report::fingerprint::Change]) -> String {
        use crate::report::fingerprint::ChangeKind;
        if changes.is_empty() {
            return "# Fingerprint: no changes\n".to_string();
        }
        let mut md = format!("# Fingerprint changes\n\n**{} changed.**\n", changes.len());
        for ch in changes {
            let kind = match ch.kind {
                ChangeKind::Added => "Added",
                ChangeKind::Removed => "Removed",
                ChangeKind::Changed => "Changed",
            };
            md.push_str(&format!("\n## {kind}: {} :: {}\n", ch.group, ch.test_name));
            if ch.kind != ChangeKind::Removed {
                if let Some(rec) = self
                    .records
                    .iter()
                    .find(|r| r.group == ch.group && r.test_name == ch.test_name)
                {
                    let json = serde_json::to_string_pretty(rec).unwrap();
                    md.push_str(&format!(
                        "\n<details><summary>captured observation</summary>\n\n```json\n{json}\n```\n\n</details>\n"
                    ));
                }
            }
        }
        md
    }
}

#[cfg(feature = "fingerprint-baseline")]
impl Reporter {
    /// The slug -> pretty-JSON map of the current run's records.
    fn record_map(&self) -> std::collections::BTreeMap<String, String> {
        self.records
            .iter()
            .map(|r| {
                (
                    crate::report::observation::record_slug(&r.test_file, &r.test_name),
                    serde_json::to_string_pretty(r).expect("serialize record"),
                )
            })
            .collect()
    }

    /// Write the current run as the committed baseline (`<dir>/baseline.tar.gz`).
    pub fn write_baseline(&self, dir: &std::path::Path) {
        let manifest = self.manifest();
        let records: Vec<(String, String)> = self.record_map().into_iter().collect();
        crate::report::baseline::write_baseline(dir, &manifest.render(), &records);
    }

    /// Diff the current run against the committed baseline tarball in `dir`.
    pub fn explain(&self, dir: &std::path::Path) -> Option<Vec<crate::report::baseline::RecordDiff>> {
        crate::report::baseline::baseline_diff(dir, &self.record_map())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::observation::{Anchor, ExecutionFacts, Expect, ReportRecord, SCHEMA_VERSION};

    fn write_corpus(dir: &std::path::Path, recs: &[ReportRecord]) {
        std::fs::create_dir_all(dir).unwrap();
        for r in recs {
            std::fs::write(dir.join(format!("{}.json", r.test_name)), serde_json::to_vec(r).unwrap()).unwrap();
        }
    }

    fn rec(group: &str, name: &str, line: u32) -> ReportRecord {
        ReportRecord {
            schema_version: SCHEMA_VERSION,
            group: group.into(),
            title: name.into(),
            test_name: name.into(),
            test_file: "tests/x.rs".into(),
            anchor: Anchor { start: line, end: None },
            expect: Expect::Succeeds,
            facts: ExecutionFacts { error: None, compute_units: 0, frames: vec![] },
            aliases: vec![],
        }
    }

    #[test]
    fn index_sections_in_declared_order_and_fingerprint_is_stable() {
        let dir = std::env::temp_dir().join("testsvm-reporter-test");
        let _ = std::fs::remove_dir_all(&dir);
        write_corpus(&dir, &[rec("Beta", "b1", 10), rec("Alpha", "a1", 20), rec("Alpha", "a2", 5)]);

        let r = Reporter::from_dir(dir.to_str().unwrap()).group_order(&["Alpha", "Beta"]);
        let index = r.index("H", "intro");
        let alpha = index.find("## Alpha").unwrap();
        let beta = index.find("## Beta").unwrap();
        assert!(alpha < beta, "declared order wins");
        // Within Alpha, a2 (line 5) precedes a1 (line 20).
        let a2 = index.find("| a2 |").unwrap();
        let a1 = index.find("| a1 |").unwrap();
        assert!(a2 < a1, "source order within a group");
        assert_eq!(index.matches("scenarios.").count(), 1);

        let root1 = r.manifest().root;
        let r2 = Reporter::from_dir(dir.to_str().unwrap()).group_order(&["Alpha", "Beta"]);
        assert_eq!(root1, r2.manifest().root, "deterministic");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_override_replaces_the_default_for_its_group() {
        fn shout(rec: &ReportRecord) -> crate::report::normalize::NormalRecord {
            let mut n = crate::report::normalize::normalize_default(rec);
            n.summary = "OVERRIDDEN".into();
            n
        }
        let dir = std::env::temp_dir().join("testsvm-reporter-override");
        let _ = std::fs::remove_dir_all(&dir);
        write_corpus(&dir, &[rec("Alpha", "a1", 1)]);
        let r = Reporter::from_dir(dir.to_str().unwrap()).override_records("Alpha", shout);
        assert!(r.index("H", "i").contains("OVERRIDDEN"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_names_the_changed_test_and_embeds_its_json() {
        use crate::report::fingerprint::{ChangeKind, Manifest};
        let dir = std::env::temp_dir().join("testsvm-verify");
        let _ = std::fs::remove_dir_all(&dir);
        write_corpus(&dir, &[rec("Core", "a1", 1)]);
        let committed = Reporter::from_dir(dir.to_str().unwrap()).manifest().render();

        // Mutate the captured observation: a1 now carries an error.
        let mut changed = rec("Core", "a1", 1);
        changed.facts.error = Some("boom".into());
        write_corpus(&dir, &[changed]);

        let r = Reporter::from_dir(dir.to_str().unwrap());
        let changes = r.verify(&Manifest::parse(&committed));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Changed);
        assert_eq!(changes[0].test_name, "a1");
        assert_eq!(changes[0].group, "Core");

        let report = r.render_changes(&changes);
        assert!(report.contains("Changed: Core :: a1"));
        assert!(report.contains("captured observation"));
        assert!(report.contains("boom"), "the captured JSON is embedded for context");
        std::fs::remove_dir_all(&dir).ok();
    }
}
