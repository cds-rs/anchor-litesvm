//! Baseline diff helpers (the `fingerprint-baseline` feature): write/read a deterministic
//! fingerprint tarball and compare two record maps to surface what changed.

#[cfg(feature = "fingerprint-baseline")]
use {
    flate2::{read::GzDecoder, write::GzEncoder, Compression},
    std::{collections::BTreeMap, io::Read, path::Path},
};

/// The fixed baseline tarball name inside the report dir.
pub const BASELINE_FILE: &str = "baseline.tar.gz";

/// Pack a run into a deterministic gzip tarball at `path`: a `manifest.txt`
/// entry plus one `records/<slug>.json` entry per record. Entries are written in
/// sorted name order with normalized headers (mtime/uid/gid = 0, mode 0644) and
/// flate2's default gzip header (mtime 0), so identical inputs yield byte-
/// identical output and a committed baseline tarball does not churn.
#[cfg(feature = "fingerprint-baseline")]
pub fn write_tarball(path: &Path, manifest_text: &str, records: &[(String, String)]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create archive dir");
    }
    // Collect (entry_name, bytes) and sort by name for a stable layout.
    let mut entries: Vec<(String, Vec<u8>)> = Vec::with_capacity(records.len() + 1);
    entries.push(("manifest.txt".to_string(), manifest_text.as_bytes().to_vec()));
    for (slug, json) in records {
        entries.push((format!("records/{slug}.json"), json.as_bytes().to_vec()));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let gz = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = tar::Builder::new(gz);
    for (name, bytes) in &entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_cksum();
        builder.append_data(&mut header, name, bytes.as_slice()).expect("tar append");
    }
    let gz = builder.into_inner().expect("tar finish");
    let bytes = gz.finish().expect("gzip finish");
    std::fs::write(path, bytes).expect("write tarball");
}

/// Read the `records/<slug>.json` entries from a tarball, keyed by `slug`.
/// The `manifest.txt` entry is skipped.
#[cfg(feature = "fingerprint-baseline")]
pub fn read_tarball_records(path: &Path) -> BTreeMap<String, String> {
    let file = std::fs::File::open(path).expect("open tarball");
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let mut out = BTreeMap::new();
    for entry in archive.entries().expect("tar entries") {
        let mut entry = entry.expect("tar entry");
        let name = entry.path().expect("entry path").to_string_lossy().into_owned();
        let Some(slug) =
            name.strip_prefix("records/").and_then(|n| n.strip_suffix(".json"))
        else {
            continue;
        };
        let mut content = String::new();
        entry.read_to_string(&mut content).expect("read entry");
        out.insert(slug.to_string(), content);
    }
    out
}

/// Write the run's corpus as the committed baseline (`<dir>/baseline.tar.gz`).
#[cfg(feature = "fingerprint-baseline")]
pub fn write_baseline(dir: &Path, manifest_text: &str, records: &[(String, String)]) {
    write_tarball(&dir.join(BASELINE_FILE), manifest_text, records);
}

/// Diff `fresh` (slug -> json) against the committed baseline tarball in `dir`.
/// `None` if no baseline exists yet (nothing to diff against).
#[cfg(feature = "fingerprint-baseline")]
pub fn baseline_diff(dir: &Path, fresh: &BTreeMap<String, String>) -> Option<Vec<RecordDiff>> {
    let path = dir.join(BASELINE_FILE);
    if !path.exists() {
        return None;
    }
    Some(diff_record_maps(&read_tarball_records(&path), fresh))
}

/// How one test's captured record changed between two shapes.
#[cfg(feature = "fingerprint-baseline")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordChange {
    Added(String),
    Removed(String),
    Changed { old: String, new: String },
}

/// One test's change between the two shapes being diffed, keyed by corpus slug.
#[cfg(feature = "fingerprint-baseline")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordDiff {
    pub slug: String,
    pub change: RecordChange,
}

/// Per-slug diff of two record maps: present only in `new` is Added, only in
/// `old` is Removed, in both with different JSON is Changed. Sorted by slug.
#[cfg(feature = "fingerprint-baseline")]
pub fn diff_record_maps(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> Vec<RecordDiff> {
    let mut out = Vec::new();
    for (slug, new_json) in new {
        match old.get(slug) {
            None => out.push(RecordDiff {
                slug: slug.clone(),
                change: RecordChange::Added(new_json.clone()),
            }),
            Some(old_json) if old_json != new_json => out.push(RecordDiff {
                slug: slug.clone(),
                change: RecordChange::Changed { old: old_json.clone(), new: new_json.clone() },
            }),
            Some(_) => {}
        }
    }
    for (slug, old_json) in old {
        if !new.contains_key(slug) {
            out.push(RecordDiff {
                slug: slug.clone(),
                change: RecordChange::Removed(old_json.clone()),
            });
        }
    }
    out.sort_by(|a, b| a.slug.cmp(&b.slug));
    out
}

/// A developer report of a baseline diff, ordered by what matters: `Changed`
/// (existing behavior moved — the regression surface) first, then `Removed`
/// (confirm intentional), then `Added` (new coverage, benign). Empty -> a
/// "no change" line.
#[cfg(feature = "fingerprint-baseline")]
pub fn render_explain(diffs: &[RecordDiff]) -> String {
    if diffs.is_empty() {
        return "# Baseline: no change\n".to_string();
    }
    let count = |k: fn(&RecordChange) -> bool| diffs.iter().filter(|d| k(&d.change)).count();
    let changed = count(|c| matches!(c, RecordChange::Changed { .. }));
    let removed = count(|c| matches!(c, RecordChange::Removed(_)));
    let added = count(|c| matches!(c, RecordChange::Added(_)));
    let mut md = format!(
        "# Baseline diff\n\n**{changed} changed (review), {removed} removed (confirm), {added} added (new coverage).**\n"
    );
    let mut section = |title: &str, want: fn(&RecordChange) -> bool| {
        for d in diffs.iter().filter(|d| want(&d.change)) {
            match &d.change {
                RecordChange::Changed { old, new } => md.push_str(&format!(
                    "\n## {title}: {}\n\n<details><summary>before</summary>\n\n```json\n{old}\n```\n\n</details>\n\n<details><summary>after</summary>\n\n```json\n{new}\n```\n\n</details>\n",
                    d.slug
                )),
                RecordChange::Removed(old) => md.push_str(&format!("\n## {title}: {}\n\n```json\n{old}\n```\n", d.slug)),
                RecordChange::Added(new) => md.push_str(&format!("\n## {title}: {}\n\n```json\n{new}\n```\n", d.slug)),
            }
        }
    };
    section("Changed", |c| matches!(c, RecordChange::Changed { .. }));
    section("Removed", |c| matches!(c, RecordChange::Removed(_)));
    section("Added", |c| matches!(c, RecordChange::Added(_)));
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "fingerprint-baseline")]
    #[test]
    fn tarball_round_trips_and_is_byte_deterministic() {
        let dir = std::env::temp_dir().join("testsvm-baseline-tar");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let records = vec![
            ("core__create".to_string(), r#"{"a":1}"#.to_string()),
            ("core__burn".to_string(), r#"{"b":2}"#.to_string()),
        ];
        let p1 = dir.join("one.tar.gz");
        let p2 = dir.join("two.tar.gz");
        write_tarball(&p1, "manifest-text", &records);
        write_tarball(&p2, "manifest-text", &records);
        // Deterministic: identical inputs -> byte-identical archives.
        assert_eq!(std::fs::read(&p1).unwrap(), std::fs::read(&p2).unwrap());
        // Round-trips the records (manifest.txt skipped).
        let back = read_tarball_records(&p1);
        assert_eq!(back.len(), 2);
        assert_eq!(back.get("core__create").unwrap(), r#"{"a":1}"#);
        assert_eq!(back.get("core__burn").unwrap(), r#"{"b":2}"#);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(feature = "fingerprint-baseline")]
    #[test]
    fn diff_reports_added_removed_changed_by_slug() {
        use std::collections::BTreeMap;
        let old: BTreeMap<String, String> =
            [("a".into(), "1".into()), ("b".into(), "2".into())].into();
        let new: BTreeMap<String, String> =
            [("b".into(), "CHANGED".into()), ("c".into(), "3".into())].into();
        let diffs = diff_record_maps(&old, &new);
        assert_eq!(diffs, vec![
            RecordDiff { slug: "a".into(), change: RecordChange::Removed("1".into()) },
            RecordDiff { slug: "b".into(), change: RecordChange::Changed { old: "2".into(), new: "CHANGED".into() } },
            RecordDiff { slug: "c".into(), change: RecordChange::Added("3".into()) },
        ]);
        let md = render_explain(&diffs);
        assert!(md.contains("1 changed (review), 1 removed (confirm), 1 added (new coverage)"));
        assert!(md.find("Changed: b").unwrap() < md.find("Added: c").unwrap(), "Changed leads, Added trails");
        assert!(render_explain(&[]).contains("no change"));
    }

    #[cfg(feature = "fingerprint-baseline")]
    #[test]
    fn baseline_diff_against_committed_baseline() {
        let dir = std::env::temp_dir().join("testsvm-baseline-baseline");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fresh: BTreeMap<String, String> = [("g__a".into(), r#"{"v":"B"}"#.into())].into();
        assert!(baseline_diff(&dir, &fresh).is_none(), "no baseline yet");
        write_baseline(&dir, "m", &[("g__a".to_string(), r#"{"v":"A"}"#.to_string())]);
        let diffs = baseline_diff(&dir, &fresh).expect("baseline exists");
        assert_eq!(diffs.len(), 1);
        assert!(matches!(diffs[0].change, RecordChange::Changed { .. }));
        std::fs::remove_dir_all(&dir).ok();
    }
}
