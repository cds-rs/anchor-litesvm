//! A registry of decoders for Anchor events, so a `Program data:` payload
//! renders as `🔔 Transfer { from: Alice, amount: 100 }` instead of the raw
//! base64 blob the runtime logs.
//!
//! ## The type-erasure boundary
//!
//! `litesvm-utils` carries no `anchor-lang` dependency, so it cannot name an
//! event type or call `try_from_slice` on one. The type lives entirely inside a
//! closure the caller registers: `anchor-litesvm`'s `register_event::<E>()` is
//! where the concrete event is known, and it builds
//!
//! ```ignore
//! move |bytes: &[u8]| E::try_from_slice(bytes).ok().map(|e| format!("{e:?}"))
//! ```
//!
//! then hands us `(E::DISCRIMINATOR, name, that closure)`. We store a
//! type-erased `dyn Fn(&[u8]) -> Option<String>` and never see an Anchor type.
//! The closure is `Fn` (not `FnOnce`): an event type recurs across emits and
//! transactions, so its decoder is reused, and `Arc` keeps the registry
//! `Clone` so it can ride along on every `TransactionResult` like the alias and
//! name tables do.

use base64::Engine as _;
use std::collections::HashMap;
use std::sync::Arc;

/// A decoded event: its resolved name and the formatted field body.
///
/// `fields` is the field *body* (no type name; that's [`name`](Self::name)),
/// with `Pubkey`s still in base58; the *renderer* substitutes aliases into it
/// (the same division of labour as the rest of the structured views, which
/// carry raw `Pubkey`s and let each view name them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventInfo {
    /// The event's display name, e.g. `Transfer`.
    pub name: String,
    /// The formatted field body, e.g. `{ from: <pubkey>, amount: 100 }`.
    pub fields: String,
}

impl EventInfo {
    /// The one-line badge both renderers show: `🔔 Name { fields }` (or just
    /// `🔔 Name` when the event has no fields). Centralised here so the mermaid
    /// note and the tree line can't drift.
    pub fn badge(&self) -> String {
        format!("🔔 {} {}", self.name, self.fields)
            .trim_end()
            .to_string()
    }
}

/// Decodes one event type's borsh body (the payload *after* the 8-byte
/// discriminator) into its formatted fields, or `None` if the bytes don't
/// deserialize. `Arc` so the owning [`EventRegistry`] stays `Clone`.
type Decoder = Arc<dyn Fn(&[u8]) -> Option<String> + Send + Sync>;

/// A `discriminator -> (name, decoder)` table.
///
/// Empty by default: every lookup misses, so events render as raw base64
/// exactly as they did before any registration. Populated by
/// `anchor-litesvm`'s `register_event::<E>()` (see the [module docs](self)).
#[derive(Clone, Default)]
pub struct EventRegistry {
    by_discriminator: HashMap<[u8; 8], (String, Decoder)>,
}

impl EventRegistry {
    /// An empty registry: every `decode` misses.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `discriminator -> (name, decoder)`. The discriminator is the
    /// event's 8-byte Anchor discriminator; `decode` takes the bytes that
    /// follow it and formats the fields. Called from `register_event::<E>()`,
    /// the one place the concrete event type is in scope.
    pub fn register(
        &mut self,
        discriminator: [u8; 8],
        name: impl Into<String>,
        decode: Decoder,
    ) -> &mut Self {
        self.by_discriminator
            .insert(discriminator, (name.into(), decode));
        self
    }

    /// Decode a `Program data:` base64 payload into an [`EventInfo`], or `None`
    /// when the payload isn't valid base64, is too short to carry a
    /// discriminator, or carries one we have no decoder for (each a clean miss,
    /// never a panic: an undecodable event just keeps its raw form upstream).
    pub fn decode(&self, payload_b64: &str) -> Option<EventInfo> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload_b64.trim())
            .ok()?;
        let (disc, body) = bytes.split_first_chunk::<8>()?;
        let (name, decode) = self.by_discriminator.get(disc)?;
        let fields = decode(body)?;
        Some(EventInfo {
            name: name.clone(),
            fields,
        })
    }

    /// Whether any decoder is registered. The renderers use this to skip the
    /// decode attempt entirely when no events were registered.
    pub fn is_empty(&self) -> bool {
        self.by_discriminator.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn decodes_a_registered_event_and_cleanly_misses_everything_else() {
        let disc = [1u8; 8];
        let mut reg = EventRegistry::new();
        reg.register(
            disc,
            "Ping",
            Arc::new(|b: &[u8]| {
                let n = u64::from_le_bytes(b.try_into().ok()?);
                Some(format!("{{ nonce: {n} }}"))
            }),
        );

        let mut raw = disc.to_vec();
        raw.extend_from_slice(&42u64.to_le_bytes());
        let ev = reg.decode(&b64(&raw)).expect("registered event decodes");
        assert_eq!(ev.name, "Ping");
        assert_eq!(ev.fields, "{ nonce: 42 }");
        assert_eq!(ev.badge(), "🔔 Ping { nonce: 42 }");

        let mut other = [9u8; 8].to_vec();
        other.extend_from_slice(&42u64.to_le_bytes());
        assert!(reg.decode(&b64(&other)).is_none());

        assert!(reg.decode("not base64!!!").is_none());
        assert!(reg.decode(&b64(&[1, 2, 3])).is_none());
    }
}
