# Architecture Notes

This page is deliberately *thin*: it routes, it does not duplicate. The
architecture lives in the committed design docs, and the API reference is
generated, not written. This is the signpost, so a reader who reaches the end of
the book knows where the deep internals are.

## Where the internals are documented

- **Renderer family** (the `CpiModel`, the `Renderer` port, the tree / mermaid /
  authority / ownership adapters): [`docs/design/cpi-rendering.md`](https://github.com/cds-rs/anchor-litesvm/blob/turbin3/docs/design/cpi-rendering.md).
- **The derive** (`BundledPubkeys` and friends): [`docs/design/bundled-pubkeys.md`](https://github.com/cds-rs/anchor-litesvm/blob/turbin3/docs/design/bundled-pubkeys.md).
- **The litesvm boundary** (what the executor owns versus what the testing
  layers reconstruct, and the execution-observer direction): [`docs/design/litesvm-boundary.md`](https://github.com/cds-rs/anchor-litesvm/blob/turbin3/docs/design/litesvm-boundary.md).
- **Every public type, trait, and method**: the generated rustdoc. Run
  `cargo doc --no-deps --open` against your checkout (the crate isn't published,
  so there's no docs.rs page yet).

## Why so thin

The internals the design docs describe are `pub(super)`: deliberately private,
so they don't appear in rustdoc, which is why the design docs are their home.
Duplicating them here would create a second source of truth the toolchain can't
check. The principle (one home per question, each guarded against drift) is laid
out in [`docs/README.md`](https://github.com/cds-rs/anchor-litesvm/blob/turbin3/docs/README.md).
