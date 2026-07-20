//! The narrative test, frood edition: a story world over a committed `.so`
//! plus its Codama IDL, no program crate anywhere in the graph. The book's
//! "Narrative on frood" section includes each anchored region below; the
//! whole file compiles and passes in CI, so a snippet the book shows can
//! never drift from code that runs.

use frood::blocks::{authority, cast, ownership, sequence, tree, Lifelines};
use frood::{Actor, BuiltIx, Outcome, ReportConfig, ReportState, Reporter, Story};
use frood_idl::types::Value;
use frood_idl::FromValue;
use solana_pubkey::Pubkey;

const SO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/counter.so");
const IDL: &str = include_str!("../idls/counter.codama.json");

// ANCHOR: standard
/// The suite's report standard, declared once, beside the world it
/// configures: each block carries its own options (`sequence` its style,
/// `.collapsed()` composes on any block). A test that deviates assigns its
/// own `ReportConfig::of(...)` through `report_state().config_mut()`.
fn report_standard() -> ReportConfig {
    ReportConfig::of([
        cast(),
        sequence(Lifelines),
        authority(),
        ownership(),
        tree().collapsed(),
    ])
}
// ANCHOR_END: standard

// ANCHOR: view
/// The on-chain counter, decoded: each snake_case field reads its camelCase
/// IDL spelling, so the per-suite `as_u64`/`as_pk` decoder zoo never exists.
#[derive(FromValue)]
struct CounterView {
    count: u64,
}
// ANCHOR_END: view

// ANCHOR: world
#[derive(Reporter)]
struct CounterWorld {
    story: Story,
    payer: Actor,
    counter: Pubkey,
    report: ReportState,
}

impl Drop for CounterWorld {
    fn drop(&mut self) {
        // The lifecycle's last safety net: a law that broke and that nothing
        // ever asserted on must still fail the test, even on a plain
        // `cargo test`. Rendering is a projection of the trajectory, run
        // only when a report was asked for (`FROOD_LINK_REPORT_DIR`).
        self.story.conclusion();
        if self.report.enabled() {
            let config = self.report.config();
            let arc = self.story.project(&config);
            self.report.set_arc(arc);
        }
        self.finish();
    }
}

impl CounterWorld {
    #[track_caller]
    fn live() -> CounterWorld {
        let mut story = Story::load(SO, IDL);
        let payer = story.cast("Payer");
        let counter = story.cast_unfunded("Counter");
        story.when_ok(
            "Initialize",
            story.ix(
                "initialize",
                &[("counter", counter.pubkey()), ("payer", payer.pubkey())],
                Value::Struct(vec![]),
            ),
            &[&payer, &counter],
        );
        // Registered once, sampled into every subsequent moment on its own;
        // the law is evaluated at every mint and a break is located to its T.
        let counter_pk = counter.pubkey();
        let count_obs = story.observe("count", move |s| {
            let view = CounterView::from_value(&s.account_as("counter", &counter_pk))
                .expect("counter decodes as CounterView");
            Value::U64(view.count)
        });
        story.monotonic(count_obs);
        CounterWorld {
            story,
            payer,
            counter: counter_pk,
            report: ReportState::new(report_standard()),
        }
    }

    fn ix_increment(&self, by: u64) -> BuiltIx {
        self.story.ix(
            "increment",
            &[("counter", self.counter)],
            Value::Struct(vec![("by".into(), Value::U64(by))]),
        )
    }
}
// ANCHOR_END: world

impl CounterWorld {
    // ANCHOR: sends
    /// A beat the rest of the story depends on: `when_ok` panics on refusal,
    /// with the story so far, at the caller's line.
    fn increment_ok(&mut self, by: u64) -> Outcome {
        let ix = self.ix_increment(by);
        let out = self.story.when_ok("Increment", ix, &[&self.payer]);
        self.story.svm.expire_blockhash();
        out
    }

    /// A refusal that is itself the behavior under test: `when_err` settles
    /// it as a Then claim ("refused: zeroIncrement"), and panics if the
    /// action succeeds anyway.
    fn increment_err(&mut self, by: u64, error: &str) -> Outcome {
        let ix = self.ix_increment(by);
        let out = self.story.when_err("Increment", ix, &[&self.payer], error);
        self.story.svm.expire_blockhash();
        out
    }
    // ANCHOR_END: sends

    // ANCHOR: raw
    /// A send outside `when`'s bundle vocabulary (a foreign program's
    /// instruction, explicit metas) is still a named beat of the story:
    /// unlabeled it would render as "(instruction)" and be invisible to
    /// `count_actions`.
    fn bump_raw(&mut self) -> Outcome {
        let ix = self.ix_increment(1).instruction().clone();
        let out = self
            .story
            .run_instruction_as("Maintenance", ix, &[&self.payer]);
        self.story.svm.expire_blockhash();
        out
    }
    // ANCHOR_END: raw
}

// ANCHOR: narrative
#[test]
fn count_grows_and_zero_is_refused() {
    let mut world = CounterWorld::live();
    world.increment_ok(2);
    world.increment_err(0, "zeroIncrement");
    world.bump_raw();
    // Terminal facts settle at conclusion (the world's drop); the raw
    // "Maintenance" beat is deliberately not an "Increment".
    world.story.finally("exactly two Increment beats settled", |seen| {
        seen.count_actions("Increment") == 2
    });
}
// ANCHOR_END: narrative

// ANCHOR: attenuate
#[test]
fn cu_focused_report() {
    let mut world = CounterWorld::live();
    // This test's report deviates from the standard, in the test that owns
    // the deviation: typed, rename-safe, runner-agnostic.
    *world.report_state().config_mut() =
        ReportConfig::of([sequence(Lifelines), tree().collapsed()]);
    world.increment_ok(1);
}
// ANCHOR_END: attenuate
