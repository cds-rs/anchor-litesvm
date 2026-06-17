//! The event-decode vocabulary now lives in `testsvm` (beside the instruction
//! and error tables) so the [`TestSVM`](testsvm::TestSVM) trait can expose event
//! registration as a backend socket. This module re-exports it, and owns the one
//! piece that is a *log-format* detail rather than vocabulary: stripping the
//! base64 framing off a `Program data:` line before handing the bytes to the
//! registry. See [`testsvm::events`].

pub use testsvm::events::{EventInfo, EventRegistry};

use base64::Engine as _;

/// Decode a `Program data:` base64 payload into an [`EventInfo`] via `registry`,
/// or `None` when the payload isn't valid base64, is too short to carry a
/// discriminator, or carries one with no decoder (each a clean miss). The
/// base64 framing is stripped here; the registry decodes the resulting bytes.
pub(crate) fn decode_program_data(
    registry: &EventRegistry,
    payload_b64: &str,
) -> Option<EventInfo> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload_b64.trim())
        .ok()?;
    registry.decode_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn base64_framing_is_stripped_before_the_registry_decodes() {
        let disc = [1u8; 8];
        let mut reg = EventRegistry::new();
        reg.register(
            disc,
            "Ping",
            Arc::new(|b: &[u8]| {
                let n = u64::from_le_bytes(b.try_into().ok()?);
                Some(vec![("nonce".to_string(), n.to_string())])
            }),
        );

        let mut raw = disc.to_vec();
        raw.extend_from_slice(&42u64.to_le_bytes());
        let ev = decode_program_data(&reg, &b64(&raw)).expect("registered event decodes");
        assert_eq!(ev.badge(), "🔔 Ping { nonce: 42 }");

        // Non-base64 misses cleanly (no panic).
        assert!(decode_program_data(&reg, "not base64!!!").is_none());
    }
}
