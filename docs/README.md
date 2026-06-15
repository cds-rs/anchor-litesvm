# Documentation

This project documents itself in layers. Each layer answers a different
question, lives in a different place, and has its own guard against rot. The
rule of thumb: if the reader is asking "how do I use this?", it's narrative; if
"what does this function do?", it's generated from the source.

| Question | Home | Guard |
|---|---|---|
| **What does this item do?** (types, traits, methods, macros) | rustdoc, generated from the `///` doc comments. Run `cargo doc --no-deps --open` against your checkout (the crate isn't published, so there's no docs.rs page for this code; the existing docs.rs/anchor-litesvm is the upstream this forked from). | `cargo doc` runs under `deny(rustdoc::broken_intra_doc_links)`, so a broken link fails the build; the core examples are `no_run` doctests, so a renamed method or changed signature breaks `cargo test --doc`. |
| **How do I use it / get started?** | **The book** ([`book/src/`](../book/src/SUMMARY.md); build with `mdbook serve book --open`) and the runnable [`examples/`](../examples/). | The examples compile and run (`cargo run -p anchor-litesvm --example account_graphs`); the book's rendering chapters embed real captured output. |
| **Why is it built this way?** (architecture, internals, tradeoffs) | [`design/`](design/): [cpi-rendering.md](design/cpi-rendering.md) for the renderer family, [bundled-pubkeys.md](design/bundled-pubkeys.md) for the derive, [litesvm-boundary.md](design/litesvm-boundary.md) for the litesvm/anchor-litesvm split. | Committed markdown, kept honest by review. The internals they describe are `pub(super)`, so they do not appear in rustdoc; the design docs are their home. |
| **How do I move an existing raw-LiteSVM suite over?** | **The book**: [Migrating from Raw LiteSVM](../book/src/appendix/migration.md). | Hand-written, kept honest by review. |

## Why it's shaped this way

The API reference is **generated, not maintained**. Every public item carries a
`///` doc comment; rustdoc renders those into a cross-linked, searchable site
that cannot drift, because it *is* the code. A hand-written `API_REFERENCE.md`
used to sit alongside it as a second source of truth that the toolchain could
not check, so it silently fell behind (it had lost the graph renderers
entirely). It is now a thin pointer to `cargo doc`.

rustdoc only covers the public surface, though, and it cannot explain *why*. So
the "why" lives in the design docs (and the internals they describe are
deliberately private, which is also why they are not in rustdoc). The "how do I
start" lives in the book (`book/`, an mdBook) and the examples. A standalone
`QUICK_START.md` used to hold the narrative; it moved into the book (Parts I
through III) so the guide could grow worked examples and rendering chapters
without one file ballooning, and `QUICK_START.md` is now a thin redirect, the
same move as `API_REFERENCE.md`. Each question has exactly one home, and each
home has a guard that fails loudly when it drifts: a broken link, a doctest that
no longer compiles, an example that no longer runs, an `mdbook build` that
fails.

## Also in this directory

`RELEASE_NOTES.md` and `proposals/` (forward-looking notes, not yet built).
Compat (`compat/anchor-0.31`) compatibility lives in the book's
[Anchor Version Compatibility](../book/src/appendix/anchor-compat.md) appendix; it is a
Metaplex bridge, not a parity target, so there is no parity ledger.

`QUICK_START.md`, `MIGRATION.md`, and `CONVENTIONS.md` are now thin redirects:
their narrative moved into the book (Part I, the migration appendix, and the
conventions appendix respectively), so the file here just points at its new
home.
