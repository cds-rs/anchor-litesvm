# Quick Start

The quick start now lives in **the book**, so there's one home for it instead of two. Part I (Getting Started) is the former contents of this file, split into four short chapters; Parts II and III carry the rest (building instructions, executing, asserting).

## Read it

Build the book locally and open it:

```bash
cargo install mdbook mdbook-mermaid   # once
mdbook serve book --open
```

Or read the source directly on GitHub, starting from [`book/src/SUMMARY.md`](../book/src/SUMMARY.md). The Getting Started chapters are:

1. [Why anchor-litesvm](../book/src/intro/why.md)
2. [Installation & Setup](../book/src/intro/installation.md)
3. [Your First Test](../book/src/intro/first-test.md)
4. [The Five-Step Pattern](../book/src/intro/five-step-pattern.md)

## Why this is a redirect

The narrative guide moved into the book so it could grow worked examples (Vault, Escrow, CPAMM) and the rendering chapters without this single file ballooning. Keeping a copy here would just be a second source of truth that drifts. For how the docs are layered (and why each layer has its own guard against rot), see [README.md](README.md).
