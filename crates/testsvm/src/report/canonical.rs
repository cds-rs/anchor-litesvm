//! Canonical JSON + fingerprint. Object keys are sorted recursively; arrays are
//! left in place (frame order is behavior); strings are not lowercased (case is
//! signal); the encoding is compact. Big integers reach here already as decimal
//! strings (our normalized structs carry no integers past 2^53 and no floats),
//! so the fiddly number-canonicalization of RFC 8785 does not arise.

use {serde::Serialize, sha2::{Digest, Sha256}};

/// A compact, key-sorted, array-preserving rendering of `value`.
pub fn canonical_json(value: &serde_json::Value) -> String {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let body: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", serde_json::to_string(k).unwrap(), canonical_json(&map[k])))
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Value::Array(items) => {
            let body: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", body.join(","))
        }
        // Scalars: serde_json already renders strings/bools/null canonically;
        // numbers are small/absent in our normalized structs.
        scalar => serde_json::to_string(scalar).unwrap(),
    }
}

/// Hex sha256 of the canonical encoding of `value`.
pub fn fingerprint<T: Serialize>(value: &T) -> String {
    let json = serde_json::to_value(value).expect("serialize for fingerprint");
    let canonical = canonical_json(&json);
    let digest = Sha256::digest(canonical.as_bytes());
    hex::encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_are_sorted_but_arrays_are_not() {
        let a = canonical_json(&json!({"b": 1, "a": [3, 1, 2]}));
        let b = canonical_json(&json!({"a": [3, 1, 2], "b": 1}));
        assert_eq!(a, b, "key order does not matter");
        assert_eq!(a, r#"{"a":[3,1,2],"b":1}"#, "array order is preserved");
    }

    #[test]
    fn case_is_preserved() {
        let c = canonical_json(&json!({"name": "CreateAccount"}));
        assert!(c.contains("CreateAccount"), "no lowercasing");
    }

    #[test]
    fn fingerprint_is_stable_across_key_order() {
        let x = fingerprint(&json!({"b": 1, "a": 2}));
        let y = fingerprint(&json!({"a": 2, "b": 1}));
        assert_eq!(x, y);
        assert_eq!(x.len(), 64, "hex sha256");
    }
}
