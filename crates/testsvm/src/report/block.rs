use frood_guide::Block;

/// A domain value that renders itself into the frood-guide document vocabulary.
///
/// Implement it for a type you want to drop into [`Report::snapshot`](super::Report::snapshot)
/// or [`Report::authority`](super::Report::authority): the report pushes the
/// resulting [`Block`] into its assembly and emits it through frood-guide's
/// renderer. A caller with a one-off block builds it directly and passes it to
/// [`Report::block`](super::Report::block); this trait is for the types that
/// have a natural rendering (a balances view, an authority story).
///
/// Implementors resolve any `Pubkey` to an alias name before this call, so the
/// output is deterministic across runs (no base58 leaking into the report). That
/// determinism is the implementor's contract, not something this layer enforces.
pub trait ToBlock {
    fn to_block(&self) -> Block;
}
