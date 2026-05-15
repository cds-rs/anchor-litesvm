use super::*;

#[test]
fn parse_preserves_siblings() {
    // Multiple invocations at the same depth (siblings) all become children
    // of the parent. Guards against the regression where retain() dropped
    // siblings.
    let logs = vec![
        "Program ParentProgram invoke [1]".to_string(),
        "Program ChildA invoke [2]".to_string(),
        "Program ChildA success".to_string(),
        "Program ChildB invoke [2]".to_string(),
        "Program ChildB success".to_string(),
        "Program ChildC invoke [2]".to_string(),
        "Program ChildC success".to_string(),
        "Program ParentProgram success".to_string(),
    ];

    let roots = parse(&logs);

    assert_eq!(roots.len(), 1);
    let parent = &roots[0];
    assert_eq!(parent.info, "ParentProgram");
    assert_eq!(parent.children.len(), 3);
    assert_eq!(parent.children[0].info, "ChildA");
    assert_eq!(parent.children[1].info, "ChildB");
    assert_eq!(parent.children[2].info, "ChildC");
}

#[test]
fn parse_captures_outcome_and_compute_units() {
    let logs = vec![
        "Program ParentProgram invoke [1]".to_string(),
        "Program ChildA invoke [2]".to_string(),
        "Program ChildA consumed 105 of 184618 compute units".to_string(),
        "Program ChildA success".to_string(),
        "Program ChildB invoke [2]".to_string(),
        "Program ChildB consumed 118 of 182783 compute units".to_string(),
        "Program ChildB failed: custom program error: 0x123".to_string(),
        "Program ParentProgram consumed 17913 of 200000 compute units".to_string(),
        "Program ParentProgram success".to_string(),
    ];

    let roots = parse(&logs);

    assert_eq!(roots.len(), 1);
    let parent = &roots[0];
    assert_eq!(parent.info, "ParentProgram");
    assert_eq!(parent.outcome, Some(Outcome::Success));
    assert_eq!(parent.compute_units, Some(17913));
    assert_eq!(parent.children.len(), 2);

    let child_a = &parent.children[0];
    assert_eq!(child_a.info, "ChildA");
    assert_eq!(child_a.outcome, Some(Outcome::Success));
    assert_eq!(child_a.compute_units, Some(105));

    let child_b = &parent.children[1];
    assert_eq!(child_b.info, "ChildB");
    assert_eq!(
        child_b.outcome,
        Some(Outcome::Failed { message: Some("custom program error: 0x123".to_string()) })
    );
    assert_eq!(child_b.compute_units, Some(118));
}

#[test]
fn matching_is_position_based_not_name_based() {
    // Same program name appears as outer caller, nested call, and sibling.
    // Stack-based matching attributes each consumed/success to the correct
    // node regardless of name collisions.
    let logs = vec![
        "Program X invoke [1]".to_string(),
        "Program X invoke [2]".to_string(),
        "Program X consumed 10 of 200000 compute units".to_string(),
        "Program X success".to_string(),
        "Program X invoke [2]".to_string(),
        "Program X consumed 20 of 200000 compute units".to_string(),
        "Program X failed: inner kaboom".to_string(),
        "Program X consumed 100 of 200000 compute units".to_string(),
        "Program X success".to_string(),
    ];

    let roots = parse(&logs);

    assert_eq!(roots.len(), 1);
    let outer = &roots[0];
    assert_eq!(outer.compute_units, Some(100));
    assert_eq!(outer.outcome, Some(Outcome::Success));
    assert_eq!(outer.children.len(), 2);

    assert_eq!(outer.children[0].compute_units, Some(10));
    assert_eq!(outer.children[0].outcome, Some(Outcome::Success));

    assert_eq!(outer.children[1].compute_units, Some(20));
    assert_eq!(
        outer.children[1].outcome,
        Some(Outcome::Failed { message: Some("inner kaboom".to_string()) })
    );
}

#[test]
fn parse_captures_error_message() {
    let logs = vec![
        "Program ParentProgram invoke [1]".to_string(),
        "Program ChildA invoke [2]".to_string(),
        "Program ChildA failed: custom program error: 0xbc4".to_string(),
        "Program ParentProgram failed: Program failed to complete: insufficient funds".to_string(),
    ];

    let roots = parse(&logs);

    assert_eq!(roots.len(), 1);
    let parent = &roots[0];
    assert_eq!(
        parent.outcome,
        Some(Outcome::Failed { message: Some("Program failed to complete: insufficient funds".to_string()) })
    );
    assert_eq!(parent.children.len(), 1);
    assert_eq!(
        parent.children[0].outcome,
        Some(Outcome::Failed { message: Some("custom program error: 0xbc4".to_string()) })
    );
}

mod props {
    use super::super::*;
    use proptest::prelude::*;
    use proptest::test_runner::TestCaseError;

    #[derive(Debug, Clone)]
    struct SimTree {
        name: String,
        outcome: SimOutcome,
        compute_units: Option<u64>,
        children: Vec<SimTree>,
    }

    #[derive(Debug, Clone)]
    enum SimOutcome {
        Success,
        Failed(Option<String>),
    }

    fn emit(tree: &SimTree, depth: usize, out: &mut Vec<String>) {
        out.push(format!("Program {} invoke [{}]", tree.name, depth));
        for child in &tree.children {
            emit(child, depth + 1, out);
        }
        if let Some(cu) = tree.compute_units {
            out.push(format!(
                "Program {} consumed {} of 200000 compute units",
                tree.name, cu
            ));
        }
        match &tree.outcome {
            SimOutcome::Success => out.push(format!("Program {} success", tree.name)),
            SimOutcome::Failed(Some(msg)) => {
                out.push(format!("Program {} failed: {}", tree.name, msg))
            }
            SimOutcome::Failed(None) => out.push(format!("Program {} failed:", tree.name)),
        }
    }

    // Names use uppercase ASCII only, which avoids every parser hook
    // (`invoke`, `consumed`, `success`, `failed`, `Program log:`).
    //
    // Failure messages use a broad alphabet including digits and spaces.
    // We trim leading/trailing whitespace at strategy time because the
    // parser trims the same way: a generated `"  "` would round-trip as
    // `None`, breaking exact equality. After trim, empty becomes `None`,
    // non-empty becomes `Some(...)`.
    fn arb_outcome() -> impl Strategy<Value = SimOutcome> {
        prop_oneof![
            Just(SimOutcome::Success),
            "[A-Za-z0-9 ]{0,40}".prop_map(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    SimOutcome::Failed(None)
                } else {
                    SimOutcome::Failed(Some(trimmed.to_string()))
                }
            }),
        ]
    }

    // Names now use a broad alphabet — mixed case, digits, underscores —
    // because the parser keyword constants are anchored and tolerate names
    // like "invoker", "consumed_tracker", "success", or "failed".
    const NAME_REGEX: &str = "[A-Za-z0-9_]{1,8}";

    fn arb_tree() -> impl Strategy<Value = SimTree> {
        let leaf = (
            NAME_REGEX,
            arb_outcome(),
            prop::option::of(any::<u64>()),
        )
            .prop_map(|(name, outcome, compute_units)| SimTree {
                name,
                outcome,
                compute_units,
                children: vec![],
            });

        leaf.prop_recursive(
            4,  // up to 4 levels of nesting
            32, // up to 32 total nodes per generated tree
            3,  // up to 3 children per node
            |inner| {
                (
                    NAME_REGEX,
                    arb_outcome(),
                    prop::option::of(any::<u64>()),
                    prop::collection::vec(inner, 0..3),
                )
                    .prop_map(|(name, outcome, compute_units, children)| SimTree {
                        name,
                        outcome,
                        compute_units,
                        children,
                    })
            },
        )
    }

    fn assert_matches(sim: &SimTree, parsed: &TreeNode) -> Result<(), TestCaseError> {
        prop_assert_eq!(&sim.name, &parsed.info);
        let expected = match &sim.outcome {
            SimOutcome::Success => Outcome::Success,
            SimOutcome::Failed(message) => Outcome::Failed {
                message: message.clone(),
            },
        };
        prop_assert_eq!(parsed.outcome.as_ref(), Some(&expected));
        prop_assert_eq!(sim.compute_units, parsed.compute_units);
        prop_assert_eq!(sim.children.len(), parsed.children.len());
        for (s, p) in sim.children.iter().zip(parsed.children.iter()) {
            assert_matches(s, p)?;
        }
        Ok(())
    }

    proptest! {
        /// Generate a forest of simulated invocation trees, serialize them
        /// to Solana-style log lines, parse them back, and assert the
        /// resulting tree matches the original shape and data.
        #[test]
        fn parse_roundtrip(forest in prop::collection::vec(arb_tree(), 1..4)) {
            let mut logs = vec![];
            for tree in &forest {
                emit(tree, 1, &mut logs);
            }
            let parsed = parse(&logs);
            prop_assert_eq!(parsed.len(), forest.len());
            for (sim, root) in forest.iter().zip(parsed.iter()) {
                assert_matches(sim, root)?;
            }
        }

        /// Parsing arbitrary garbage must never panic. The output may be
        /// nonsense, but the function must return.
        #[test]
        fn parse_never_panics(logs in prop::collection::vec(".*", 0..50)) {
            let _ = parse(&logs);
        }

        /// Render must never panic and must either produce empty output or
        /// start with the "Transaction\n" header.
        #[test]
        fn render_well_formed(logs in prop::collection::vec(".*", 0..50)) {
            let out = render(&logs);
            prop_assert!(out.is_empty() || out.starts_with("Transaction\n"));
        }
    }
}

#[test]
fn names_containing_keyword_substrings() {
    // Program names that contain the bare substrings `invoke` or `consumed`
    // (or are literally those keywords) used to fool the parser into
    // matching the keyword position inside the name. Keyword constants now
    // require surrounding context (" invoke [" / " consumed ") which fixes
    // it; `rfind` for consumed handles the bare-keyword-as-name case where
    // " consumed " appears twice in the line.
    let logs = vec![
        "Program invoker invoke [1]".to_string(),
        "Program invoker consumed 50 of 200000 compute units".to_string(),
        "Program invoker success".to_string(),
        "Program consumed_tracker invoke [1]".to_string(),
        "Program consumed_tracker consumed 75 of 200000 compute units".to_string(),
        "Program consumed_tracker success".to_string(),
        "Program consumed invoke [1]".to_string(),
        "Program consumed consumed 99 of 200000 compute units".to_string(),
        "Program consumed success".to_string(),
        "Program success invoke [1]".to_string(),
        "Program success success".to_string(),
    ];

    let roots = parse(&logs);

    assert_eq!(roots.len(), 4);
    assert_eq!(roots[0].info, "invoker");
    assert_eq!(roots[0].compute_units, Some(50));
    assert_eq!(roots[0].outcome, Some(Outcome::Success));

    assert_eq!(roots[1].info, "consumed_tracker");
    assert_eq!(roots[1].compute_units, Some(75));
    assert_eq!(roots[1].outcome, Some(Outcome::Success));

    assert_eq!(roots[2].info, "consumed");
    assert_eq!(roots[2].compute_units, Some(99));
    assert_eq!(roots[2].outcome, Some(Outcome::Success));

    assert_eq!(roots[3].info, "success");
    assert_eq!(roots[3].outcome, Some(Outcome::Success));
}

#[test]
fn failed_message_can_contain_other_line_shapes() {
    // A failed-status message body is free-form and can incidentally look
    // like a consumed line, an invoke line, or a success line. The parser
    // must always treat such lines as Failed status, not mis-classify them.
    let logs = vec![
        "Program A invoke [1]".to_string(),
        "Program A failed: consumed 100 of 200 compute units".to_string(),
        "Program B invoke [1]".to_string(),
        "Program B failed: invoke [42]".to_string(),
        "Program C invoke [1]".to_string(),
        "Program C failed: success not achieved".to_string(),
    ];

    let roots = parse(&logs);

    assert_eq!(roots.len(), 3);
    assert_eq!(
        roots[0].outcome,
        Some(Outcome::Failed { message: Some("consumed 100 of 200 compute units".to_string()) })
    );
    // The "consumed-looking" message must not have set compute_units.
    assert_eq!(roots[0].compute_units, None);

    assert_eq!(
        roots[1].outcome,
        Some(Outcome::Failed { message: Some("invoke [42]".to_string()) })
    );
    // The "invoke-looking" message must not have spawned a phantom child.
    assert_eq!(roots[1].children.len(), 0);

    assert_eq!(
        roots[2].outcome,
        Some(Outcome::Failed { message: Some("success not achieved".to_string()) })
    );
}

#[test]
fn parse_truncated_logs_keeps_partial_structure() {
    // If the log stream is cut off before children/parents close, we should
    // still return whatever structure exists rather than dropping nodes.
    let logs = vec![
        "Program Outer invoke [1]".to_string(),
        "Program Inner invoke [2]".to_string(),
        // No success/failed lines — both invocations are left open
    ];

    let roots = parse(&logs);

    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].info, "Outer");
    assert_eq!(roots[0].outcome, None);
    assert_eq!(roots[0].children.len(), 1);
    assert_eq!(roots[0].children[0].info, "Inner");
    assert_eq!(roots[0].children[0].outcome, None);
}
