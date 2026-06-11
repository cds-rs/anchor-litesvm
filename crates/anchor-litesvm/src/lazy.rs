//! Late-bound bundle fields: a value resolved against live SVM state at build
//! time, kept `Copy` by deferring to a dispatch enum rather than a closure.
//!
//! A bundle field whose pubkey depends on on-chain state (a counter-seeded PDA,
//! say) cannot be filled when the bundle is declared. Embedding a closure would
//! make the bundle non-`Copy` and break the "declare the cast once, reuse it"
//! ergonomic. Instead a [`Lazy`] field holds either a ready pubkey or a `Copy`
//! *strategy* that implements [`Resolve`]. The strategy and its `match` are
//! declared once, up front; each use is then a plain declaration, and
//! [`Tx::build`](crate::tx::Tx::build) resolves them against the SVM (via the
//! generated [`Resolvable`] impl) before projecting the bundle onto account
//! metas.
//!
//! ```ignore
//! #[derive(Copy, Clone)]
//! enum MySeed { CounterPda(Pubkey) }            // carries Copy inputs only
//! impl Resolve for MySeed {
//!     fn resolve(self, ctx: &AnchorContext) -> Pubkey {
//!         match self {
//!             MySeed::CounterPda(parent) => {
//!                 let p: ParentState = ctx.get_account(&parent).unwrap();
//!                 derive_child(parent, p.next_index)
//!             }
//!         }
//!     }
//! }
//!
//! #[derive(Copy, Clone, Bundle)]
//! struct MyBundle { owner: Pubkey, child: Lazy<MySeed> }
//! // child: Lazy::Deferred(MySeed::CounterPda(parent)) resolves at build().
//! ```

use crate::context::AnchorContext;
use solana_program::pubkey::Pubkey;

/// A bundle field that is either a ready pubkey or a deferred [`Resolve`]
/// strategy. `Copy` whenever `D` is (the whole point: a strategy enum of
/// `Copy` inputs, not a closure).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Lazy<D> {
    Ready(Pubkey),
    Deferred(D),
}

/// A deferred derivation strategy: typically a `Copy` enum plus a `match` that
/// fetches account state and derives a pubkey. Declared once per program.
///
/// Returns `None` when the strategy can't resolve yet (a dependency account
/// doesn't exist), so the field stays `Deferred` for a later build rather than
/// failing. That is what lets a bundle declare, up front, a field whose
/// dependency a *later* instruction in the same flow creates: `None` on the
/// builds that don't need it, resolved on the build that does. If an
/// instruction actually projects a still-`Deferred` field, projection panics
/// with a clear message.
pub trait Resolve {
    fn resolve(self, ctx: &AnchorContext) -> Option<Pubkey>;
}

/// Resolve a single bundle field against the SVM, in place. Implemented for
/// `Pubkey` (a no-op) and `Lazy<D>` (runs the strategy), so the generated
/// [`Resolvable::resolve_all`] can call it uniformly on every field without
/// inspecting types.
pub trait ResolveField {
    fn resolve_field(&mut self, ctx: &AnchorContext);
}

impl ResolveField for Pubkey {
    fn resolve_field(&mut self, _ctx: &AnchorContext) {}
}

impl<D: Resolve + Copy> ResolveField for Lazy<D> {
    fn resolve_field(&mut self, ctx: &AnchorContext) {
        if let Lazy::Deferred(d) = *self {
            // Stay `Deferred` if the strategy can't resolve yet; a later build
            // (after the dependency exists) resolves it, and projection panics
            // only if an instruction actually needs a still-unresolved field.
            if let Some(pk) = d.resolve(ctx) {
                *self = Lazy::Ready(pk);
            }
        }
    }
}

/// A bundle that can resolve all its [`Lazy`] fields against the SVM. Generated
/// by `#[derive(Bundle)]`; `build()` calls it before projecting.
pub trait Resolvable {
    fn resolve_all(&mut self, ctx: &AnchorContext);
}

/// The placeholder value `#[derive(Bundle)]`'s `Default` uses per field: a fresh
/// `Pubkey::new_unique()` (std `Pubkey::default()` is all-zeros and gets
/// rejected by virtually every program). Implemented for `Pubkey` and
/// `Lazy<D>` so a bundle with `Lazy` fields still derives `Default`.
pub trait BundleDefault {
    fn bundle_default() -> Self;
}

impl BundleDefault for Pubkey {
    fn bundle_default() -> Self {
        Pubkey::new_unique()
    }
}

impl<D> BundleDefault for Lazy<D> {
    fn bundle_default() -> Self {
        Lazy::Ready(Pubkey::new_unique())
    }
}

/// `Option<T>` is a first-class bundle field shape (the `unwrap`/`wrap_some`
/// projections exist precisely for it), so the `Bundle` derive must handle
/// it: resolution delegates to the inner value when present, and the
/// placeholder default is `None` (the "not set yet" the projections check).
impl<T: ResolveField> ResolveField for Option<T> {
    fn resolve_field(&mut self, ctx: &AnchorContext) {
        if let Some(inner) = self {
            inner.resolve_field(ctx);
        }
    }
}

impl<T> BundleDefault for Option<T> {
    fn bundle_default() -> Self {
        None
    }
}

impl<D> From<Pubkey> for Lazy<D> {
    fn from(pk: Pubkey) -> Self {
        Lazy::Ready(pk)
    }
}

impl<D> From<Lazy<D>> for Pubkey {
    fn from(l: Lazy<D>) -> Self {
        match l {
            Lazy::Ready(pk) => pk,
            Lazy::Deferred(_) => panic!(
                "unresolved Lazy bundle field projected to a Pubkey; build() resolves \
                 Lazy fields before projection, so a Deferred value here means a bundle \
                 reached account-meta construction without going through build()"
            ),
        }
    }
}
