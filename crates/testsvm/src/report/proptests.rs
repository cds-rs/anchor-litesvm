//! Property tests for the fingerprint subsystem's pure functions.
//!
//! Coverage axes:
//!   - Idempotence / determinism: the same input always yields the same output.
//!   - Canonical encoding invariants: key order vs. array order, case.
//!   - Big-number (BN) exactness: u64 values up to 2^64-1 round-trip without
//!     float precision loss.
//!   - Variance: the behavioral hash changes iff the behavioral input changes
//!     (location fields are excluded, content fields are included).
//!   - Merkle fold stability: member order within a group doesn't change the root.

use proptest::prelude::*;
use serde_json::json;

use crate::report::{canonical_json, fingerprint, merkle, NormalFrame, NormalRecord};

// ---------------------------------------------------------------------------
// Leaf strategies
// ---------------------------------------------------------------------------

/// A `NormalFrame` with no children (leaf node). Bounded string lengths keep
/// proptest shrinking fast.
fn leaf_frame() -> impl Strategy<Value = NormalFrame> {
    (
        "[a-zA-Z0-9_]{1,20}",
        proptest::option::of("[a-zA-Z0-9_]{1,20}"),
        "[a-zA-Z0-9_]{1,20}",
        proptest::option::of(any::<u64>()),
    )
        .prop_map(|(program, instruction, outcome, compute_units)| NormalFrame {
            program,
            instruction,
            operands: vec![],
            outcome,
            compute_units,
            children: vec![],
        })
}

/// A `NormalFrame` with up to 3 children (each a leaf). One level of nesting
/// is enough to exercise the recursive Serialize path without exploding the
/// search space.
fn frame_with_children() -> impl Strategy<Value = NormalFrame> {
    (
        "[a-zA-Z0-9_]{1,20}",
        proptest::option::of("[a-zA-Z0-9_]{1,20}"),
        "[a-zA-Z0-9_]{1,20}",
        proptest::option::of(any::<u64>()),
        prop::collection::vec(leaf_frame(), 0..=3),
    )
        .prop_map(|(program, instruction, outcome, compute_units, children)| NormalFrame {
            program,
            instruction,
            operands: vec![],
            outcome,
            compute_units,
            children,
        })
}

/// A `NormalRecord` with small frame vecs (0-3 frames, each with 0-3 children).
fn normal_record() -> impl Strategy<Value = NormalRecord> {
    (
        "[a-zA-Z0-9 _]{1,20}",
        "[a-zA-Z0-9 _]{1,20}",
        "[a-zA-Z0-9_]{1,20}",
        "[a-zA-Z0-9/_\\.]{1,30}",
        "[a-zA-Z0-9:]{1,20}",
        "[a-zA-Z0-9_ ]{1,20}",
        "[a-zA-Z0-9 _\\.!]{1,40}",
        prop::collection::vec(frame_with_children(), 0..=3),
    )
        .prop_map(|(group, title, test_name, test_file, anchor, verdict, summary, frames)| {
            NormalRecord { group, title, test_name, test_file, anchor, verdict, summary, frames }
        })
}

// ---------------------------------------------------------------------------
// 1. Idempotence / determinism
// ---------------------------------------------------------------------------

proptest! {
    /// The behavioral fingerprint of any record is stable across two calls.
    #[test]
    fn fingerprint_is_idempotent(r in normal_record()) {
        prop_assert_eq!(fingerprint(&r.behavioral()), fingerprint(&r.behavioral()));
    }

    /// canonical_json produces identical output on repeated calls for the same
    /// input. We derive the value from a map of String→i64 so the JSON object
    /// has a variety of key orderings in the input HashMap.
    #[test]
    fn canonical_json_is_idempotent(
        kvs in prop::collection::hash_map("[a-z]{1,10}", any::<i64>(), 0..=8)
    ) {
        let v: serde_json::Value =
            serde_json::Value::Object(kvs.iter().map(|(k, i)| (k.clone(), json!(*i))).collect());
        prop_assert_eq!(canonical_json(&v), canonical_json(&v));
    }
}

// ---------------------------------------------------------------------------
// 2. Canonical encoding invariants
// ---------------------------------------------------------------------------

proptest! {
    /// Object key insertion order does NOT change the canonical encoding.
    #[test]
    fn key_order_does_not_matter(
        // A vec of (key, value) pairs; we'll dedup by key to ensure uniqueness.
        pairs in prop::collection::vec(("[a-z]{1,10}", any::<i64>()), 1..=8)
    ) {
        // Dedup by key: last writer wins (matches what serde_json Map would do).
        let mut seen = std::collections::HashSet::new();
        let unique: Vec<(String, i64)> = pairs
            .into_iter()
            .rev()
            .filter(|(k, _)| seen.insert(k.clone()))
            .collect();

        let forward: serde_json::Map<String, serde_json::Value> =
            unique.iter().map(|(k, v)| (k.clone(), json!(*v))).collect();
        let backward: serde_json::Map<String, serde_json::Value> =
            unique.iter().rev().map(|(k, v)| (k.clone(), json!(*v))).collect();

        prop_assert_eq!(
            canonical_json(&serde_json::Value::Object(forward)),
            canonical_json(&serde_json::Value::Object(backward)),
        );
    }

    /// Array element order IS preserved: reversing a non-palindrome array
    /// changes the canonical encoding.
    #[test]
    fn array_order_is_preserved(v in prop::collection::vec(any::<i64>(), 2..=10)) {
        let mut reversed = v.clone();
        reversed.reverse();
        prop_assume!(v != reversed);
        prop_assert_ne!(canonical_json(&json!(v)), canonical_json(&json!(reversed)));
    }

    /// String case is preserved: the canonical encoding of a mixed-case string
    /// differs from the same string uppercased.
    #[test]
    fn case_is_preserved(s in "[a-zA-Z]{1,20}") {
        prop_assume!(s != s.to_uppercase());
        prop_assert_ne!(canonical_json(&json!(s)), canonical_json(&json!(s.to_uppercase())));
    }
}

// ---------------------------------------------------------------------------
// 3. Big-number (BN) edge cases
// ---------------------------------------------------------------------------

proptest! {
    /// A u64 value (including values > 2^53 where f64 lossy conversion would
    /// matter) round-trips exactly through canonical_json: serialise → parse.
    #[test]
    fn u64_round_trips_exactly(n in any::<u64>()) {
        let encoded = canonical_json(&json!(n));
        let back: u64 = encoded.parse().unwrap_or_else(|e| {
            panic!("canonical_json of u64 {n} produced {encoded:?}, which does not parse as u64: {e}")
        });
        prop_assert_eq!(back, n);
    }

    /// Two distinct u64 values always produce distinct fingerprints, proving no
    /// float-precision collapse on the way through canonical_json.
    #[test]
    fn distinct_u64_do_not_collide(a in any::<u64>(), b in any::<u64>()) {
        prop_assume!(a != b);
        prop_assert_ne!(fingerprint(&json!(a)), fingerprint(&json!(b)));
    }
}

/// Pin the f64 precision boundary through the ACTUAL CU struct path
/// (`NormalFrame.compute_units: Option<u64>` -> `behavioral()` -> `fingerprint`),
/// not just through a bare `json!(u64)`. 2^53+1 and 2^53+2 are distinct u64 but
/// collide as f64 (2^53+1 rounds to 2^53), so if CU ever routed through f64
/// these two records would hash equal.
#[test]
fn adjacent_large_cu_do_not_collide_through_the_record_path() {
    let mk = |cu: u64| NormalRecord {
        group: "g".into(),
        title: "t".into(),
        test_name: "n".into(),
        test_file: "f".into(),
        anchor: "L1".into(),
        verdict: "passed".into(),
        summary: "ok".into(),
        frames: vec![NormalFrame {
            program: "P".into(),
            instruction: None,
            operands: vec![],
            outcome: "success".into(),
            compute_units: Some(cu),
            children: vec![],
        }],
    };
    let a = fingerprint(&mk(9_007_199_254_740_993).behavioral()); // 2^53 + 1
    let b = fingerprint(&mk(9_007_199_254_740_994).behavioral()); // 2^53 + 2
    assert_ne!(a, b, "adjacent CU across the f64 boundary must not collide");
}

// ---------------------------------------------------------------------------
// 4. Variance: hash changes iff behavioral input changes
// ---------------------------------------------------------------------------

proptest! {
    /// Changing only location fields (anchor, title, test_file) does NOT change
    /// the behavioral fingerprint.
    #[test]
    fn location_change_does_not_change_fingerprint(
        r in normal_record(),
        new_anchor in "[a-zA-Z0-9:]{1,20}",
        new_title in "[a-zA-Z0-9 _]{1,20}",
        new_test_file in "[a-zA-Z0-9/_\\.]{1,30}",
    ) {
        // Guard against a vacuous idempotence check: at least one location field
        // must actually differ for this to test location-invariance.
        prop_assume!(new_anchor != r.anchor || new_title != r.title || new_test_file != r.test_file);
        let r2 = NormalRecord {
            anchor: new_anchor,
            title: new_title,
            test_file: new_test_file,
            ..r.clone()
        };
        prop_assert_eq!(fingerprint(&r.behavioral()), fingerprint(&r2.behavioral()));
    }

    /// Changing the summary (a behavioral field) DOES change the fingerprint.
    #[test]
    fn behavioral_change_changes_fingerprint(r in normal_record()) {
        let new_summary = format!("{}!CHANGED", r.summary);
        let r2 = NormalRecord { summary: new_summary, ..r.clone() };
        prop_assert_ne!(fingerprint(&r.behavioral()), fingerprint(&r2.behavioral()));
    }
}

// ---------------------------------------------------------------------------
// 5. Merkle fold
// ---------------------------------------------------------------------------

/// Build a minimal groups input: one group whose members are the given
/// already-paired `(anchor_start, NormalRecord)` vec.
fn one_group(members: Vec<(u32, NormalRecord)>) -> Vec<(String, Vec<(u32, NormalRecord)>)> {
    vec![("G".to_string(), members)]
}

/// Pair each record with a sequential anchor and a unique test_name.
/// Returns the paired members in the given order. Relabelling happens BEFORE
/// any reordering so that forward and reversed variants differ only in insertion
/// order, not in which content is attached to which test_name.
fn pair_records(records: Vec<NormalRecord>) -> Vec<(u32, NormalRecord)> {
    records
        .into_iter()
        .enumerate()
        .map(|(i, mut r)| {
            r.test_name = format!("t{i:02}"); // zero-pad so lex order == insertion order
            (i as u32, r)
        })
        .collect()
}

proptest! {
    /// merkle is deterministic: called twice on the same input the root is
    /// identical.
    #[test]
    fn merkle_is_idempotent(records in prop::collection::vec(normal_record(), 0..=4)) {
        let groups = one_group(pair_records(records));
        prop_assert_eq!(merkle(&groups).root, merkle(&groups).root);
    }

    /// Member order within a group does NOT change the Merkle root: the fold
    /// sorts by test_name, so insertion order is irrelevant. We pair records
    /// first (giving them unique, stable test_names), then reverse the already-
    /// paired members to change only their insertion order.
    #[test]
    fn member_reorder_does_not_change_root(
        records in prop::collection::vec(normal_record(), 1..=4)
    ) {
        let paired = pair_records(records);
        let mut reversed = paired.clone();
        reversed.reverse();
        prop_assert_eq!(
            merkle(&one_group(paired)).root,
            merkle(&one_group(reversed)).root,
        );
    }
}
