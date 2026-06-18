//! The renderer port: the contract every CPI-model presentation
//! implements, plus the alias-display service they share.
//!
//! A [`Renderer`] takes a fully-resolved [`CpiModel`](super::model::CpiModel)
//! and produces a complete string in its own format, owning its own
//! header/footer/framing. The tree and mermaid adapters live in their own
//! modules; future specializations (authority / ownership graphs) are new
//! `Renderer` impls over the same model.
//!
//! [`LegendCollector`] is the one piece of presentation state shared
//! across adapters: it resolves a `Pubkey` to a friendly name *and*
//! records, in first-seen order, each `(name, Pubkey)` pair actually used,
//! so an adapter can print a legend reflecting only the aliases that
//! appeared in this render. It lives here (not in an adapter) because both
//! adapters resolve pubkeys through it, each in its own traversal order.

use {
    super::aliases::Aliases,
    super::events::{EventInfo, EventRegistry},
    super::model::CpiModel,
    indexmap::IndexMap,
    solana_pubkey::Pubkey,
    std::collections::HashSet,
};

/// The port. Each presentation of a [`CpiModel`] implements this; the
/// caller picks an impl and prints its output verbatim.
pub(super) trait Renderer {
    fn render(&self, model: &CpiModel, aliases: &Aliases) -> String;
}

/// Wraps an [`Aliases`] for the duration of a render pass and records, in
/// insertion order, each `(name, Pubkey)` pair that was actually resolved.
///
/// Adapters call into the collector instead of [`Aliases`] directly, so a
/// legend reflects only aliases that appeared in this particular render
/// (not every alias the user supplied). One entry per name; the first-seen
/// `Pubkey` wins (later occurrences are silently deduplicated).
///
/// `seen` is an [`IndexMap`] (insertion-order iteration) rather than a
/// `HashMap` so the legend prints in the order names first appeared, and so
/// snapshot tests don't flake on a `HashMap`'s randomized iteration. Dedup
/// goes through [`IndexMap::entry`]`.or_insert(..)`: first-seen wins, O(1).
pub(super) struct LegendCollector<'a> {
    aliases: &'a Aliases,
    events: &'a EventRegistry,
    seen: IndexMap<&'a str, Pubkey>,
}

impl<'a> LegendCollector<'a> {
    pub(super) fn new(aliases: &'a Aliases, events: &'a EventRegistry) -> Self {
        Self {
            aliases,
            events,
            seen: IndexMap::new(),
        }
    }

    /// Decode a `Program data:` base64 payload into a named, field-formatted
    /// event, with `Pubkey`s in the fields substituted to their aliases. `None`
    /// when no decoder is registered for the payload's discriminator (the
    /// renderer then keeps the raw form). The fields arrive from the decoder
    /// with base58 keys; this is the one render-time place they're aliased,
    /// since a decoded event is free text, not the typed `Pubkey`s the rest of
    /// the model carries.
    pub(super) fn decode_event(&self, payload: &str) -> Option<EventInfo> {
        Some(self.events.decode_logged(payload)?.resolved(self.aliases))
    }

    /// Decode a *self-CPI* event from an inner frame's raw instruction `data`
    /// (an `emit_cpi!`-style event leaves no `Program data:` log; its payload is
    /// the inner instruction's data, which the trace carries onto the frame).
    /// Fields are alias-substituted exactly as in [`decode_event`](Self::decode_event).
    pub(super) fn decode_cpi_event(&self, program: &Pubkey, data: &[u8]) -> Option<EventInfo> {
        Some(
            self.events
                .decode_cpi(program, data)?
                .resolved(self.aliases),
        )
    }

    /// The recorded `(name, Pubkey)` pairs in insertion order.
    pub(super) fn into_entries(self) -> IndexMap<&'a str, Pubkey> {
        self.seen
    }

    /// First-seen wins: later occurrences of `name` keep the original
    /// `Pubkey` and don't shift its position in the legend.
    fn record(&mut self, name: &'a str, pk: Pubkey) {
        self.seen.entry(name).or_insert(pk);
    }

    /// Resolve a `Pubkey` to a name (recording the alias for the legend)
    /// or fall back to `<first 8>…<last 4>` truncation. Called for every
    /// program ID and signer pubkey rendered.
    pub(super) fn render_pubkey(&mut self, pubkey: &Pubkey) -> String {
        if let Some(name) = self.aliases.resolve_by_pubkey(pubkey) {
            self.record(name, *pubkey);
            return name.to_string();
        }
        super::aliases::short_pubkey(pubkey)
    }
}

/// Hands out stable, unique, readable Mermaid node ids for the flowchart
/// graphs (authority / ownership), and a matching label helper.
///
/// A node id must be a bare identifier (ASCII alphanumerics + `_`); a label
/// can be anything. The naive approach (id = sanitized name) collides when two
/// distinct names sanitize to the same thing (`a.b` and `a-b` both `a_b`),
/// which silently merges nodes in the diagram. So [`NodeIds`] keeps a registry:
/// the first claimant of a sanitized id keeps it; later collisions get a
/// `_2`, `_3`, ... suffix. The readable name still rides in the label.
///
/// This is the flowchart counterpart to the sequence renderer's
/// `participant Id as "name"` trick. (That renderer predates this and keeps its
/// own copy; consolidating it would churn its snapshot tests for no behavior
/// change.)
pub(super) struct NodeIds {
    by_name: IndexMap<String, String>,
    used: HashSet<String>,
}

impl NodeIds {
    pub(super) fn new() -> Self {
        Self {
            by_name: IndexMap::new(),
            used: HashSet::new(),
        }
    }

    /// The id for `name`, assigning a fresh one on first sight. Idempotent:
    /// the same name always returns the same id.
    pub(super) fn id(&mut self, name: &str) -> String {
        if let Some(id) = self.by_name.get(name) {
            return id.clone();
        }
        let base = sanitize_id(name);
        let mut id = base.clone();
        let mut n = 2;
        while self.used.contains(&id) {
            id = format!("{base}_{n}");
            n += 1;
        }
        self.used.insert(id.clone());
        self.by_name.insert(name.to_string(), id.clone());
        id
    }
}

/// The text to place inside a node's shape brackets: the bare name when it is
/// already a safe identifier (so common aliased names read plainly), else the
/// name in quotes (so `…`-truncated pubkeys and punctuated names don't break
/// the parser). Any embedded `"` becomes the Mermaid `#quot;` entity.
pub(super) fn node_label(name: &str) -> String {
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('"', "#quot;"))
    }
}

/// Sanitize a name into a Mermaid identifier base: ASCII alphanumerics and `_`
/// pass through, everything else (dots, hyphens, the `…` from pubkey
/// truncation, unicode) becomes `_`.
fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
