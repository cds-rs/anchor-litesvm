//! The event-decode vocabulary: a registry of decoders so a program's emitted
//! events render as `🔔 Transfer { from: Alice, amount: 100 }` instead of an
//! opaque blob. It lives here, beside [`InstructionNames`](crate::instructions)
//! and [`ErrorNames`](crate::errors), so the [`TestSVM`](crate::TestSVM) trait
//! can expose event registration as a backend socket, uniform with the
//! discriminator and error tables.
//!
//! ## The type-erasure boundary
//!
//! `testsvm` carries no `anchor-lang` (nor any program framework) dependency,
//! so it cannot name an event type or call `try_from_slice` on one. The type
//! lives entirely inside a closure the caller registers: `anchor-litesvm`'s
//! `register_event::<E>()` is where the concrete event is known, and it builds a
//! `move |bytes| E::try_from_slice(bytes).ok().map(..)` decoder, then hands us
//! `(discriminator, name, decoder)`. We store a type-erased [`EventDecoder`] and
//! never see a framework type. The closure is `Fn` (an event recurs across
//! emits), and `Arc` keeps the registry `Clone` so it rides on every
//! transaction record like the alias and name tables do.
//!
//! ## Two emission shapes
//!
//! - **Logged** (`emit!`): the runtime writes `Program data: <base64>` where the
//!   bytes are `discriminator(8) ++ borsh`. The base64 framing is a log-format
//!   detail the renderer strips; this registry decodes the resulting bytes via
//!   [`decode_bytes`](EventRegistry::decode_bytes).
//! - **Self-CPI** (`emit_cpi!`, and compatible hand-rolled engines): the program
//!   invokes itself with `tag ++ disc ++ borsh` as the instruction data, leaving
//!   no log. The payload is the inner instruction's data, which the execution
//!   trace carries onto the frame; [`decode_cpi`](EventRegistry::decode_cpi)
//!   matches a registered prefix and decodes the remainder.

use std::collections::HashMap;
use std::sync::Arc;

use solana_pubkey::Pubkey;

/// A decoded event: its resolved name and its fields as `(name, value)` pairs.
///
/// Fields stay *structured* (not one pre-joined string) so each renderer lays
/// them out its own way: the mermaid note joins them on one line
/// ([`badge`](Self::badge)), the tree prints one aligned field per line. Values
/// keep `Pubkey`s in base58; the renderer substitutes aliases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventInfo {
    /// The event's display name, e.g. `Transfer`.
    pub name: String,
    /// The decoded fields as `(name, value)` pairs, in declaration order. Empty
    /// for a field-less event.
    pub fields: Vec<(String, String)>,
}

impl EventInfo {
    /// The one-line badge the mermaid note shows: `🔔 Name { a: 1, b: 2 }` (or
    /// just `🔔 Name` when the event has no fields).
    pub fn badge(&self) -> String {
        if self.fields.is_empty() {
            return format!("🔔 {}", self.name);
        }
        let body = self
            .fields
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("🔔 {} {{ {body} }}", self.name)
    }
}

/// Decodes one event type's borsh body into its `(field, value)` pairs, or
/// `None` if the bytes don't deserialize. `Arc` so the owning [`EventRegistry`]
/// stays `Clone`. For a logged event the body is the bytes after the 8-byte
/// discriminator; for a self-CPI event, the bytes after the registered prefix.
pub type EventDecoder = Arc<dyn Fn(&[u8]) -> Option<Vec<(String, String)>> + Send + Sync>;

/// A registry of event decoders: `discriminator -> (name, decoder)` for logged
/// events, and `prefix -> (name, decoder)` for self-CPI events.
///
/// Empty by default: every lookup misses, so events keep their raw form exactly
/// as before any registration. Populated through the [`TestSVM`](crate::TestSVM)
/// sockets `register_event_decoder` / `register_cpi_event`.
#[derive(Clone, Default)]
pub struct EventRegistry {
    // Logged events key on the 8-byte discriminator alone: Anchor derives it
    // from the event name, so it is effectively unique across programs.
    by_discriminator: HashMap<[u8; 8], (String, EventDecoder)>,
    // Self-CPI events key on `(program, prefix)`: the prefix is a *shared* tag
    // (`Sha256("anchor:event")[..8]`) plus a short discriminator, so the same
    // prefix recurs across programs. Keying by the emitting program keeps a
    // transaction that composes two event-emitting programs from cross-decoding.
    cpi_by_prefix: HashMap<(Pubkey, Vec<u8>), (String, EventDecoder)>,
}

// The decoders are closures (`Arc<dyn Fn ..>`), which aren't `Debug`; a manual
// impl reports the populated counts so the registry can sit on the
// `Debug`-deriving transaction record without leaking the closure type.
impl std::fmt::Debug for EventRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventRegistry")
            .field("logged", &self.by_discriminator.len())
            .field("cpi", &self.cpi_by_prefix.len())
            .finish()
    }
}

impl EventRegistry {
    /// An empty registry: every decode misses.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a logged event: `discriminator -> (name, decoder)`. The
    /// discriminator is the event's 8-byte leading tag; `decode` takes the bytes
    /// that follow it.
    pub fn register(
        &mut self,
        discriminator: [u8; 8],
        name: impl Into<String>,
        decode: EventDecoder,
    ) -> &mut Self {
        self.by_discriminator
            .insert(discriminator, (name.into(), decode));
        self
    }

    /// Register a *self-CPI* event decoder for `program`, keyed by the leading
    /// bytes of the emitting instruction's data: a constant tag plus the event's
    /// discriminator (e.g. `EVENT_IX_TAG_LE ++ [disc]`). The program is part of
    /// the key because the tag is shared across anchor-compatible programs;
    /// `decode` receives the bytes *after* `prefix`.
    pub fn register_cpi(
        &mut self,
        program: Pubkey,
        prefix: impl Into<Vec<u8>>,
        name: impl Into<String>,
        decode: EventDecoder,
    ) -> &mut Self {
        self.cpi_by_prefix
            .insert((program, prefix.into()), (name.into(), decode));
        self
    }

    /// Decode a logged event from its raw bytes (`discriminator(8) ++ body`),
    /// or `None` when too short to carry a discriminator or carrying one we have
    /// no decoder for (a clean miss, never a panic). The base64 framing of a
    /// `Program data:` line is the renderer's concern; this takes the decoded
    /// bytes.
    pub fn decode_bytes(&self, bytes: &[u8]) -> Option<EventInfo> {
        let (disc, body) = bytes.split_first_chunk::<8>()?;
        let (name, decode) = self.by_discriminator.get(disc)?;
        let fields = decode(body)?;
        Some(EventInfo {
            name: name.clone(),
            fields,
        })
    }

    /// Decode a self-CPI event emitted by `program` from an inner instruction's
    /// raw `data`: among the decoders registered for that program, find the
    /// prefix the data begins with and decode the remainder. Only one prefix can
    /// match a given payload, so the `HashMap`'s iteration order is irrelevant.
    /// `None` on no match (the frame stays a bare CPI).
    pub fn decode_cpi(&self, program: &Pubkey, data: &[u8]) -> Option<EventInfo> {
        for ((prog, prefix), (name, decode)) in &self.cpi_by_prefix {
            if prog != program {
                continue;
            }
            let Some(body) = data.strip_prefix(prefix.as_slice()) else {
                continue;
            };
            return Some(EventInfo {
                name: name.clone(),
                fields: decode(body)?,
            });
        }
        None
    }

    /// Whether any logged-event decoder is registered. Renderers use this to
    /// skip the decode attempt when none were.
    pub fn is_empty(&self) -> bool {
        self.by_discriminator.is_empty()
    }

    /// Whether any self-CPI event decoder is registered.
    pub fn has_cpi_events(&self) -> bool {
        !self.cpi_by_prefix.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_registered_logged_event_and_cleanly_misses_everything_else() {
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
        let ev = reg.decode_bytes(&raw).expect("registered event decodes");
        assert_eq!(ev.name, "Ping");
        assert_eq!(ev.fields, vec![("nonce".to_string(), "42".to_string())]);
        assert_eq!(ev.badge(), "🔔 Ping { nonce: 42 }");

        // An unregistered discriminator, and a too-short payload, miss cleanly.
        let mut other = [9u8; 8].to_vec();
        other.extend_from_slice(&42u64.to_le_bytes());
        assert!(reg.decode_bytes(&other).is_none());
        assert!(reg.decode_bytes(&[1, 2, 3]).is_none());
    }

    #[test]
    fn decodes_a_self_cpi_event_by_program_and_prefix() {
        // tag(4) ++ disc(1) ++ a u64 field.
        let program = Pubkey::new_unique();
        let other_program = Pubkey::new_unique();
        let tag = [0xe4, 0x45, 0xa5, 0x2e];
        let mut prefix = tag.to_vec();
        prefix.push(0);
        let mut reg = EventRegistry::new();
        assert!(!reg.has_cpi_events());
        reg.register_cpi(
            program,
            prefix.clone(),
            "Created",
            Arc::new(|b: &[u8]| {
                let n = u64::from_le_bytes(b.try_into().ok()?);
                Some(vec![("id".to_string(), n.to_string())])
            }),
        );
        assert!(reg.has_cpi_events());

        let mut data = prefix.clone();
        data.extend_from_slice(&7u64.to_le_bytes());
        let ev = reg.decode_cpi(&program, &data).expect("self-CPI event decodes");
        assert_eq!(ev.badge(), "🔔 Created { id: 7 }");

        // The same payload attributed to a *different* program is a clean miss:
        // the tag is shared, but the decoder is keyed to its emitting program.
        assert!(reg.decode_cpi(&other_program, &data).is_none());

        // A different leading byte (not a registered prefix) is a clean miss.
        let mut nope = vec![0x00, 0x45, 0xa5, 0x2e, 0];
        nope.extend_from_slice(&7u64.to_le_bytes());
        assert!(reg.decode_cpi(&program, &nope).is_none());
    }
}
