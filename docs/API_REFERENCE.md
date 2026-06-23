# API reference

The API reference is **generated from the source**, not maintained here. Every
public type, trait, method, and macro carries its own `///` doc comment (often
with an example), and `rustdoc` renders those into a browsable, cross-linked,
searchable site. A hand-written copy in markdown can only drift: `cargo test`
can check a `///` example, but it cannot check this file.

## Read it

Build it locally against your checkout:

```bash
cargo doc --no-deps --open
```

There's no docs.rs page for this work: the crates aren't published (see the
book's Installation chapter for why), so the reference is something you generate
locally rather than browse. The `docs.rs/anchor-litesvm` and
`docs.rs/litesvm-utils` pages that do exist belong to the upstream published
crates this forked from, not this codebase.

## Where the rest lives

`rustdoc` answers "what does this item do, what does it take, what traits does
it implement". The other two questions have other homes:

- **"How do I get started / how do the pieces fit in a real test?"** is
  narrative: see the book (`book/`, an mdBook) and the runnable `examples/`
  (`cargo run -p anchor-litesvm --example account_graphs`, etc.).
- **"Why is it built this way?"** is architecture: see `docs/design/`
  (`cpi-rendering.md` for the renderer family, `bundled-pubkeys.md` for the
  derive). N.B. the internals there are `pub(super)`, so they deliberately do
  not appear in `rustdoc`; the design docs are their home.
