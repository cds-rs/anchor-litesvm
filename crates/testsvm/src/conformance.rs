//! Conformance scenarios: the shared artifact of cross-engine testing.
//! Generic over [`TestSVM`]; owned by neither engine nor program framework.
//! Each adapter crate runs these in its OWN tests, in its own dependency
//! graph: same test, different backend, rebuild. Engines never meet in one
//! graph.

use {crate::TestSVM, solana_pubkey::Pubkey, solana_signer::Signer};

pub use crate::frame::Frame;
pub use crate::trace::InstructionTrace;

/// A single way the structured observability record fails the spec. Typed so a
/// caller can match a specific invariant rather than scan error strings.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ObservabilityViolation {
    /// `per_frame_trace` is declared but the trace is absent or empty.
    TraceDeclaredButAbsent,
    /// The top-level frame's `stack_height` is not 1.
    TopLevelStackHeight { found: usize },
    /// A frame's `stack_height` is below 1.
    StackHeightBelowOne { frame: usize, height: usize },
    /// A frame's `stack_height` jumped more than one level descending.
    StackHeightJumped { frame: usize, from: usize, to: usize },
    /// `structured_cpi`: the frame count disagrees with the trace length.
    FrameCountMismatch { frames: usize, trace: usize },
    /// `structured_cpi`: frame program ids disagree with the trace, pre-order.
    FrameProgramIdsMismatch,
}

impl std::fmt::Display for ObservabilityViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TraceDeclaredButAbsent => {
                write!(f, "per_frame_trace declared, but the trace is absent or empty")
            }
            Self::TopLevelStackHeight { found } => {
                write!(f, "top-level stack_height must be 1, was {found}")
            }
            Self::StackHeightBelowOne { frame, height } => {
                write!(f, "frame {frame} stack_height {height} is below 1")
            }
            Self::StackHeightJumped { frame, from, to } => write!(
                f,
                "frame {frame} stack_height {to} jumped from {from} (CPIs descend one level at a time)"
            ),
            Self::FrameCountMismatch { frames, trace } => {
                write!(f, "structured_cpi: frame count {frames} disagrees with trace length {trace}")
            }
            Self::FrameProgramIdsMismatch => {
                write!(f, "structured_cpi: frame program ids disagree with the trace, pre-order")
            }
        }
    }
}

/// Validate the structured observability record against the spec invariants.
/// Returns every violation found (empty `Ok(())` when the record conforms), so
/// a caller can report all problems at once rather than the first.
///
/// Observability only: this does not look at fees, atomicity, or fork state.
pub fn validate_observability(
    frames: &[crate::frame::Frame],
    trace: Option<&crate::trace::InstructionTrace>,
    caps: &crate::Capabilities,
) -> Result<(), Vec<ObservabilityViolation>> {
    use ObservabilityViolation as V;
    let mut errs = Vec::new();

    // I1: a declared per_frame_trace must be populated.
    let trace_present = trace.is_some_and(|t| !t.0.is_empty());
    if caps.per_frame_trace && !trace_present {
        errs.push(V::TraceDeclaredButAbsent);
    }

    if let Some(trace) = trace {
        // I2: stack heights are valid: top-level is 1, each step descends by at
        // most one, none is below 1.
        if let Some(first) = trace.0.first() {
            if first.stack_height != 1 {
                errs.push(V::TopLevelStackHeight {
                    found: first.stack_height,
                });
            }
        }
        let mut prev = 0usize;
        for (i, ti) in trace.0.iter().enumerate() {
            if ti.stack_height < 1 {
                errs.push(V::StackHeightBelowOne {
                    frame: i,
                    height: ti.stack_height,
                });
            }
            if i > 0 && ti.stack_height > prev + 1 {
                errs.push(V::StackHeightJumped {
                    frame: i,
                    from: prev,
                    to: ti.stack_height,
                });
            }
            prev = ti.stack_height;
        }

        // I4 (removed): "no default pubkeys in the trace" cannot be checked
        // reliably because the System program's address is all zeros, identical
        // to `Pubkey::default()`. Any program that CPIs into the System program
        // would produce a false positive. The underlying adapter bug this check
        // was written to catch (quasar's `extract_execution_trace` indexing into
        // `sanitized_message.account_keys()` instead of the full transaction
        // context key list) is now fixed at the source.

        // I3: a structured_cpi engine sources frames from the trace, so the
        // flattened frame tree matches the trace in count and program ids.
        if caps.structured_cpi {
            let flat = crate::frame::preorder_program_ids(frames);
            let traced: Vec<Pubkey> = trace.0.iter().map(|ti| ti.program_id).collect();
            if flat.len() != traced.len() {
                errs.push(V::FrameCountMismatch {
                    frames: flat.len(),
                    trace: traced.len(),
                });
            } else if flat != traced {
                errs.push(V::FrameProgramIdsMismatch);
            }
        }
    }

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

/// Assert a rendered artifact matches its committed golden file. `UPDATE_GOLDEN`
/// (re)writes goldens instead of comparing; do that only when intentionally
/// regenerating, and review the diff before committing. The value is a per-golden
/// filter, so a regenerate stays scoped: `1`, `true`, `all`, or a bare presence
/// rewrites every golden, while any other value is a path substring that rewrites
/// only matching goldens (`UPDATE_GOLDEN=counter_authority_graph`) and leaves the
/// rest asserting, so a divergence outside the target is not masked.
pub fn assert_golden(golden_path: &str, actual: &str) {
    let update = should_update(golden_path, std::env::var_os("UPDATE_GOLDEN").as_deref());
    assert_golden_inner(golden_path, actual, update);
}

/// Whether `assert_golden` writes (vs compares) THIS golden, from the
/// `UPDATE_GOLDEN` value. Unset compares; a blanket value (`1`/`true`/`all`,
/// case-insensitive, or empty) writes every golden; any other value is a path
/// substring that writes only goldens whose path contains it.
fn should_update(golden_path: &str, update_var: Option<&std::ffi::OsStr>) -> bool {
    let Some(value) = update_var else { return false };
    // Non-UTF8 presence: no sensible substring, so treat as a blanket on.
    let Some(value) = value.to_str() else { return true };
    let value = value.trim();
    value.is_empty()
        || value.eq_ignore_ascii_case("1")
        || value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("all")
        || golden_path.contains(value)
}

fn assert_golden_inner(golden_path: &str, actual: &str, update: bool) {
    if update {
        if let Some(parent) = std::path::Path::new(golden_path).parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("assert_golden: create {parent:?}: {e}"));
        }
        std::fs::write(golden_path, actual)
            .unwrap_or_else(|e| panic!("assert_golden: write {golden_path}: {e}"));
        return;
    }
    let expected = std::fs::read_to_string(golden_path).unwrap_or_else(|e| {
        panic!("assert_golden: read {golden_path}: {e}\nRegenerate with UPDATE_GOLDEN=1")
    });
    // `assert_eq!` labels its first arg `left` and second `right`; put expected
    // (the golden) left and actual right so the macro's labels read with the
    // custom message rather than against it.
    assert_eq!(
        expected, actual,
        "golden mismatch at {golden_path}\n--- expected (golden) ---\n{expected}\n--- actual ---\n{actual}"
    );
}

/// Fund a payer, send a System transfer, assert the model, warp the clock.
/// Capability-gated assertions cover the engine differences.
pub fn scenario<B: TestSVM>(backend: &mut B) {
    // One declaration, three effects: deterministic keypair, alias, funding.
    let payer = backend.actor("payer", 1_000_000_000);

    let dest = Pubkey::new_unique();
    let ix = solana_system_interface::instruction::transfer(&payer.pubkey(), &dest, 2_000_000);
    let tx = backend.send(&[ix], &[&payer]);

    // Universal: every engine yields a model the renderers can consume.
    assert!(
        tx.error.is_none(),
        "transfer should succeed: {:?}",
        tx.error
    );
    assert!(
        !tx.frames.is_empty(),
        "structured frames present on every engine"
    );
    assert!(
        !tx.account_keys.is_empty(),
        "indices never ship without their frame"
    );
    assert!(!tx.logs.is_empty(), "logs are the floor on every engine");
    assert_eq!(
        backend.account_owner(&dest),
        Some(solana_system_interface::program::id()),
    );

    // Capability-gated: engines differ, and say so.
    let caps = backend.capabilities();
    if caps.fees {
        assert!(tx.fee.is_some(), "engines that model fees report them");
    } else {
        assert!(tx.fee.is_none(), "absent facts are absent, not zero");
    }
    if caps.per_frame_trace {
        assert!(tx.trace.is_some(), "trace-capable engines populate it");
    }

    // The relocated renderers run on every engine's record, not just litesvm's.
    // The authority graph draws the transfer's top-level roles (signer + the
    // account it writes), which come from the message, so it renders the same on
    // every engine. Edges emit as Mermaid `-->|verb|`.
    let authority = tx.authority_graph_string();
    assert!(
        authority.contains("|signs|") && authority.contains("|writes|"),
        "authority graph renders the transfer's roles on every engine:\n{authority}"
    );

    // The ownership graph is the trace-gated one: an account's owner is not in
    // the message or the logs, only in the per-frame trace. A trace-capable
    // engine names the destination's owner (`-->|owns|`); one without degrades
    // to no owner edges, the graceful path the relocation documents.
    let ownership = tx.ownership_graph_string();
    if caps.per_frame_trace {
        assert!(
            ownership.contains("|owns|"),
            "trace-sourced owners reach the ownership graph:\n{ownership}"
        );
    } else {
        assert!(
            !ownership.contains("|owns|"),
            "without a trace the ownership graph has no owner to draw:\n{ownership}"
        );
    }

    // The lever that could not go cross-engine before this trait.
    backend.warp_to_timestamp(1_700_000_000);
    assert_eq!(backend.clock().unix_timestamp, 1_700_000_000);
}

#[cfg(test)]
mod validate_tests {
    use crate::{
        frame::{Frame, Outcome},
        trace::{InstructionTrace, TracedAccount, TracedInstruction},
        Capabilities,
    };
    use solana_pubkey::Pubkey;
    use super::{validate_observability, ObservabilityViolation};

    fn caps_full() -> Capabilities {
        Capabilities {
            per_frame_trace: true,
            structured_cpi: true,
            atomic_send: true,
            fees: true,
            instant_reset: true,
            fork: false,
        }
    }

    fn frame(program_id: Pubkey, children: Vec<Frame>) -> Frame {
        Frame {
            program_id,
            outcome: Outcome::Success,
            compute_units: None,
            instruction_name: None,
            operands: Vec::new(),
            logs: Vec::new(),
            children,
        }
    }

    fn ti(program_id: Pubkey, stack_height: usize) -> TracedInstruction {
        TracedInstruction {
            program_id,
            stack_height,
            accounts: vec![TracedAccount {
                pubkey: Pubkey::new_unique(),
                is_signer: true,
                is_writable: true,
                owner: Pubkey::new_unique(),
            }],
            data: vec![],
        }
    }

    #[test]
    fn ok_when_frames_and_trace_agree() {
        let a = Pubkey::new_unique();
        let b = Pubkey::new_unique();
        let frames = vec![frame(a, vec![frame(b, vec![])])];
        let trace = InstructionTrace(vec![ti(a, 1), ti(b, 2)]);
        assert_eq!(
            validate_observability(&frames, Some(&trace), &caps_full()),
            Ok(())
        );
    }

    #[test]
    fn err_when_per_frame_trace_declared_but_empty() {
        let frames = vec![frame(Pubkey::new_unique(), vec![])];
        let errs = validate_observability(&frames, None, &caps_full()).unwrap_err();
        assert!(
            errs.contains(&ObservabilityViolation::TraceDeclaredButAbsent),
            "got: {errs:?}"
        );
    }

    #[test]
    fn err_when_top_level_stack_height_not_one() {
        let a = Pubkey::new_unique();
        let frames = vec![frame(a, vec![])];
        let trace = InstructionTrace(vec![ti(a, 2)]);
        let errs = validate_observability(&frames, Some(&trace), &caps_full()).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ObservabilityViolation::TopLevelStackHeight { found: 2 })),
            "got: {errs:?}"
        );
    }

    #[test]
    fn err_when_structured_cpi_frames_disagree_with_trace() {
        let a = Pubkey::new_unique();
        let b = Pubkey::new_unique();
        // Trace has two frames; the frame tree has only one.
        let frames = vec![frame(a, vec![])];
        let trace = InstructionTrace(vec![ti(a, 1), ti(b, 2)]);
        let errs = validate_observability(&frames, Some(&trace), &caps_full()).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ObservabilityViolation::FrameCountMismatch { .. })),
            "got: {errs:?}"
        );
    }

    // NOTE: I4 ("no default pubkeys in the trace") was removed because
    // Pubkey::default() (all zeros) is the System program's valid address.
    // There is no sentinel that distinguishes an unresolved lookup fallback
    // from a legitimate System program reference; the adapter-level bug that
    // motivated I4 (quasar indexing into the wrong key slice) is fixed
    // upstream. A test that asserts an error on Pubkey::default() would
    // produce a false positive for any CPI that calls the System program.
}

#[cfg(test)]
mod golden_tests {
    use super::*;

    fn tmp_path(name: &str) -> String {
        // Process- and test-unique path under the target dir; no Date/random.
        format!(
            "{}/target/golden-test-{}-{}.txt",
            env!("CARGO_MANIFEST_DIR"),
            std::process::id(),
            name
        )
    }

    /// Removes its path on drop, so a `#[should_panic]` test that writes a file
    /// before panicking does not leak it (the unwind runs the drop).
    struct CleanupGuard<'a>(&'a str);
    impl Drop for CleanupGuard<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0);
        }
    }

    #[test]
    fn update_writes_then_compare_passes() {
        let path = tmp_path("write");
        let _ = std::fs::remove_file(&path);
        assert_golden_inner(&path, "hello\nworld\n", true); // writes
        assert_golden_inner(&path, "hello\nworld\n", false); // matches
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    #[should_panic(expected = "golden mismatch")]
    fn mismatch_panics() {
        let path = tmp_path("mismatch");
        let _guard = CleanupGuard(&path);
        assert_golden_inner(&path, "expected\n", true);
        assert_golden_inner(&path, "different\n", false);
    }

    #[test]
    #[should_panic(expected = "UPDATE_GOLDEN")]
    fn missing_golden_panics_with_instruction() {
        let path = tmp_path("missing");
        let _ = std::fs::remove_file(&path);
        assert_golden_inner(&path, "anything\n", false);
    }

    #[test]
    fn update_var_is_a_per_golden_path_filter() {
        use std::ffi::OsStr;
        let cpi = "probe/golden/counter_cpi_tree.txt";
        let graph = "probe/golden/counter_authority_graph.txt";

        // Unset: compare, never write.
        assert!(!should_update(cpi, None));

        // Blanket forms update every golden (the explicit regenerate-all).
        assert!(should_update(cpi, Some(OsStr::new("1"))));
        assert!(should_update(graph, Some(OsStr::new("all"))));
        assert!(should_update(graph, Some(OsStr::new("TRUE"))));
        assert!(should_update(cpi, Some(OsStr::new("")))); // bare presence = blanket

        // A substring targets one golden; the others still assert, so a
        // divergence outside the target is not masked.
        assert!(should_update(cpi, Some(OsStr::new("counter_cpi_tree"))));
        assert!(!should_update(graph, Some(OsStr::new("counter_cpi_tree"))));
    }
}
