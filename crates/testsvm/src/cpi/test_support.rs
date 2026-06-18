//! Test-only construction helper shared by the renderer test modules.
//!
//! The renderer tests work from a log stream (the shape the runtime hands a
//! backend) and need a [`CpiModel`] to render. The production path is
//! [`from_transaction`](super::model::from_transaction), which reads the neutral
//! [`Transaction`](crate::model::Transaction)'s `frames` for structure and its
//! per-frame `trace` for inner-frame accounts/data. This helper assembles that
//! `Transaction` from a flat log stream plus the inner-frame instruction `data`
//! (in DFS pre-order, for the discriminator name decode), runs the real
//! `from_transaction`, then pins the signer annotations the test specifies.
//!
//! Signers are pinned rather than derived because the old `build`-path tests
//! supplied a `SignerInfo` directly; reconstructing a `Message` whose header and
//! per-instruction account indices reproduce an arbitrary `per_root` would be
//! fiddly and orthogonal to what a renderer test is checking. The structure,
//! names, outcomes, CU, and event decode all still come from the real
//! `from_transaction`.

use {
    super::model::{from_transaction, CpiModel},
    crate::{
        frame::{frames_from_logs, Frame},
        model::{AnchorFailures, Transaction},
        trace::{InstructionTrace, TracedInstruction},
    },
    solana_pubkey::Pubkey,
};

/// The ingredients a renderer test supplies.
pub(super) struct RenderInput<'a> {
    /// The flat runtime log stream.
    pub logs: &'a [String],
    /// Per-frame instruction data in DFS pre-order (root first, then its
    /// children, recursively). Feeds the inner-frame discriminator name decode
    /// (e.g. a System `Transfer` CPI). Frames past the end of this list get
    /// empty data; an empty slice means "no inner data to decode".
    pub inner_data: &'a [Vec<u8>],
    /// The tx-required signers referenced by each top-level instruction, in
    /// `frames`/instruction order: the `root.signers` the renderers annotate.
    pub per_root: Vec<Vec<Pubkey>>,
    /// The transaction's required signers (fee payer first): `model.tx_signers`.
    pub tx_signers: Vec<Pubkey>,
}

/// Build the [`CpiModel`] for a renderer test from a log stream, going through
/// the real [`from_transaction`] path and then pinning the signer annotations.
pub(super) fn render_model(input: RenderInput<'_>) -> CpiModel {
    let frames = frames_from_logs(input.logs);

    // A trace mirroring the frame tree (DFS pre-order), carrying the per-frame
    // instruction data so inner-frame names decode. Accounts stay empty: the
    // renderer tests that assert account roles build `CpiModel` literals
    // directly (see authority.rs / ownership.rs); these log-driven tests only
    // exercise structure, names, outcomes, and events.
    let mut traced = Vec::new();
    let mut next = 0usize;
    collect_trace(&frames, 1, input.inner_data, &mut next, &mut traced);
    let trace = (!traced.is_empty()).then_some(InstructionTrace(traced));

    let tx = Transaction::assemble(
        frames,
        Default::default(),
        input.logs.to_vec(),
        None,
        0,
        None,
        trace,
        None,
        &Default::default(),
        &Default::default(),
        &AnchorFailures,
        crate::aliases::Aliases::default(),
        Default::default(),
    );

    let mut model = from_transaction(&tx);
    // Pin the signer annotations: `from_transaction` derives these from the
    // (default, empty) message, so override with the test's intent.
    model.tx_signers = input.tx_signers;
    for (root, signers) in model.roots.iter_mut().zip(input.per_root) {
        root.signers = signers;
    }
    model
}

/// Walk `frames` in DFS pre-order, emitting one [`TracedInstruction`] per frame
/// with its `data` pulled from `inner_data[next]` (or empty past the end).
fn collect_trace(
    frames: &[Frame],
    stack_height: usize,
    inner_data: &[Vec<u8>],
    next: &mut usize,
    out: &mut Vec<TracedInstruction>,
) {
    for frame in frames {
        let data = inner_data.get(*next).cloned().unwrap_or_default();
        *next += 1;
        out.push(TracedInstruction {
            program_id: frame.program_id,
            stack_height,
            accounts: Vec::new(),
            data,
        });
        collect_trace(&frame.children, stack_height + 1, inner_data, next, out);
    }
}
