# Test + README Conventions

The test-output conventions now live in **the book**, so the narrative has one home instead of two.

## Read it

- In the book: [Test-Output Conventions](../book/src/appendix/conventions.md) (build with `mdbook serve book --open`).

It covers the same house standard: the four pillars (fast, deterministic, low-cost, enabling), the test-output contract (seeded identities, one `Report` per test, the assembled canonical `docs/testing/test-report.md`), the README contract, and the first-principles reminder that the test is the source of truth and the trace is derived.

## Why this is a redirect

The conventions are narrative house-style, so they moved into the book with the rest of the prose; keeping a copy here would just be a second source of truth that drifts. For how the docs are layered, see [README.md](README.md).
