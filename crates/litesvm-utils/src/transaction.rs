//! Transaction execution and result handling utilities
//!
//! This module provides convenient wrappers for executing transactions
//! and handling their results in tests.

mod aliases;
mod events;
mod mermaid;
mod signers;
mod style;
mod tree;

pub use aliases::Aliases;
pub use events::{EventInfo, EventRegistry};

use litesvm::types::TransactionMetadata;
use litesvm::LiteSVM;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::message::Message;
use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use std::fmt;
use thiserror::Error;

/// Total width (in `char`s, not bytes) the structured-log section header
/// fills with the trailing `─` rule. Slightly wider than typical tree
/// content (~45–55 chars) so the rule visibly "extends past" the body
/// and reads as a section break rather than a label.
const HEADER_WIDTH: usize = 60;

/// Minimum trailing `─` count when the title alone exceeds `HEADER_WIDTH`
/// (very long program::ix names). Matches the pre-fill format so headers
/// never visually collapse into the title.
const HEADER_MIN_TRAILING: usize = 4;

/// Write `title` followed by enough `─` characters to reach `HEADER_WIDTH`
/// (or `HEADER_MIN_TRAILING` dashes when the title is already wider), then
/// a newline. `title` is expected to end with a single ASCII space so the
/// rule sits flush against it.
fn push_section_header(out: &mut String, title: &str) {
    out.push_str(title);
    let trailing = HEADER_WIDTH
        .saturating_sub(title.chars().count())
        .max(HEADER_MIN_TRAILING);
    for _ in 0..trailing {
        out.push('─');
    }
    out.push('\n');
}

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("Transaction execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Transaction build error: {0}")]
    BuildError(String),

    #[error("Assertion failed: {0}")]
    AssertionFailed(String),
}

/// Wrapper around LiteSVM's TransactionMetadata with helper methods for testing
///
/// This struct provides convenient methods for analyzing transaction results,
/// including log inspection, compute unit tracking, and success assertions.
///
/// # Example
///
/// ```ignore
/// let result = svm.send_instruction(ix, &[&signer])?
///     .tap(|r| {
///         assert!(r.has_log("Transfer complete"));
///         println!("Used {} compute units", r.compute_units());
///     })
///     .assert_success();
/// ```
/// The program ID + serialized data of a single top-level instruction.
///
/// Carried on [`TransactionResult`] for single-instruction sends so the
/// structured-logs printer can resolve the program ID through aliases at
/// render time *and* decode the instruction name via the same discriminator
/// table the tree uses for inner frames (System, SPL Token, ATA). Anchor
/// user programs hash through `decode_instruction` as `None` today and fall
/// back to just the program name; adding an IDL-driven decode is a separate
/// concern.
///
/// `None` on `TransactionResult` for batches (multi-ix sends) and for the
/// raw `send_transaction_result` path, since neither carries a single
/// canonical "the instruction" to decode.
#[derive(Debug, Clone)]
pub struct InstructionInfo {
    pub program_id: Pubkey,
    /// Full instruction data. Only the first 1-8 bytes are ever read (by
    /// `decode_instruction`); the rest is preserved because copying a few
    /// hundred bytes per test is cheaper than worrying about which slice
    /// width covers every future decoder.
    pub data: Box<[u8]>,
}

impl InstructionInfo {
    /// Capture program ID + a clone of the data bytes from an
    /// `Instruction`. Used by `send_instruction` / `execute_instruction`
    /// to stash the ix info before the original `Instruction` is moved
    /// into a `Transaction`.
    pub fn from_instruction(ix: &Instruction) -> Self {
        Self {
            program_id: ix.program_id,
            data: ix.data.clone().into_boxed_slice(),
        }
    }
}

pub struct TransactionResult {
    inner: TransactionMetadata,
    /// Top-level instruction info when the caller built the
    /// `TransactionResult` from a single instruction. Drives the
    /// `Instruction: <Program>::<Name>` header line in
    /// [`logs_structured_string`](Self::logs_structured_string). See
    /// [`InstructionInfo`] for the `None` cases.
    instruction: Option<InstructionInfo>,
    error: Option<String>,
    pub(crate) message: Message,
    /// Pubkey-to-friendly-name table used by `print_logs_structured` /
    /// `logs_structured_string`. Set via [`with_aliases`](Self::with_aliases);
    /// `None` falls back to [`Aliases::default`] (well-known programs only).
    aliases: Option<Aliases>,
    /// Event decoders, so a `Program data:` payload renders by name and
    /// destructured fields (a mermaid `note`, an indented tree line) instead of
    /// raw base64. Empty until an event type is registered via
    /// `AnchorContext::register_event`; set via
    /// [`with_event_registry`](Self::with_event_registry).
    event_registry: EventRegistry,
}

impl TransactionResult {
    /// Create a new TransactionResult wrapper for a successful transaction.
    ///
    /// Pass `Some(_)` for `instruction` only when the transaction wraps a
    /// single instruction (so the structured-logs header has something
    /// canonical to render); pass `None` for batches.
    pub fn new(
        result: TransactionMetadata,
        instruction: Option<InstructionInfo>,
        message: Message,
    ) -> Self {
        Self {
            inner: result,
            instruction,
            error: None,
            message,
            aliases: None,
            event_registry: EventRegistry::new(),
        }
    }

    /// Create a new TransactionResult wrapper for a failed transaction.
    ///
    /// See [`new`](Self::new) for the `instruction` convention.
    pub fn new_failed(
        error: String,
        result: TransactionMetadata,
        instruction: Option<InstructionInfo>,
        message: Message,
    ) -> Self {
        Self {
            inner: result,
            instruction,
            error: Some(error),
            message,
            aliases: None,
            event_registry: EventRegistry::new(),
        }
    }

    /// Attach an alias table that drives subsequent
    /// [`print_logs_structured`](Self::print_logs_structured) /
    /// [`logs_structured_string`](Self::logs_structured_string) calls.
    /// Returns `self` for chaining; the table is cloned in (cheap), so
    /// the caller keeps ownership of the original.
    pub fn with_aliases(mut self, aliases: Aliases) -> Self {
        self.aliases = Some(aliases);
        self
    }

    /// Attach a table of event decoders so a `Program data:` payload renders by
    /// name and fields instead of raw base64. Cloned in (cheap: the decoders
    /// are `Arc`d); chainable. The `AnchorContext` send helpers attach the
    /// context's registry automatically; populate it with
    /// `AnchorContext::register_event::<E>()`. See [`EventRegistry`].
    pub fn with_event_registry(mut self, events: EventRegistry) -> Self {
        self.event_registry = events;
        self
    }

    /// Assert that the transaction succeeded, panic with logs if it failed.
    ///
    /// Consumes and returns `self` so the result can flow into a further
    /// chain (`...assert_success().print_logs_structured()`) or
    /// be bound at chain end (`let result = svm.send_ok(...).assert_success();`).
    /// Read-only inspection inside a chain goes through [`tap`](Self::tap),
    /// which borrows for the closure and returns the owned value back.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = svm.send_instruction(ix, &[&payer])?.assert_success();
    /// ```
    pub fn assert_success(self) -> Self {
        assert!(
            self.error.is_none(),
            "Transaction failed: {}\nLogs:\n{}",
            self.error.as_ref().unwrap_or(&"Unknown error".to_string()),
            self.logs().join("\n")
        );
        self
    }

    /// Assert that the transaction succeeded AND a caller-supplied
    /// predicate holds on the result. Panics with context on either
    /// failure. Consumes and returns `self`.
    ///
    /// Useful for baking an additional check (compute units, log
    /// presence, custom invariant) into a single chain step rather than
    /// reaching for [`tap`](Self::tap) + a separate
    /// [`assert_success`](Self::assert_success).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let r = svm.send_ok(ix, &[&payer], &aliases)
    ///     .assert_success_with(|r| r.compute_units() < 100_000);
    /// ```
    pub fn assert_success_with<F>(self, predicate: F) -> Self
    where
        F: FnOnce(&Self) -> bool,
    {
        assert!(
            self.error.is_none(),
            "Transaction failed: {}\nLogs:\n{}",
            self.error.as_ref().unwrap_or(&"Unknown error".to_string()),
            self.logs().join("\n")
        );
        assert!(
            predicate(&self),
            "Predicate failed on successful transaction.\nLogs:\n{}",
            self.logs().join("\n")
        );
        self
    }

    /// Check if the transaction succeeded
    ///
    /// # Returns
    ///
    /// true if the transaction succeeded, false otherwise
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }

    /// Get the error message if the transaction failed
    ///
    /// # Returns
    ///
    /// The error message if the transaction failed, None otherwise
    pub fn error(&self) -> Option<&String> {
        self.error.as_ref()
    }

    /// Get the transaction logs
    ///
    /// # Returns
    ///
    /// A slice of log messages
    pub fn logs(&self) -> &[String] {
        &self.inner.logs
    }

    /// Check if the logs contain a specific message
    ///
    /// # Arguments
    ///
    /// * `message` - The message to search for
    ///
    /// # Returns
    ///
    /// true if the message is found in the logs, false otherwise
    pub fn has_log(&self, message: &str) -> bool {
        self.inner.logs.iter().any(|log| log.contains(message))
    }

    /// Find a log entry containing the specified text
    ///
    /// # Arguments
    ///
    /// * `pattern` - The pattern to search for
    ///
    /// # Returns
    ///
    /// The first matching log entry, or None
    pub fn find_log(&self, pattern: &str) -> Option<&String> {
        self.inner.logs.iter().find(|log| log.contains(pattern))
    }

    /// Get the compute units consumed
    ///
    /// # Returns
    ///
    /// The number of compute units consumed
    pub fn compute_units(&self) -> u64 {
        self.inner.compute_units_consumed
    }

    /// The fee (in lamports) the SVM reported for this transaction.
    ///
    /// N.B. (LTS / anchor 0.31 branch): litesvm 0.6's `TransactionMetadata`
    /// does not carry a fee field, so this always returns 0 on this
    /// branch. The method is kept so call sites stay compatible with main.
    pub fn fee(&self) -> u64 {
        0
    }

    /// Print the transaction logs. Consumes and returns `self`; chain or
    /// bind at chain end. Wrap in [`tap`](Self::tap) if you also want to
    /// inspect a borrowed view inside the same statement.
    pub fn print_logs(self) -> Self {
        // Leading blank line separates our banner from whatever the test
        // runner just printed (the test name, prior assertions, etc.).
        println!();
        println!("=== Transaction Logs ===");
        if let Some(info) = &self.instruction {
            println!("Program: {}", info.program_id);
        }
        for log in &self.inner.logs {
            println!("{}", log);
        }
        if let Some(err) = &self.error {
            println!("Error: {}", err);
        }
        // `(this run)` reminds readers the value is exact for *this*
        // execution; per-frame CU drifts across runs because Anchor's
        // find_program_address iterates a different number of bumps for
        // different random pubkeys.
        println!("Compute Units (this run): {}", self.compute_units());
        println!("Fee: {} lamports", self.fee());
        println!("========================");
        self
    }

    /// Print the transaction logs as an annotated structured tree.
    ///
    /// Substitutes pubkey aliases (well-known programs are included by
    /// `Aliases::default()`; user-named actors via `.with(pubkey, name)`),
    /// truncates unaliased pubkeys to `<8>…<4>`, and annotates each
    /// top-level frame with `signer=X` derived from the source `Message`.
    ///
    /// Reads the alias table from the result (set via
    /// [`with_aliases`](Self::with_aliases), or by the `send_*` helpers on
    /// [`TransactionHelpers`]); falls back to [`Aliases::default`] when
    /// no table is attached.
    ///
    /// Consumes and returns `self`; chain or bind at chain end.
    ///
    /// # Colors
    ///
    /// Plain text by default. Set `ANCHOR_LITESVM_COLOR=1` in the
    /// environment to wrap status glyphs (`✓` / `✗`), the
    /// `(no cu)` / `(truncated)` markers, and error lines with ANSI
    /// SGR codes so a terminal renders them in color. `NO_COLOR=1`
    /// (<https://no-color.org>) takes precedence and forces plain output.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let aliases = Aliases::default()
    ///     .with(admin.pubkey(), "admin")
    ///     .with(bob.pubkey(), "bob");
    /// let result = svm.send_ok(ix, &[&admin], &aliases)
    ///     .print_logs_structured()
    ///     .assert_success();
    /// ```
    pub fn print_logs_structured(self) -> Self {
        print!("{}", self.logs_structured_string());
        self
    }

    /// Same content as [`print_logs_structured`](Self::print_logs_structured)
    /// but returned as a `String` instead of printed. Useful for tests and
    /// for callers that want to capture the rendered tree.
    ///
    /// Output shape:
    ///
    /// ```text
    /// ── <program>::<ix-name> ────────────────────…  (single-ix; batches omit this opener;
    ///                                                 trailing rule fills to HEADER_WIDTH)
    /// Transaction  signers=[...]
    /// <tree body>
    /// Error: ...                                    (failure only)
    /// Compute Units (this run): N
    /// Fee: N lamports
    /// Legend (M):                                   (omitted if no user aliases used)
    ///   alice          = <full base58 pubkey>
    ///   vault_program  = <full base58 pubkey>
    /// ```
    ///
    /// The footer (Compute Units / Fee / Legend) is one tight block with no
    /// internal blank line, so the visual gap between transactions (the
    /// leading blank of the next render) is strictly larger than any gap
    /// inside a single transaction. The header is closed with a fill rule
    /// so the section opener is unambiguous even when the previous
    /// transaction's legend ran long.
    ///
    /// The legend only lists aliases that actually appeared in this render
    /// (insertion-ordered, deduplicated by name). Well-known program names
    /// seeded by [`Aliases::with_well_known`] (System, Token, etc.) are
    /// filtered out so the legend stays focused on test-specific actors.
    pub fn logs_structured_string(&self) -> String {
        let default_aliases;
        let aliases: &Aliases = match &self.aliases {
            Some(a) => a,
            None => {
                default_aliases = Aliases::default();
                &default_aliases
            }
        };
        let signers = signers::extract(&self.message);
        // Read the color preference once per render so a single
        // transaction's output is internally consistent (no env-flip
        // mid-render).
        let style = style::Style::detect();
        let mut collector = tree::LegendCollector::new(aliases, &self.event_registry);
        let mut out = String::new();
        // Leading blank line separates our header from whatever the test
        // runner just printed (test name, prior assertions, etc.).
        out.push('\n');
        if let Some(info) = &self.instruction {
            let program_display = collector.render_pubkey(&info.program_id);
            // Two sources, in priority order:
            //   1. `decode_instruction` against the discriminator (works
            //      for well-known programs: SPL Token, ATA, System).
            //   2. The first `Program log: Instruction: <Name>` line in
            //      the runtime log stream. Anchor's generated dispatcher
            //      emits this on every handler entry, so we get the name
            //      for any Anchor user program without per-program
            //      registration. For a single-instruction tx that is the
            //      top-level instruction's name by construction.
            //
            // We stringify the Pubkey here to match decode_instruction's
            // existing &str signature (which the inner-frame call site also
            // hands a String). One allocation per render, not a hot path.
            let decoded = tree::decode_instruction(&info.program_id.to_string(), &info.data)
                .map(str::to_string)
                .or_else(|| {
                    self.inner.logs.iter().find_map(|log| {
                        log.strip_prefix("Program log: Instruction: ")
                            .map(str::to_string)
                    })
                });
            // Single-line header in place of the old `=== Structured
            // Transaction Logs ===` + `Instruction: <name>` pair. Batches
            // (`info == None`) skip the header entirely; the renderer's
            // `Transaction  signers=[...]` line leads naturally.
            //
            // The trailing rule fills to HEADER_WIDTH so the header reads as
            // a "section opener" rather than a label.
            let title = match decoded {
                Some(name) => format!("── {program_display}::{name} "),
                None => format!("── {program_display} "),
            };
            push_section_header(&mut out, &title);
        }
        out.push_str(&tree::render(
            &self.inner.logs,
            &self.inner.inner_instructions,
            &mut collector,
            &signers,
            style,
        ));
        if let Some(err) = &self.error {
            out.push_str(&format!("{}\n", style.red(&format!("Error: {err}"))));
        }
        out.push_str(&format!(
            "Compute Units (this run): {}\n",
            self.compute_units()
        ));
        // N.B. LTS branch: litesvm 0.6's `TransactionMetadata` has no
        // `fee` field, so `self.fee()` returns 0; the line is kept for
        // shape-compatibility with main but is informationally empty
        // here. See `fee()` for the rationale.

        let entries: Vec<(&str, Pubkey)> = collector
            .into_entries()
            .into_iter()
            .filter(|(name, _)| !aliases::is_well_known_name(name))
            .collect();
        if !entries.is_empty() {
            // No leading blank: keep CU / Legend as one tight footer
            // block. The between-transaction gap is the next render's
            // leading `\n`, which must stay strictly larger than any
            // within-footer gap or the eye loses the section boundary.
            let width = entries.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
            out.push_str(&format!("Legend ({}):\n", entries.len()));
            for (name, pk) in &entries {
                out.push_str(&format!("  {name:<width$} = {pk}\n"));
            }
        }
        out
    }

    /// Print the transaction's CPI invocation tree as a Mermaid
    /// `sequenceDiagram` block.
    ///
    /// Walks the same frame tree as
    /// [`print_logs_structured`](Self::print_logs_structured), reuses
    /// the same alias table for participant names, and emits one
    /// `participant` per first-seen program + first-seen signer, then
    /// one `->>` arrow per CPI edge in traversal order. Each arrow
    /// label is the instruction name; compute units stay in the
    /// structured tree, where measurement belongs. Failures get a
    /// `note over <target>: ✗ <error>`.
    ///
    /// Output is wrapped in a fenced ```mermaid block ready to drop
    /// into a markdown file. For the bare diagram body (e.g. for
    /// pasting into <https://mermaid.live>), call
    /// [`mermaid_string`](Self::mermaid_string) and strip the fence
    /// at the call site.
    ///
    /// Consumes and returns `self`; chain or bind at chain end.
    ///
    /// # Example
    ///
    /// ```ignore
    /// svm.send_ok(ix, &[&payer], &aliases)
    ///     .print_mermaid()
    ///     .assert_success();
    /// ```
    pub fn print_mermaid(self) -> Self {
        print!("{}", self.mermaid_string());
        self
    }

    /// Same content as [`print_mermaid`](Self::print_mermaid) but
    /// returned as a `String` instead of printed. Useful for tests
    /// and for callers that want to capture or post-process the
    /// rendered diagram.
    pub fn mermaid_string(&self) -> String {
        self.render_mermaid(mermaid::Mode::Plain)
    }

    /// Print the transaction's CPI invocation tree as a Mermaid
    /// `sequenceDiagram` block with lifelines (round-trip arrows).
    ///
    /// Variant of [`print_mermaid`](Self::print_mermaid) that emits
    /// paired `->>+` (call, activate target) and `-->>-` (return,
    /// deactivate target) arrows so the synchronous parent-stays-
    /// active-while-children-run nesting is visible. The forward
    /// arrow carries the instruction name; the return arrow carries
    /// `ok` (or `✗ <error>` for failures, rendered with Mermaid's
    /// `--x` "lost message" arrow).
    ///
    /// Roughly doubles the line count relative to
    /// [`print_mermaid`](Self::print_mermaid). Use this when the
    /// reader needs to see CPI ordering and nesting (typical for
    /// pedagogical diagrams in READMEs and walkthroughs); the plain
    /// variant is snappier for at-a-glance "what got called" checks
    /// in test output.
    ///
    /// Consumes and returns `self`; chain or bind at chain end.
    pub fn print_mermaid_with_lifelines(self) -> Self {
        print!("{}", self.mermaid_string_with_lifelines());
        self
    }

    /// Same content as
    /// [`print_mermaid_with_lifelines`](Self::print_mermaid_with_lifelines)
    /// but returned as a `String` instead of printed.
    pub fn mermaid_string_with_lifelines(&self) -> String {
        self.render_mermaid(mermaid::Mode::Lifelines)
    }

    /// Print the tx's structured CPI tree and a Mermaid sequence
    /// diagram with lifelines, both wrapped in README-ready markdown
    /// delimiters. The structured tree goes inside a ```console
    /// fence; the diagram goes inside a `<details><summary>Lifelines
    /// diagram` `</summary>` element so the visible README stays
    /// scannable.
    ///
    /// Equivalent to calling
    /// [`print_logs_structured`](Self::print_logs_structured) then
    /// [`print_mermaid_with_lifelines`](Self::print_mermaid_with_lifelines)
    /// with the right `` ```console `` / `` <details> `` lines emitted
    /// between them by [`tap`](Self::tap). The point: `cargo test
    /// --nocapture` produces output that drops straight into a
    /// markdown file (README, issue, post-mortem) with no
    /// reformatting.
    ///
    /// Consumes and returns `self`; chain or bind at chain end.
    ///
    /// # Example
    ///
    /// ```ignore
    /// ctx.tx(&[&user.signer])
    ///     .build(bundle, ix_args)
    ///     .send_ok()
    ///     .print_markdown_pair();
    /// ```
    pub fn print_markdown_pair(self) -> Self {
        self.tap(|_| println!("```console"))
            .print_logs_structured()
            .tap(|_| {
                println!("```");
                println!();
                println!("<details><summary>Lifelines diagram</summary>");
                println!();
            })
            .print_mermaid_with_lifelines()
            .tap(|_| {
                println!("</details>");
                println!();
            })
    }

    /// Shared body for the four `*_mermaid*` methods: resolve the
    /// alias table (falling back to `Aliases::default()`), extract
    /// signers from the message, detect the `ANCHOR_LITESVM_MERMAID_LOGS`
    /// env var (events always render; logs opt-in via the env var),
    /// and dispatch to the mermaid renderer at the requested mode.
    fn render_mermaid(&self, mode: mermaid::Mode) -> String {
        let default_aliases;
        let aliases: &Aliases = match &self.aliases {
            Some(a) => a,
            None => {
                default_aliases = Aliases::default();
                &default_aliases
            }
        };
        let signers = signers::extract(&self.message);
        let include_logs = mermaid::detect_include_logs();
        let mut collector = tree::LegendCollector::new(aliases, &self.event_registry);
        mermaid::render(
            &self.inner.logs,
            &self.inner.inner_instructions,
            &mut collector,
            &signers,
            mode,
            include_logs,
        )
    }

    /// Get the inner TransactionMetadata for direct access
    pub fn inner(&self) -> &TransactionMetadata {
        &self.inner
    }

    /// Execute a closure for side effects on a borrowed view, then return
    /// the owned `self` for further chaining.
    ///
    /// This is the bridge between read-only methods (`compute_units`,
    /// `is_success`, `error`, `logs`, etc., which take `&self`) and the
    /// consuming chain methods (`assert_*`, `print_logs*`). The closure
    /// receives `&Self`, so it can call any number of read-only methods
    /// inline; `tap` then hands ownership back to the chain.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = svm.send_ok(ix, &[&payer], &aliases)
    ///     .tap(|r| println!("CU used: {}", r.compute_units()))
    ///     .assert_success()
    ///     .print_logs_structured();
    /// ```
    pub fn tap<F>(self, f: F) -> Self
    where
        F: FnOnce(&Self),
    {
        f(&self);
        self
    }

    /// Assert that the transaction failed. Panics if it succeeded.
    /// Consumes and returns `self`; chain or bind at chain end.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = svm.send_instruction(ix, &[&payer])?.assert_failure();
    /// ```
    pub fn assert_failure(self) -> Self {
        assert!(
            self.error.is_some(),
            "Expected transaction to fail, but it succeeded.\nLogs:\n{}",
            self.logs().join("\n")
        );
        self
    }

    /// Assert that the transaction failed AND a caller-supplied
    /// predicate holds on the result. Panics with context on either
    /// failure. Consumes and returns `self`.
    ///
    /// Mirrors [`assert_success_with`](Self::assert_success_with) for
    /// the negative-path case: useful for tests that want to verify
    /// both the failure and a specific signal about how/why it failed.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let r = svm.send_instruction(ix, &[&payer])?
    ///     .assert_failure_with(|r| r.has_log("EscrowExpired"));
    /// ```
    pub fn assert_failure_with<F>(self, predicate: F) -> Self
    where
        F: FnOnce(&Self) -> bool,
    {
        assert!(
            self.error.is_some(),
            "Expected transaction to fail, but it succeeded.\nLogs:\n{}",
            self.logs().join("\n")
        );
        assert!(
            predicate(&self),
            "Predicate failed on failed transaction.\nError: {:?}\nLogs:\n{}",
            self.error,
            self.logs().join("\n")
        );
        self
    }

    /// Assert that the transaction failed and the given substring
    /// appears in the runtime logs *or* the error field. Panics if it
    /// succeeded or the substring wasn't found in either source.
    /// Consumes and returns `self`.
    ///
    /// The lenient logs-or-error search covers Anchor error names
    /// (which surface as `Error code: <Name>` in logs but rarely in the
    /// error field) and runtime errors (which surface in the error
    /// field, e.g. `"InsufficientFundsForRent"`) with one method, so
    /// the caller doesn't have to remember which source carries which
    /// kind of error.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = svm.send_instruction(ix, &[&payer])?
    ///     .assert_error("EscrowExpired");  // Anchor error name, in logs
    ///
    /// let result = svm.send_instruction(ix, &[&payer])?
    ///     .assert_error("InsufficientFundsForRent");  // runtime error, in the error field
    /// ```
    pub fn assert_error(self, expected_error: &str) -> Self {
        // assert_failure consumes; rebind so we can keep inspecting.
        let this = self.assert_failure();

        let found_in_logs = this.logs().iter().any(|log| log.contains(expected_error));
        let found_in_error = this
            .error
            .as_ref()
            .map(|e| e.contains(expected_error))
            .unwrap_or(false);

        assert!(
            found_in_logs || found_in_error,
            "Expected error containing '{}' not found in transaction logs or error field.\nError: {:?}\nLogs:\n{}",
            expected_error,
            this.error,
            this.logs().join("\n")
        );
        this
    }

    /// Assert that the transaction failed with a specific Anchor custom
    /// error code (e.g. `6000` for the first error in an Anchor
    /// `#[error_code]` enum). Formats the code as the
    /// `"custom program error: 0x<hex>"` substring runtime emits, then
    /// delegates to [`assert_error`](Self::assert_error). Consumes and
    /// returns `self`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = svm.send_instruction(ix, &[&payer])?
    ///     .assert_error_code(6000);
    /// ```
    pub fn assert_error_code(self, error_code: u32) -> Self {
        let error_code_str = format!("custom program error: 0x{:x}", error_code);
        self.assert_error(&error_code_str)
    }
}

impl fmt::Debug for TransactionResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransactionResult")
            .field(
                "program_id",
                &self.instruction.as_ref().map(|i| i.program_id),
            )
            .field("success", &self.is_success())
            .field("error", &self.error())
            .field("compute_units", &self.compute_units())
            .field("log_count", &self.logs().len())
            .finish()
    }
}

/// Transaction helper methods for LiteSVM
pub trait TransactionHelpers {
    /// Send a single instruction and return a wrapped result
    ///
    /// # Example
    /// ```no_run
    /// # use litesvm_utils::TransactionHelpers;
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_sdk::signer::keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let signer = Keypair::new();
    /// let result = svm.send_instruction(ix, &[&signer]).unwrap().assert_success();
    /// ```
    fn send_instruction(
        &mut self,
        instruction: Instruction,
        signers: &[&Keypair],
    ) -> Result<TransactionResult, TransactionError>;

    /// Send multiple instructions in a single transaction
    ///
    /// # Example
    /// ```no_run
    /// # use litesvm_utils::TransactionHelpers;
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_sdk::signer::keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix1 = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let ix2 = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let signer = Keypair::new();
    /// let result = svm.send_instructions(&[ix1, ix2], &[&signer]).unwrap().assert_success();
    /// ```
    fn send_instructions(
        &mut self,
        instructions: &[Instruction],
        signers: &[&Keypair],
    ) -> Result<TransactionResult, TransactionError>;

    /// Send a transaction and return a wrapped result
    ///
    /// # Example
    /// ```no_run
    /// # use litesvm_utils::TransactionHelpers;
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_sdk::signer::keypair::Keypair;
    /// # use solana_sdk::signer::Signer;
    /// # use solana_sdk::transaction::Transaction;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let signer = Keypair::new();
    /// let tx = Transaction::new_signed_with_payer(
    ///     &[ix],
    ///     Some(&signer.pubkey()),
    ///     &[&signer],
    ///     svm.latest_blockhash(),
    /// );
    /// let result = svm.send_transaction_result(tx).unwrap().assert_success();
    /// ```
    fn send_transaction_result(
        &mut self,
        transaction: Transaction,
    ) -> Result<TransactionResult, TransactionError>;

    /// Send an ix expected to succeed: unwraps the build-time `Result`
    /// (panics on build errors, e.g. no signers) and asserts the
    /// transaction itself didn't carry a program error. Returns the
    /// wrapped result with `aliases` stashed on it, so callers can chain
    /// `.print_logs_structured()` or inspect compute units.
    ///
    /// The `aliases` map is used for the failure-path structured CPI tree
    /// print, so test authors who built an alias map in setup see it
    /// applied to the diagnostic. Pass `&Aliases::default()` if you just
    /// want the well-known program names.
    ///
    /// Use this in the happy path of a test. When failure is expected
    /// with a specific Anchor error name, use
    /// [`send_err_named`](Self::send_err_named); when failure is
    /// expected without a name to assert (the outcome alone is the
    /// contract), use [`send_err`](Self::send_err); otherwise drop down
    /// to [`send_instruction`](Self::send_instruction).
    ///
    /// # Example
    ///
    /// ```ignore
    /// svm.send_ok(ix, &[&maker], &aliases).print_logs_structured();
    /// ```
    fn send_ok(
        &mut self,
        instruction: Instruction,
        signers: &[&Keypair],
        aliases: &Aliases,
    ) -> TransactionResult {
        let mut result = self
            .send_instruction(instruction, signers)
            .expect("send_ok: transaction build failed")
            .with_aliases(aliases.clone());
        if !result.is_success() {
            // The underlying assert_success panic includes only flat logs.
            // Print the structured CPI tree first so the test author sees
            // which frame the error came from, then let assert_success
            // raise the panic with its embedded log dump as normal.
            eprintln!("\nsend_ok: transaction failed, structured CPI tree:");
            result = result.print_logs_structured();
        }
        result.assert_success()
    }

    /// Send an ix expected to fail without asserting a specific error
    /// name. Mirror of [`send_ok`](Self::send_ok) for the negative path
    /// when the outcome alone is the contract (e.g. an authorization
    /// check, a generic constraint trip) and pinning to a specific
    /// `ErrorCode::Foo` would over-constrain the test.
    ///
    /// The `aliases` map is used for the failure-path structured CPI
    /// tree print (i.e. when the tx *unexpectedly succeeded*, which is
    /// the assertion-failure mode here). Returns the wrapped result so
    /// callers can chain further inspection.
    ///
    /// Panics if the transaction succeeded.
    ///
    /// # Example
    ///
    /// ```ignore
    /// svm.send_err(ix, &[&attacker], &aliases)
    ///     .print_logs_structured();
    /// ```
    fn send_err(
        &mut self,
        instruction: Instruction,
        signers: &[&Keypair],
        aliases: &Aliases,
    ) -> TransactionResult {
        let mut result = self
            .send_instruction(instruction, signers)
            .expect("send_err: transaction build failed")
            .with_aliases(aliases.clone());
        if result.is_success() {
            eprintln!("\nsend_err: tx unexpectedly succeeded, structured CPI tree:");
            result = result.print_logs_structured();
        }
        result.assert_failure()
    }

    /// Send an ix expected to fail with a specific error name
    /// (e.g. `"EscrowExpired"`, `"ConstraintHasOne"`,
    /// `"InsufficientFundsForRent"`). The match is the same substring
    /// check that [`TransactionResult::assert_error`] performs against
    /// logs and the error field, just bundled with the send + unwrap.
    /// Returns the wrapped result so callers can chain further
    /// inspection (compute units, log scrapes, custom prints),
    /// mirroring [`send_ok`](Self::send_ok)'s shape.
    ///
    /// The `aliases` map is used for the failure-path structured CPI tree
    /// print (same as [`send_ok`](Self::send_ok)). Pass
    /// `&Aliases::default()` if you just want the well-known program names.
    ///
    /// Panics if the transaction succeeded or failed with a different
    /// error.
    ///
    /// # Example
    ///
    /// ```ignore
    /// svm.send_err_named(ix, &[&taker], &aliases, "EscrowExpired")
    ///     .tap(|r| assert!(r.compute_units() < 100_000));
    /// ```
    fn send_err_named(
        &mut self,
        instruction: Instruction,
        signers: &[&Keypair],
        aliases: &Aliases,
        error_name: &str,
    ) -> TransactionResult {
        let mut result = self
            .send_instruction(instruction, signers)
            .expect("send_err_named: transaction build failed")
            .with_aliases(aliases.clone());
        // If we're about to fail the assertion (tx succeeded, or tx failed
        // with a different error), print the structured tree first so the
        // CPI shape is visible above the eventual panic dump.
        let error_matches = !result.is_success()
            && (result.logs().iter().any(|log| log.contains(error_name))
                || result
                    .error()
                    .map(|e| e.contains(error_name))
                    .unwrap_or(false));
        if !error_matches {
            eprintln!("\nsend_err_named: assertion will fail, structured CPI tree:");
            result = result.print_logs_structured();
        }
        result.assert_error(error_name)
    }
}

impl TransactionHelpers for LiteSVM {
    fn send_instruction(
        &mut self,
        instruction: Instruction,
        signers: &[&Keypair],
    ) -> Result<TransactionResult, TransactionError> {
        if signers.is_empty() {
            return Err(TransactionError::BuildError(
                "No signers provided".to_string(),
            ));
        }

        // We know this transaction wraps exactly one instruction, so we
        // capture its program ID + data for the structured-logs header.
        // Stashed before the `Transaction::new_signed_with_payer` call
        // below because that call consumes `instruction`.
        let info = InstructionInfo::from_instruction(&instruction);
        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&signers[0].pubkey()),
            signers,
            self.latest_blockhash(),
        );
        let message = tx.message.clone();
        match self.send_transaction(tx) {
            Ok(result) => Ok(TransactionResult::new(result, Some(info), message)),
            Err(failed) => Ok(TransactionResult::new_failed(
                format!("{:?}", failed.err),
                failed.meta,
                Some(info),
                message,
            )),
        }
    }

    fn send_instructions(
        &mut self,
        instructions: &[Instruction],
        signers: &[&Keypair],
    ) -> Result<TransactionResult, TransactionError> {
        if signers.is_empty() {
            return Err(TransactionError::BuildError(
                "No signers provided".to_string(),
            ));
        }

        let tx = Transaction::new_signed_with_payer(
            instructions,
            Some(&signers[0].pubkey()),
            signers,
            self.latest_blockhash(),
        );

        self.send_transaction_result(tx)
    }

    fn send_transaction_result(
        &mut self,
        transaction: Transaction,
    ) -> Result<TransactionResult, TransactionError> {
        // Clone the message before send_transaction consumes the tx; the
        // structured-logs printer needs it to derive signer annotations.
        let message = transaction.message.clone();
        match self.send_transaction(transaction) {
            Ok(result) => Ok(TransactionResult::new(result, None, message)),
            Err(failed) => Ok(TransactionResult::new_failed(
                format!("{:?}", failed.err),
                failed.meta,
                None,
                message,
            )),
        }
    }
}

#[cfg(test)]
mod tests;
