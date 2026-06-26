//! The labeled-flowchart emitter shared by the authority and ownership graphs.
//!
//! Both graphs are the same shape: a Mermaid `flowchart LR` of typed nodes (a
//! node's role picks its bracket shape and CSS class) and verb-labeled edges
//! (the relationship rides the arrow). They differ only in which accounts
//! become which nodes and edges; the rendering is identical, so it lives here
//! once. Each renderer walks the model, builds the node map and edge groups,
//! then calls [`render_flowchart`].

use {
    super::renderer::{node_label, NodeIds},
    indexmap::{IndexMap, IndexSet},
    std::fmt::Write,
};

/// A node's bracket shape, which reads as its role: a `Stadium` is an
/// authority, a `Rect` a program or owner, a `Cylinder` a piece of account
/// state.
#[derive(Copy, Clone)]
pub(super) enum Shape {
    /// `([name])`
    Stadium,
    /// `[name]`
    Rect,
    /// `[(name)]`
    Cylinder,
}

/// How to draw a node: its shape, its CSS class (matched to a `classDef`), and
/// its precedence `rank`. When a pubkey appears in more than one role,
/// [`upsert`] keeps the highest rank (a program that is also written stays a
/// program).
#[derive(Copy, Clone)]
pub(super) struct NodeStyle {
    pub shape: Shape,
    pub class: &'static str,
    pub rank: u8,
}

/// Insert `name` at `style`, upgrading only when its rank beats what's there.
/// An `IndexMap` re-insert updates in place, so the node keeps its first-seen
/// position, which is what fixes the emit order.
pub(super) fn upsert(nodes: &mut IndexMap<String, NodeStyle>, name: String, style: NodeStyle) {
    match nodes.get(&name) {
        Some(existing) if existing.rank >= style.rank => {}
        _ => {
            nodes.insert(name, style);
        }
    }
}

/// Emit a `flowchart LR` from typed nodes and verb-labeled edge groups.
///
/// `classdefs` are `(class, "fill:..,stroke:..")` pairs declared up front;
/// `nodes` render in insertion order with their shape and class; each
/// `(verb, edges)` group renders its arrows in insertion order, and the groups
/// render in the order given (so the caller controls, e.g., "signs" arrows
/// before "writes"). Edge endpoints reuse the ids assigned during the node
/// pass.
pub(super) fn render_flowchart(
    classdefs: &[(&str, &str)],
    nodes: &IndexMap<String, NodeStyle>,
    edge_groups: &[(&str, &IndexSet<(String, String)>)],
) -> String {
    let mut out = String::new();
    out.push_str("```mermaid\n");
    out.push_str("flowchart LR\n");
    for (class, style) in classdefs {
        let _ = writeln!(out, "    classDef {class} {style};");
    }
    let mut ids = NodeIds::new();
    for (name, node) in nodes {
        let id = ids.id(name);
        let lbl = node_label(name);
        let _ = match node.shape {
            Shape::Stadium => writeln!(out, "    {id}([{lbl}]):::{}", node.class),
            Shape::Rect => writeln!(out, "    {id}[{lbl}]:::{}", node.class),
            Shape::Cylinder => writeln!(out, "    {id}[({lbl})]:::{}", node.class),
        };
    }
    for (verb, edges) in edge_groups {
        for (from, to) in *edges {
            let (fid, tid) = (ids.id(from), ids.id(to));
            let _ = writeln!(out, "    {fid} -->|{verb}| {tid}");
        }
    }
    out.push_str("```\n");
    out
}
