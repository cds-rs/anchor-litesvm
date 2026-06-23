//! The Merkle fold: per-record hash (behavioral view only: CU in, location out)
//! -> per-group hash (members sorted by test_name for stability across source
//! moves) -> suite root. The manifest is the committed regression anchor; a
//! root mismatch walks down to the group, then to the one record.

use {
    crate::report::{canonical::fingerprint, normalize::NormalRecord},
    sha2::{Digest, Sha256},
};

fn hash_str(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// The committed fingerprint file: every record (with its group, so a mismatch
/// can name the test and group for the developer), every group, the root.
pub struct Manifest {
    pub records: Vec<(String, String, String)>, // (group, test_name, hash), sorted
    pub groups: Vec<(String, String)>,           // (group_name, hash), sorted
    pub root: String,
}

impl Manifest {
    pub fn render(&self) -> String {
        let mut out = String::from("# fingerprint v1\n\n## records\n");
        for (group, name, hash) in &self.records {
            out.push_str(&format!("{group}\t{name}\t{hash}\n"));
        }
        out.push_str("\n## groups\n");
        for (name, hash) in &self.groups {
            out.push_str(&format!("{name}\t{hash}\n"));
        }
        out.push_str(&format!("\n## root\n{}\n", self.root));
        out
    }

    /// Parse a rendered manifest back, to compare a committed file against a
    /// freshly computed one.
    pub fn parse(text: &str) -> Manifest {
        let (mut records, mut groups, mut root, mut section) = (Vec::new(), Vec::new(), String::new(), "");
        for line in text.lines() {
            match line.trim() {
                "## records" => section = "records",
                "## groups" => section = "groups",
                "## root" => section = "root",
                "" => {}
                l if l.starts_with('#') => {}
                l => {
                    let p: Vec<&str> = l.split('\t').collect();
                    match section {
                        "records" if p.len() == 3 => records.push((p[0].into(), p[1].into(), p[2].into())),
                        "groups" if p.len() == 2 => groups.push((p[0].into(), p[1].into())),
                        "root" => root = l.into(),
                        _ => {}
                    }
                }
            }
        }
        Manifest { records, groups, root }
    }
}

/// Fold groups of `(anchor_start, record)` into a manifest. Members are folded
/// in `test_name` order (stable across source moves); only the behavioral view
/// is hashed (CU in, location out). The records and groups sections are sorted
/// by name so the file is stable; the root folds the group hashes in
/// group-name order.
pub fn merkle(groups: &[(String, Vec<(u32, NormalRecord)>)]) -> Manifest {
    let mut record_hashes: Vec<(String, String, String)> = Vec::new();
    let mut group_hashes: Vec<(String, String)> = Vec::new();

    for (group_name, members) in groups {
        let mut ordered = members.clone();
        ordered.sort_by(|(_, a), (_, b)| a.test_name.cmp(&b.test_name));
        let mut group_pre = group_name.clone();
        for (_, rec) in &ordered {
            let h = fingerprint(&rec.behavioral());
            group_pre.push_str(&h);
            record_hashes.push((group_name.clone(), rec.test_name.clone(), h));
        }
        group_hashes.push((group_name.clone(), hash_str(&group_pre)));
    }

    record_hashes.sort();
    group_hashes.sort();
    let root_pre: String = group_hashes.iter().map(|(_, h)| h.as_str()).collect();
    Manifest {
        records: record_hashes,
        groups: group_hashes,
        root: hash_str(&root_pre),
    }
}

/// One difference between a committed manifest and a freshly computed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub group: String,
    pub test_name: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
}

/// Compare two manifests by `(group, test_name)`: present only in `current` is
/// Added, only in `committed` is Removed, in both with a different hash is
/// Changed. Sorted, for a stable report.
pub fn diff(committed: &Manifest, current: &Manifest) -> Vec<Change> {
    use std::collections::BTreeMap;
    let key = |g: &str, n: &str| format!("{g}\t{n}");
    let c: BTreeMap<String, &str> =
        committed.records.iter().map(|(g, n, h)| (key(g, n), h.as_str())).collect();
    let n: BTreeMap<String, &str> =
        current.records.iter().map(|(g, nm, h)| (key(g, nm), h.as_str())).collect();
    let mut out = Vec::new();
    for (k, ch) in &n {
        let (g, nm) = k.split_once('\t').unwrap();
        match c.get(k) {
            None => out.push(Change { group: g.into(), test_name: nm.into(), kind: ChangeKind::Added }),
            Some(committed_hash) if committed_hash != ch => {
                out.push(Change { group: g.into(), test_name: nm.into(), kind: ChangeKind::Changed })
            }
            Some(_) => {}
        }
    }
    for k in c.keys() {
        if !n.contains_key(k) {
            let (g, nm) = k.split_once('\t').unwrap();
            out.push(Change { group: g.into(), test_name: nm.into(), kind: ChangeKind::Removed });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{
        canonical::fingerprint,
        normalize::{NormalFrame, NormalRecord},
    };

    fn nr(name: &str, summary: &str) -> NormalRecord {
        NormalRecord {
            group: "G".into(),
            title: name.into(),
            test_name: name.into(),
            test_file: "tests/x.rs".into(),
            anchor: "L1".into(),
            verdict: "passed".into(),
            summary: summary.into(),
            frames: vec![],
        }
    }

    fn nr_with_cu(name: &str, summary: &str, cu: Option<u64>) -> NormalRecord {
        NormalRecord {
            group: "G".into(),
            title: name.into(),
            test_name: name.into(),
            test_file: "tests/x.rs".into(),
            anchor: "L1".into(),
            verdict: "passed".into(),
            summary: summary.into(),
            frames: vec![NormalFrame {
                program: "Token".into(),
                instruction: None,
                outcome: "success".into(),
                compute_units: cu,
                children: vec![],
            }],
        }
    }

    #[test]
    fn source_order_within_a_group_does_not_change_the_root() {
        let g1 = vec![("G".to_string(), vec![(10, nr("a", "x")), (20, nr("b", "y"))])];
        let g2 = vec![("G".to_string(), vec![(20, nr("b", "y")), (10, nr("a", "x"))])];
        assert_eq!(merkle(&g1).root, merkle(&g2).root, "fold sorts by test_name");
    }

    #[test]
    fn location_does_not_change_the_per_record_fingerprint() {
        // Two records identical in behavior (verdict + summary + frames) but
        // differing in location fields (anchor, title, test_file) must hash the
        // same: location is presentation, not execution.
        let base = NormalRecord {
            group: "G".into(),
            title: "Original title".into(),
            test_name: "test_transfer".into(),
            test_file: "tests/a.rs".into(),
            anchor: "L10".into(),
            verdict: "passed".into(),
            summary: "ok".into(),
            frames: vec![NormalFrame {
                program: "Token".into(),
                instruction: None,
                outcome: "success".into(),
                compute_units: Some(5),
                children: vec![],
            }],
        };
        let moved = NormalRecord {
            title: "Renamed title".into(),
            test_file: "tests/b.rs".into(),
            anchor: "L99".into(),
            ..base.clone()
        };
        assert_eq!(
            fingerprint(&base.behavioral()),
            fingerprint(&moved.behavioral()),
            "location change must not change the fingerprint"
        );
    }

    #[test]
    fn cu_is_part_of_the_fingerprint() {
        // A CU shift is a deterministic signal (pinned .so + locked VM), so it
        // must change the per-record hash.
        let low_cu = nr_with_cu("test_transfer", "ok", Some(5));
        let high_cu = nr_with_cu("test_transfer", "ok", Some(9999));
        assert_ne!(
            fingerprint(&low_cu.behavioral()),
            fingerprint(&high_cu.behavioral()),
            "different CU must yield different fingerprints"
        );
    }

    #[test]
    fn a_changed_record_moves_its_group_and_the_root() {
        let base = vec![("G".to_string(), vec![(10, nr("a", "x"))])];
        let changed = vec![("G".to_string(), vec![(10, nr("a", "CHANGED"))])];
        let m0 = merkle(&base);
        let m1 = merkle(&changed);
        assert_ne!(m0.root, m1.root);
        assert_ne!(m0.records[0].2, m1.records[0].2); // hash is the third field
    }

    #[test]
    fn manifest_round_trips_through_parse() {
        let m = merkle(&vec![("G".to_string(), vec![(10, nr("a", "x")), (20, nr("b", "y"))])]);
        let back = Manifest::parse(&m.render());
        assert_eq!(back.records, m.records);
        assert_eq!(back.groups, m.groups);
        assert_eq!(back.root, m.root);
    }

    #[test]
    fn diff_names_the_changed_test_and_its_group() {
        let committed = merkle(&vec![(
            "Account ownership".to_string(),
            vec![(10, nr("a", "x")), (20, nr("b", "y"))],
        )]);
        let current = merkle(&vec![(
            "Account ownership".to_string(),
            vec![(10, nr("a", "x")), (20, nr("b", "CHANGED")), (30, nr("c", "z"))],
        )]);
        let changes = diff(&committed, &current);
        // b changed, c added; a unchanged (absent).
        assert!(changes.contains(&Change {
            group: "Account ownership".into(),
            test_name: "b".into(),
            kind: ChangeKind::Changed,
        }));
        assert!(changes.contains(&Change {
            group: "Account ownership".into(),
            test_name: "c".into(),
            kind: ChangeKind::Added,
        }));
        assert!(!changes.iter().any(|c| c.test_name == "a"));
    }
}
