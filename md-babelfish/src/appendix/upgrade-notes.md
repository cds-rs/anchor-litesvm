# Upgrade Notes (turbin3)

For suites pinned to `branch = "turbin3"`. Each section describes a branch
forward; the newest is first. `cargo update -p anchor-litesvm` picks it up.

## June 2026: the testsvm vocabulary

If your tests use the surface this book teaches (`ctx.tx().build().send_ok()`,
cast verbs, bundles, `Report`), there is nothing to change. Everything below
is additive except two type renames at the backend layer.

### Renames

| was | is |
|---|---|
| `ExecutionBackend` | `TestSVM` (trait, `testsvm` crate; re-exported from `litesvm_utils`) |
| `ExecutionRecord` | `model::Transaction` |

The old names are gone, not aliased. They only ever mattered if you named the
backend trait directly (for example, to hold a `RpcBackend` behind a generic);
update the two identifiers and the same code compiles.

### New since the last forward

- **`TestSVM`**: one trait describing a test runtime; `LiteSvmBackend` is the
  default engine, `RpcBackend` (feature `rpc`) targets a surfnet endpoint, and
  the `testsvm-mollusk` crate runs the same vocabulary on mollusk-svm for
  Pinocchio programs. Engines never share a dependency graph; pick one per
  build.
- **Bundles inject programs by structure**: any `Program<'info, T>` field in
  your accounts struct injects `<T as Id>::id()` in the generated conversion.
  The fixed table of well-known programs is gone; your own `Id` types work the
  same as `System`. `#[bundle(inject = expr)]` covers untyped fields,
  `#[bundle(default = expr)]` pins a bundle field's placeholder, and
  `YourAccounts::injected_programs()` feeds `ctx.alias_programs(..)` so CPI
  trees name them.
- **`Option<Pubkey>` bundle fields** derive like any other field.
- **Pinocchio support**: `litesvm-pinocchio-derive` (additive `testing`-feature
  derives for instruction/error tables) and `litesvm-pinocchio-idl` (IDL
  extraction from an annotated enum). Anchor suites are unaffected.

Workspace note: `testsvm-mollusk` is deliberately outside the workspace (its
mollusk graph and the litesvm graph cannot share a lockfile). A git dependency
on `anchor-litesvm` never builds it.
