//! Execution observation: a configurable registry of observers that an adapter
//! ([`ObservedSvm`]) runs on every transaction, producing a typed metadata bag
//! the caller reads off the result.
//!
//! This is the **local adapter** that prototypes the vocabulary before it is
//! promoted to the litesvm fork; see `docs/design/litesvm-boundary.md`. The
//! principle:
//! *write to one interface (`send`); get a response whose contents are decided
//! by how the svm was configured, not by the call site.* You register observers
//! once (session-scoped); every send runs them; the result carries
//! `metadata.get::<O::Output>()`.
//!
//! The types here are deliberately shaped **as litesvm types**, raw pubkeys and
//! frames, no `Aliases`, no anchor coupling, so they relocate into the fork
//! unchanged. Naming and rendering stay in the consumer.

use {
    crate::transaction::{
        InstructionTrace, TraceHandle, TraceRecorder, TransactionHelpers, TransactionResult,
    },
    litesvm::{
        cpi_tree::{cpi_tree, CpiFrame},
        LiteSVM,
    },
    solana_keypair::Keypair,
    solana_message::Message,
    solana_program::instruction::Instruction,
    std::{
        any::{Any, TypeId},
        collections::HashMap,
        ops::{Deref, DerefMut},
    },
};

/// The executor's record of one transaction: the input every observer reads.
/// Carries both the in-flight capture (the [`InstructionTrace`], the per-frame
/// signer / writable / owner facts the message header cannot see, e.g. an
/// `invoke_signed` PDA one frame down) and the finished result.
pub struct ExecutionView<'a> {
    pub trace: Option<&'a InstructionTrace>,
    pub logs: &'a [String],
    pub message: &'a Message,
    pub error: Option<&'a str>,
    pub compute_units: u64,
    pub fee: u64,
}

/// A fact the executor can hydrate about a transaction. The `Output` type is the
/// vocabulary entry: the consumer reads it back by that type. Implementors must
/// resolve nothing that needs naming, raw pubkeys only, so the output can live
/// at litesvm.
pub trait ExecutionObserver: Send + Sync + 'static {
    type Output: Any + Send + Sync;
    fn observe(&self, view: &ExecutionView) -> Self::Output;
}

/// A typed, heterogeneous bag keyed by output type (the `http::Extensions`
/// pattern). Each registered observer contributes one entry.
#[derive(Default)]
pub struct ExecutionMetadata {
    map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl ExecutionMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn insert<T: Any + Send + Sync>(&mut self, value: T) {
        self.map.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Read an observer's output by its type. `None` if that observer was not
    /// registered on the svm.
    pub fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>())
    }
}

/// The configured set of observers, built once (session-scoped) on the svm and
/// run on every send.
#[derive(Default)]
pub struct ObserverRegistry {
    #[allow(clippy::type_complexity)]
    runners: Vec<Box<dyn Fn(&ExecutionView, &mut ExecutionMetadata) + Send + Sync>>,
}

impl ObserverRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an observer; its output lands in the bag under its `Output` type.
    pub fn observe<O: ExecutionObserver>(&mut self, observer: O) -> &mut Self {
        self.runners.push(Box::new(move |view, bag| {
            bag.insert(observer.observe(view));
        }));
        self
    }

    pub(crate) fn run(&self, view: &ExecutionView) -> ExecutionMetadata {
        let mut bag = ExecutionMetadata::new();
        for run in &self.runners {
            run(view, &mut bag);
        }
        bag
    }
}

/// The adapter: a `LiteSVM` wrapped with an observer registry. The ctx is built
/// through it, so `ctx.svm` *is* this. It `Deref`s to the inner svm for
/// non-executing reads; the eventual send shadows (next step) call the inner
/// send and then [`observe_result`](Self::observe_result), so the registry runs
/// on every send regardless of the call site.
pub struct ObservedSvm {
    inner: LiteSVM,
    trace: TraceHandle,
    registry: ObserverRegistry,
}

impl ObservedSvm {
    /// Wrap a `LiteSVM` and install the in-flight trace recorder. Register
    /// observers with [`observe`](Self::observe).
    pub fn new(mut svm: LiteSVM) -> Self {
        let trace = TraceRecorder::install(&mut svm);
        Self {
            inner: svm,
            trace,
            registry: ObserverRegistry::new(),
        }
    }

    /// Register an observer (builder style).
    pub fn observe<O: ExecutionObserver>(mut self, observer: O) -> Self {
        self.registry.observe(observer);
        self
    }

    /// Drain the in-flight trace, build the view over a finished result, and run
    /// the registry, producing the result's metadata bag. The send shadows call
    /// this; exposed so a consumer can also observe a result it sent another way
    /// during the migration.
    pub fn observe_result(&self, result: &TransactionResult) -> ExecutionMetadata {
        let trace = self.trace.take_latest();
        let view = ExecutionView {
            trace: trace.as_ref(),
            logs: result.logs(),
            message: &result.message,
            error: result.error().map(String::as_str),
            compute_units: result.compute_units(),
            fee: result.fee(),
        };
        self.registry.run(&view)
    }

    /// Send a full instruction list as one transaction, run the observers, and
    /// return the result wrapped with its metadata. This is the single observed
    /// send the doors will route through; `Deref` makes the wrapper behave as the
    /// `TransactionResult` it carries.
    pub fn send_instructions(
        &mut self,
        instructions: &[Instruction],
        signers: &[&Keypair],
    ) -> Observed<TransactionResult> {
        let result = self
            .inner
            .send_instructions(instructions, signers)
            .expect("send_instructions: build a valid transaction");
        let metadata = self.observe_result(&result);
        Observed::new(result, metadata)
    }

    /// Single-instruction convenience over
    /// [`send_instructions`](Self::send_instructions).
    pub fn send_instruction(
        &mut self,
        instruction: Instruction,
        signers: &[&Keypair],
    ) -> Observed<TransactionResult> {
        self.send_instructions(&[instruction], signers)
    }
}

impl Deref for ObservedSvm {
    type Target = LiteSVM;
    fn deref(&self) -> &LiteSVM {
        &self.inner
    }
}

impl DerefMut for ObservedSvm {
    fn deref_mut(&mut self) -> &mut LiteSVM {
        &mut self.inner
    }
}

/// A result wrapped with the metadata the observers produced for its send.
/// `Deref`s to the inner value, so it behaves exactly as the result it carries
/// (`observed.logs()`, `observed.assert_success()`, ...); the metadata rides
/// alongside, read via [`metadata`](Self::metadata). The wrapper stays invisible
/// to anyone using the result's own API, which keeps it out of the cohort's
/// mental model of "a transaction result."
pub struct Observed<T> {
    inner: T,
    metadata: ExecutionMetadata,
}

impl<T> Observed<T> {
    pub(crate) fn new(inner: T, metadata: ExecutionMetadata) -> Self {
        Self { inner, metadata }
    }

    /// The metadata the registered observers produced for this send.
    pub fn metadata(&self) -> &ExecutionMetadata {
        &self.metadata
    }

    /// Take the inner result by value, for the rare site that moves it (`Deref`
    /// only lends a borrow). A by-value `TransactionResult` position can also
    /// accept this via `.into()`.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> Deref for Observed<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner
    }
}

/// Lets a by-value `TransactionResult` site accept an `Observed` via `.into()`.
/// Concrete, not blanket: a blanket `impl<T> From<Observed<T>> for T` collides
/// with the reflexive `From<T> for T`, and we only ever unwrap to this type.
impl From<Observed<TransactionResult>> for TransactionResult {
    fn from(observed: Observed<TransactionResult>) -> Self {
        observed.inner
    }
}

// --- the core observers (the vocabulary's first entries) ---------------------

/// In-flight observer: the per-frame signer / writable / owner facts the runtime
/// presented, the "who was authorized to touch what" of the transaction,
/// including `invoke_signed` PDAs the message header cannot see. Output is the
/// raw [`InstructionTrace`]; the authority diagram and account index project it.
pub struct SignerAuthority;

impl ExecutionObserver for SignerAuthority {
    type Output = InstructionTrace;
    fn observe(&self, view: &ExecutionView) -> InstructionTrace {
        view.trace.cloned().unwrap_or_default()
    }
}

/// Post observer: the structural CPI invocation tree, parsed from the logs (the
/// transform Amal already pushed into litesvm). Output is the raw frame forest;
/// the tree / mermaid renderers project it.
pub struct CpiTree;

/// The CPI invocation forest, one entry per top-level instruction. A newtype so
/// it is its own key in the metadata bag.
pub struct CpiForest(pub Vec<CpiFrame>);

impl ExecutionObserver for CpiTree {
    type Output = CpiForest;
    fn observe(&self, view: &ExecutionView) -> CpiForest {
        CpiForest(cpi_tree(view.logs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bag_reads_back_by_output_type() {
        let mut md = ExecutionMetadata::new();
        md.insert(42u32);
        md.insert(String::from("memo"));
        assert_eq!(md.get::<u32>(), Some(&42));
        assert_eq!(md.get::<String>().map(String::as_str), Some("memo"));
        assert_eq!(md.get::<u64>(), None, "an unregistered type reads None");
    }

    struct Probe;
    impl ExecutionObserver for Probe {
        type Output = usize;
        fn observe(&self, view: &ExecutionView) -> usize {
            view.logs.len()
        }
    }

    #[test]
    fn registry_runs_each_observer_into_the_bag() {
        let mut reg = ObserverRegistry::new();
        reg.observe(Probe);
        let msg = Message::default();
        let logs = vec!["a".to_string(), "b".to_string()];
        let view = ExecutionView {
            trace: None,
            logs: &logs,
            message: &msg,
            error: None,
            compute_units: 0,
            fee: 0,
        };
        let md = reg.run(&view);
        assert_eq!(md.get::<usize>(), Some(&2));
    }

    #[test]
    fn observed_derefs_to_inner_and_carries_metadata() {
        let mut md = ExecutionMetadata::new();
        md.insert(7u8);
        let observed = Observed::new(String::from("hello"), md);
        // Deref: the inner String's API is transparently available.
        assert_eq!(observed.len(), 5);
        // The metadata rides alongside.
        assert_eq!(observed.metadata().get::<u8>(), Some(&7));
        // And it unwraps by value.
        let inner: String = observed.into_inner();
        assert_eq!(inner, "hello");
    }
}
