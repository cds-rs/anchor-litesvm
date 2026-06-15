//! Transaction execution and result handling utilities
//!
//! This module provides convenient wrappers for executing transactions
//! and handling their results in tests.

mod account_index;
mod aliases;
mod authority;
mod authority_story;
mod error_names;
mod events;
mod instruction_names;
mod mermaid;
mod model;
mod ownership;
mod renderer;
mod signers;
mod style;
mod trace;
mod tree;

pub use account_index::{AccountIndex, AccountNode, AuthorityClass};
pub use aliases::Aliases;
pub use authority_story::AuthorityStory;
pub use error_names::ErrorNames;
pub use events::{EventInfo, EventRegistry};
pub use instruction_names::InstructionNames;
pub use trace::{InstructionTrace, TraceHandle, TraceRecorder, TracedAccount, TracedInstruction};

use renderer::Renderer;

use litesvm::types::TransactionMetadata;
use litesvm::LiteSVM;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::fmt;
use thiserror::Error;

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
/// ```no_run
/// # use litesvm_utils::TransactionHelpers;
/// # use litesvm::LiteSVM;
/// # use solana_program::instruction::Instruction;
/// # use solana_keypair::Keypair;
/// # let mut svm = LiteSVM::new();
/// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
/// # let signer = Keypair::new();
/// let result = svm.send_instruction(ix, &[&signer]).unwrap()
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
    /// The privilege-bearing instruction trace recorded for this send: the
    /// runtime's per-frame signer/writable/owner facts that the CPI tree
    /// (logs) structurally can't carry. Set via
    /// [`with_instruction_trace`](Self::with_instruction_trace) and read by the
    /// authority / account-index views. `None` until attached (trace-blind).
    instruction_trace: Option<InstructionTrace>,
    /// Discriminator-to-name table for programs without an IDL (Pinocchio and
    /// other hand-rolled programs), so their instructions render by name
    /// instead of by program alias. Empty by default (the Anchor path needs
    /// nothing here; names come from the log line). Set via
    /// [`with_instruction_names`](Self::with_instruction_names); consulted as
    /// the last resort inside [`model`](Self::model). See [`InstructionNames`].
    instruction_names: InstructionNames,
    /// Custom-error-code-to-name table, the failure-path twin of
    /// `instruction_names`: lets a Pinocchio `ProgramError::Custom(n)` render
    /// and match by name (`InvalidAmount`) instead of `0x<n>`. Empty by default
    /// (Anchor failures carry their own name in the logs). Set via
    /// [`with_error_names`](Self::with_error_names); consulted inside
    /// [`model`](Self::model) when a frame fails. See [`ErrorNames`].
    error_names: ErrorNames,
    /// Decoders for registered Anchor events, so a `Program data:` payload
    /// renders by name and fields (a mermaid `note`, an indented tree line)
    /// instead of raw base64. Empty by default (events stay raw until a type is
    /// registered via `AnchorContext::register_event`). Set via
    /// [`with_event_registry`](Self::with_event_registry); carried onto the
    /// model in [`model`](Self::model). See [`EventRegistry`].
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
            instruction_trace: None,
            instruction_names: InstructionNames::new(),
            error_names: ErrorNames::new(),
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
            instruction_trace: None,
            instruction_names: InstructionNames::new(),
            error_names: ErrorNames::new(),
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

    /// Attach the privilege-bearing instruction trace for this send (drained
    /// from the context's [`TraceRecorder`]). Drives the authority and
    /// account-index views; chainable, and `None` leaves them trace-blind.
    pub fn with_instruction_trace(mut self, trace: Option<InstructionTrace>) -> Self {
        self.instruction_trace = trace;
        self
    }

    /// The recorded instruction trace, if one was attached via
    /// [`with_instruction_trace`](Self::with_instruction_trace).
    pub fn instruction_trace(&self) -> Option<&InstructionTrace> {
        self.instruction_trace.as_ref()
    }

    /// Attach a discriminator-to-name table so instructions from a program
    /// without an IDL render by name instead of by program alias. Cloned in
    /// (cheap); chainable. The `AnchorContext` send helpers attach the
    /// context's table automatically, so most callers never touch this
    /// directly. See [`InstructionNames`].
    pub fn with_instruction_names(mut self, names: InstructionNames) -> Self {
        self.instruction_names = names;
        self
    }

    /// Attach a custom-error-code-to-name table so a `ProgramError::Custom`
    /// from a program without an IDL renders and matches by name. Cloned in
    /// (cheap); chainable. The `AnchorContext` send helpers attach the
    /// context's table automatically. See [`ErrorNames`].
    pub fn with_error_names(mut self, errors: ErrorNames) -> Self {
        self.error_names = errors;
        self
    }

    /// Attach a table of event decoders so a `Program data:` payload renders by
    /// name and fields instead of raw base64. Cloned in (cheap: the registry's
    /// decoders are `Arc`d); chainable. The `AnchorContext` send helpers attach
    /// the context's registry automatically; populate it with
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
    /// ```no_run
    /// # use litesvm_utils::TransactionHelpers;
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let payer = Keypair::new();
    /// let result = svm.send_instruction(ix, &[&payer]).unwrap().assert_success();
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
    /// ```no_run
    /// # use litesvm_utils::{Aliases, TransactionHelpers};
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let payer = Keypair::new();
    /// # let aliases = Aliases::default();
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

    /// The fee (in lamports) the SVM reported for this transaction. Note
    /// that fees are *not* refunded on failure, so a failed tx still
    /// reports the lamports the fee payer was charged. The value is just
    /// the underlying `TransactionMetadata.fee` lifted to a method for
    /// consistency with [`compute_units`](Self::compute_units).
    pub fn fee(&self) -> u64 {
        self.inner.fee
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
    /// ```no_run
    /// # use litesvm_utils::{Aliases, Signer, TransactionHelpers};
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let admin = Keypair::new();
    /// # let bob = Keypair::new();
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
        tree::TreeRenderer {
            style: style::Style::detect(),
        }
        .render(&self.model(), aliases)
    }

    /// The fully-resolved CPI model for this send: the `cpi_tree` structure
    /// with per-frame outcome, instruction names resolved (built-in decoder ->
    /// log line -> the registered [`InstructionNames`] table), and per-frame
    /// account authority (signer / writable / owner) enriched from the trace
    /// when one is attached.
    ///
    /// This is *the* model every renderer on this result consumes: the tree,
    /// the mermaid variants, the authority graph/sequence, and (after a
    /// `fill_owners` pass) the ownership graph all build from this one value, so
    /// name resolution and trace enrichment happen in exactly one place. The
    /// authority story holds the analogous per-submit value across a test.
    pub(in crate::transaction) fn model(&self) -> model::CpiModel {
        let signers = signers::extract(&self.message);
        let mut model = model::build(
            self.instruction.as_ref(),
            &self.inner.logs,
            &self.inner.inner_instructions,
            &self.message,
            &signers,
            self.error.clone(),
            self.compute_units(),
            self.fee(),
            model::Vocab {
                instructions: &self.instruction_names,
                errors: &self.error_names,
                events: &self.event_registry,
            },
        );
        if let Some(trace) = &self.instruction_trace {
            model::fill_from_trace(&mut model, trace);
        }
        model
    }

    /// Print the transaction's CPI invocation tree as a Mermaid
    /// `sequenceDiagram` block.
    ///
    /// Walks the same frame tree as
    /// [`print_logs_structured`](Self::print_logs_structured), reuses
    /// the same alias table for participant names, and emits one
    /// `participant` per first-seen program + first-seen signer, then
    /// one `->>` arrow per CPI edge in traversal order. Each arrow
    /// label is `instruction_name (Ncu)` (CU omitted when the frame
    /// has none). Failures get a `note over <target>: ✗ <error>`.
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
    /// ```no_run
    /// # use litesvm_utils::{Aliases, TransactionHelpers};
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let payer = Keypair::new();
    /// # let aliases = Aliases::default();
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
    /// `ok (Ncu)` (or `✗ <error> (Ncu)` for failures, rendered with
    /// Mermaid's `--x` "lost message" arrow).
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

    /// Print an authority graph: a Mermaid `flowchart` of who signs what and
    /// which accounts each program writes, built from the per-frame account
    /// roles. See the `authority` module for the node/edge semantics.
    ///
    /// Consumes and returns `self`; chain or bind at chain end.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use litesvm_utils::{Aliases, TransactionHelpers};
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let payer = Keypair::new();
    /// # let aliases = Aliases::default();
    /// svm.send_ok(ix, &[&payer], &aliases)
    ///     .print_authority_graph() // signer --signs--> program --writes--> account
    ///     .assert_success();
    /// ```
    pub fn print_authority_graph(self) -> Self {
        print!("{}", self.authority_graph_string());
        self
    }

    /// Same content as
    /// [`print_authority_graph`](Self::print_authority_graph) but returned as
    /// a `String`.
    pub fn authority_graph_string(&self) -> String {
        let default_aliases;
        let aliases: &Aliases = match &self.aliases {
            Some(a) => a,
            None => {
                default_aliases = Aliases::default();
                &default_aliases
            }
        };
        // `model()` already joins the trace's per-frame privilege facts: the
        // message header only knows top-level roles, so a CPI's invoke_signed
        // PDA would be invisible without it.
        authority::AuthorityGraph.render(&self.model(), aliases)
    }

    /// Print the per-submit **authority sequence**: a Mermaid `sequenceDiagram`
    /// of who was authorized to touch what, the transaction's human signers and
    /// each `invoke_signed` PDA as lanes, every privileged write as an arrow to
    /// its target (`✓` settled, `✗ <error>` rejected). The per-submit twin of the
    /// per-test [`Report::authority`](crate::Report); empty when the transaction
    /// has no authority flow to show.
    ///
    /// Consumes and returns `self`; chain or bind at chain end.
    pub fn print_authority_mermaid(self) -> Self {
        print!("{}", self.authority_mermaid_string());
        self
    }

    /// Same content as
    /// [`print_authority_mermaid`](Self::print_authority_mermaid) but returned
    /// as a `String`.
    pub fn authority_mermaid_string(&self) -> String {
        let default_aliases;
        let aliases: &Aliases = match &self.aliases {
            Some(a) => a,
            None => {
                default_aliases = Aliases::default();
                &default_aliases
            }
        };
        // `model()` joins the trace's privilege facts (same as the authority
        // graph), so an invoke_signed PDA lands in its own lane; `signers` is
        // still needed separately for the human-signer lanes.
        let signers = signers::extract(&self.message);
        authority_story::render(&self.model(), &signers.tx_signers, aliases)
    }

    /// Print an ownership graph: a Mermaid `flowchart` of which program owns
    /// each account the tx wrote (`owner --owns--> account`).
    ///
    /// Needs the `svm` because an account's owner is post-execution state, not
    /// carried by the message or logs: this fills `AccountRef.owner` with one
    /// `svm.get_account` lookup per account, then renders. (That second lookup
    /// is the stopgap the model's `fill_owners` documents; a litesvm-metadata
    /// win removes it.)
    ///
    /// Consumes and returns `self`; chain or bind at chain end.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use litesvm_utils::{Aliases, TransactionHelpers};
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let payer = Keypair::new();
    /// # let aliases = Aliases::default();
    /// // `&svm` is free again here: send_ok's `&mut` borrow ended when it
    /// // returned the owned result.
    /// let result = svm.send_ok(ix, &[&payer], &aliases);
    /// result.print_ownership_graph(&svm); // owner-program --owns--> account
    /// ```
    pub fn print_ownership_graph(self, svm: &LiteSVM) -> Self {
        print!("{}", self.ownership_graph_string(svm));
        self
    }

    /// Same content as
    /// [`print_ownership_graph`](Self::print_ownership_graph) but returned as a
    /// `String`.
    pub fn ownership_graph_string(&self, svm: &LiteSVM) -> String {
        let default_aliases;
        let aliases: &Aliases = match &self.aliases {
            Some(a) => a,
            None => {
                default_aliases = Aliases::default();
                &default_aliases
            }
        };
        let mut model = self.model();
        model::fill_owners(&mut model, |pk| svm.get_account(pk).map(|a| a.owner));
        ownership::OwnershipGraph.render(&model, aliases)
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
        mermaid::MermaidRenderer {
            mode,
            include_logs: mermaid::detect_include_logs(),
        }
        .render(&self.model(), aliases)
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
    /// ```no_run
    /// # use litesvm_utils::{Aliases, TransactionHelpers};
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let payer = Keypair::new();
    /// # let aliases = Aliases::default();
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
    /// ```no_run
    /// # use litesvm_utils::TransactionHelpers;
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let payer = Keypair::new();
    /// let result = svm.send_instruction(ix, &[&payer]).unwrap().assert_failure();
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
    /// ```no_run
    /// # use litesvm_utils::TransactionHelpers;
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let payer = Keypair::new();
    /// let r = svm.send_instruction(ix, &[&payer]).unwrap()
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
        // Also match the model's resolved failure messages, which carry any
        // registered error name (so `assert_error("InvalidAmount")` matches a
        // Pinocchio `ProgramError::Custom(7)` once its code is registered, even
        // though only `0x7` appears in the raw logs and error field).
        let found_in_resolved = this
            .model()
            .failure_messages()
            .iter()
            .any(|m| m.contains(expected_error));

        assert!(
            found_in_logs || found_in_error || found_in_resolved,
            "Expected error containing '{}' not found in transaction logs, error field, or resolved failure name.\nError: {:?}\nLogs:\n{}",
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
    /// ```no_run
    /// # use litesvm_utils::TransactionHelpers;
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let payer = Keypair::new();
    /// let result = svm.send_instruction(ix, &[&payer]).unwrap()
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
    /// # use solana_keypair::Keypair;
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
    /// # use solana_keypair::Keypair;
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
    /// # use solana_keypair::Keypair;
    /// # use solana_signer::Signer;
    /// # use solana_transaction::Transaction;
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
    /// ```no_run
    /// # use litesvm_utils::{Aliases, TransactionHelpers};
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let maker = Keypair::new();
    /// # let aliases = Aliases::default();
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
    /// ```no_run
    /// # use litesvm_utils::{Aliases, TransactionHelpers};
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let attacker = Keypair::new();
    /// # let aliases = Aliases::default();
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
    /// ```no_run
    /// # use litesvm_utils::{Aliases, TransactionHelpers};
    /// # use litesvm::LiteSVM;
    /// # use solana_program::instruction::Instruction;
    /// # use solana_keypair::Keypair;
    /// # let mut svm = LiteSVM::new();
    /// # let ix = Instruction::new_with_bytes(solana_program::pubkey::Pubkey::new_unique(), &[], vec![]);
    /// # let taker = Keypair::new();
    /// # let aliases = Aliases::default();
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
        // Fresh by default: every helper-mediated send is its own transaction.
        // Without this, an identical instruction resent under the same
        // blockhash is the same signature, and litesvm correctly rejects the
        // repeat as already processed — a chain rule no test scenario means
        // to invoke (and one that is litesvm's own to test, via raw litesvm).
        self.expire_blockhash();
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

        // Fresh by default; see send_instruction for the full rationale.
        self.expire_blockhash();
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
