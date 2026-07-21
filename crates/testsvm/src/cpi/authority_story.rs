//! Render the authority view of executed transactions as Mermaid
//! `sequenceDiagram`s: who signed, which PDAs the program signed as
//! (`invoke_signed`), and which accounts those privileges actually wrote.
//!
//! This is the view the CPI tree (mermaid.rs) structurally cannot draw. The
//! CPI tree shows which program called which; this shows which *authority*
//! carried each write. Two transactions can be identical in the CPI view and
//! completely different here: the authority-flow proposal's
//! rejected-vs-accepted restake contrast is exactly that pair.
//!
//! Two granularities, per the proposal:
//!
//! - **Per submit** ([`render`], surfaced as
//!   `TransactionResult::authority_mermaid_string()`): one diagram per
//!   transaction.
//! - **Per test** ([`AuthorityStory`], surfaced as `Report::authority()` via
//!   [`ToBlock`]): sections accumulate per submitted transaction and emit
//!   as one diagram with unified lanes, so the same signer occupies the same
//!   lane in every section.
//!
//! ## Lane and arrow rules
//!
//! Lane layout (participant order): transaction signers first, then
//! program-signed PDAs, then plain writable targets. An arrow's origin lane
//! is the authority that carried the write, chosen per frame by priority:
//!
//! 1. A frame signer that is NOT a transaction-level signer was signed for
//!    by the calling program (`invoke_signed`). It wins arrow origin.
//! 2. A frame signer that IS a transaction-level signer is that signer's
//!    transaction signature extended into the CPI; it is the origin only
//!    when no program-signed signer is present in the frame.
//!
//! The priority exists because most CPIs carry both kinds (the payer's
//! extended signature rides along to fund rent), and the deliberate act of
//! the program signing is the authority story. This rule is what makes the
//! restake contrast render correctly: the landed fix's `UpdatePlugin` has
//! the owner (extended) and the PDA (program-signed) as signers, and the
//! arrow must leave the PDA's lane.
//!
//! ## What counts as a write target
//!
//! A frame draws a write-arrow to one of its accounts iff:
//!
//! - the account is `is_writable` (write access was requested), and
//! - **the frame's program owns the account** (`account.owner ==
//!   frame.program_id`), and
//! - the account is not itself a signer of the frame.
//!
//! The ownership clause is the one that matters, and it is the runtime's own
//! mutation rule: `is_writable` is only an access *request*; the account is
//! actually mutated by its owner program. So a top-level Anchor frame that
//! lists a dozen writable accounts (its whole `#[derive(Accounts)]`) draws
//! arrows only to the few it *owns* (its own PDAs); the token accounts and
//! system accounts it merely passes through get their arrows under the CPIs
//! into the programs that own them (SPL Token, System). This de-dupes
//! naturally: each account is written by exactly the one frame whose program
//! owns it, never by a parent that only requested access. It also removes a
//! misattribution the access-only view had: a top-level `Owner -> vault`
//! arrow that claimed the human signed the write, when in fact the program
//! signed for it via `invoke_signed` one frame down.
//!
//! The signer-exclusion is a smaller refinement: for a System transfer the
//! debited account is itself the signer (a self-loop carries no information),
//! whereas for a Token transfer the authority is a separate account from the
//! debited token account, so that arrow does carry information and survives.
//! The exclusion drops the former and keeps the latter without a special
//! case.

use {
    super::{
        aliases::Aliases,
        mermaid::{mermaid_id, INDENT},
        model::{from_transaction, CpiModel, Outcome},
    },
    crate::{model::Transaction, report::ToBlock},
    frood_guide::Block,
    solana_pubkey::Pubkey,
    std::fmt::Write,
};

/// A participant's role: decides its lane group and its label suffix.
/// Ordering here is lane ordering (signers left, targets right).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    TxSigner,
    ProgramSigned,
    Target,
}

impl Role {
    fn suffix(self) -> &'static str {
        match self {
            Role::TxSigner => "tx signer",
            Role::ProgramSigned => "program-signed",
            Role::Target => "writable",
        }
    }
}

struct Arrow {
    source: Pubkey,
    target: Pubkey,
    label: String,
    failed: bool,
}

/// Per-section background tint, emitted as `rect rgba(...)`. Colour appears by
/// EXCEPTION, not by rule. A routine success gets NO band: most of a diagram is
/// routine, so tinting it is noise that drowns the one step that matters. Only
/// two things earn a band: a reverted section (translucent red, the failure)
/// and an author-spotlighted step (translucent blue, noteworthy but not broken,
/// e.g. a state transition or a silent-success scandal). `rgba` (not `rgb`) is
/// load-bearing: a translucent overlay tints the band in BOTH light and dark
/// Mermaid themes while the arrows and labels keep full-opacity contrast on top.
/// An opaque pale fill washed light-on-dark text out in dark-theme
/// presentations. `(r, g, b, alpha)`.
const SECTION_FAIL_RGBA: (u8, u8, u8, f32) = (231, 76, 60, 0.25);
const SECTION_SPOTLIGHT_RGBA: (u8, u8, u8, f32) = (52, 152, 219, 0.18);

/// Build the arrows for one transaction's model. See the module docs for the
/// origin-priority and ownership rules. The per-frame outcome (`✓` / `✗`) is
/// read from the model's `cpi_tree`-derived `outcome`, so a frame that fails
/// after a successful CPI is marked at the frame that actually failed, not by
/// position in the trace.
fn build_arrows(model: &CpiModel, tx_signers: &[Pubkey], aliases: &Aliases) -> Vec<Arrow> {
    let mut arrows: Vec<Arrow> = Vec::new();
    for frame in model.frames() {
        // The origin: program-signed signer first, extended tx signature
        // second.
        let program_signed_origin = frame
            .accounts
            .iter()
            .find(|a| a.is_signer && !tx_signers.contains(&a.pubkey));
        let origin = program_signed_origin.or_else(|| frame.accounts.iter().find(|a| a.is_signer));
        let Some(origin) = origin else {
            continue;
        };

        let program_label = aliases.label(&frame.program);
        let ix_label = match &frame.instruction_name {
            Some(n) => format!("{program_label}::{n}"),
            None => program_label,
        };

        let frame_failed = matches!(frame.outcome, Outcome::Failed { .. });
        let glyph = if frame_failed { "✗" } else { "✓" };
        let label = format!("{ix_label} {glyph}");

        // Write targets: the accounts this frame's program OWNS and has write
        // access to (see module docs). The ownership clause is what cuts the
        // top-level Anchor frame's access-only noise down to its real direct
        // writes.
        let targets = frame
            .accounts
            .iter()
            .filter(|a| a.is_writable && !a.is_signer && a.owner == Some(frame.program))
            .map(|a| a.pubkey);

        for target in targets {
            arrows.push(Arrow {
                source: origin.pubkey,
                target,
                label: label.clone(),
                failed: frame_failed,
            });
        }
    }

    arrows
}

/// Classify every pubkey the arrows touch into lanes. Role priority for
/// classification: tx signer > program-signed > target (an account keeps its
/// strongest role even if it also appears as a target of some other frame's
/// write). Returned in lane order: role groups in [`Role`] order,
/// first-appearance order within a group.
fn classify_participants(arrows: &[Arrow], tx_signers: &[Pubkey]) -> Vec<(Pubkey, Role)> {
    let mut participants: Vec<(Pubkey, Role)> = Vec::new();
    let mut classify = |pk: Pubkey, is_source: bool| {
        let role = if tx_signers.contains(&pk) {
            Role::TxSigner
        } else if is_source {
            Role::ProgramSigned
        } else {
            Role::Target
        };
        match participants.iter_mut().find(|(p, _)| *p == pk) {
            Some((_, existing)) => {
                if role < *existing {
                    *existing = role;
                }
            }
            None => participants.push((pk, role)),
        }
    };
    for arrow in arrows {
        classify(arrow.source, true);
        classify(arrow.target, false);
    }
    // Stable sort: lane groups in Role order, first-appearance order within
    // a group (sort_by_key is stable, so insertion order survives).
    participants.sort_by_key(|(_, role)| *role);
    participants
}

fn emit_participants(out: &mut String, participants: &[(Pubkey, Role)], aliases: &Aliases) {
    for (pk, role) in participants {
        let label = aliases.label(pk);
        let _ = writeln!(
            out,
            "{INDENT}participant {} as \"{} ({})\"",
            mermaid_id(&label),
            label,
            role.suffix(),
        );
    }
}

/// Make free text safe inside a Mermaid `note over ...:` line. Mermaid treats
/// `;` as a statement separator, so a semicolon mid-note silently ends the note
/// and the remainder fails to parse (GitHub renders "Parse error ... got
/// NEWLINE"). Collapse any newlines and swap `;` for `,`: note text is
/// descriptive prose, so the substitution costs nothing and keeps the diagram
/// parseable whatever an author writes in a `spotlight` caption or an alias.
fn note_safe(text: &str) -> String {
    text.split(['\n', '\r'])
        .collect::<Vec<_>>()
        .join(" ")
        .replace(';', ",")
}

fn emit_arrows(out: &mut String, arrows: &[Arrow], aliases: &Aliases) {
    for arrow in arrows {
        let connector = if arrow.failed { "--x" } else { "->>" };
        let _ = writeln!(
            out,
            "{INDENT}{} {} {}: {}",
            mermaid_id(&aliases.label(&arrow.source)),
            connector,
            mermaid_id(&aliases.label(&arrow.target)),
            arrow.label,
        );
    }
}

/// Render the authority view of one transaction's model, wrapped in a fenced
/// ```mermaid block. Returns an empty string when the model yields no
/// authority-carrying writes.
///
/// Per-frame outcome comes from the model's `cpi_tree` classification: a frame
/// that failed renders `--x` / `✗`, including a parent frame that failed after
/// a successful CPI (the heuristic this replaced mislabelled that case).
#[allow(dead_code)] // exercised by the unit tests; production uses AuthorityGraph
pub(super) fn render(model: &CpiModel, tx_signers: &[Pubkey], aliases: &Aliases) -> String {
    let arrows = build_arrows(model, tx_signers, aliases);
    if arrows.is_empty() {
        return String::new();
    }
    let participants = classify_participants(&arrows, tx_signers);

    let mut out = String::new();
    out.push_str("```mermaid\n");
    out.push_str("sequenceDiagram\n");
    out.push_str(INDENT);
    out.push_str("autonumber\n");
    emit_participants(&mut out, &participants, aliases);
    emit_arrows(&mut out, &arrows, aliases);
    out.push_str("```\n");
    out
}

/// One submitted transaction inside an [`AuthorityStory`].
#[derive(Clone)]
struct Section {
    label: String,
    /// The fully-resolved model both the sequence and the account index render
    /// from: per-frame outcome from `cpi_tree`, account authority from the
    /// trace. One source of truth per submitted transaction.
    model: CpiModel,
    /// Author-supplied "look here" caption, set via [`AuthorityStory::spotlight`]
    /// before the send. `Some` flags the section with 🧐 and appends the reason
    /// to its divider note even when the transaction settled, which is the whole
    /// point: the interesting steps are often the ones where nothing reverts (a
    /// cap that charged nothing, a guard that waved an Approve through).
    spotlight: Option<String>,
}

/// The test's authority story: one section per submitted transaction,
/// rendered as a single Mermaid diagram with unified lanes.
///
/// Unified lanes are the point of grouping: the same signer occupies the
/// same lane in every section, so "the PDA signs the freeze in cycle one
/// and the re-freeze in cycle two" is visible as two arrows leaving the
/// same lane, sections apart. This is the generated counterpart of the
/// hand-drawn `restake-authority-flow.puml` and its `== section ==`
/// dividers.
///
/// Feeding: `AnchorContext` appends a section automatically on every send
/// (the zero-ceremony path), and [`section`](Self::section) is the manual
/// hatch for curated stories. Render via [`ToBlock`] (so it drops into
/// `Report::authority()` / `Report::snapshot()`), or [`mermaid_string`](
/// Self::mermaid_string) for the fenced block directly.
#[derive(Clone, Default)]
pub struct AuthorityStory {
    sections: Vec<Section>,
    /// Alias table used at render time; refreshed on every section append
    /// (last table wins, which is the fullest one). [`ToBlock`] takes no
    /// arguments, so the table must be captured rather than passed.
    aliases: Option<Aliases>,
    /// Caption for the next section appended, set by [`spotlight`](Self::spotlight)
    /// and taken when that section is pushed. One spotlight per section.
    pending_spotlight: Option<String>,
}

impl AuthorityStory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Flag the next appended section as interesting: it renders with a 🧐 and
    /// `reason` on its divider note, regardless of outcome. The hook for
    /// silent-success steps, where the noteworthy thing is that a cap or guard
    /// did nothing while everything settled. Consumed (taken) by the next
    /// `section`/`section_auto`; failures flag themselves (with 🚩) without it.
    pub fn spotlight(&mut self, reason: impl Into<String>) -> &mut Self {
        self.pending_spotlight = Some(reason.into());
        self
    }

    /// Pull the section ingredients out of a result: the trace, the tx
    /// signers, and the execution-order frame names (cleared when the log
    /// tree and the trace disagree on frame count). `None` when the result
    /// carries no trace (raw `TransactionHelpers` sends; context sends
    /// always attach one).
    fn prepare(tx: &Transaction) -> Option<CpiModel> {
        // A backend that witnessed the per-frame trace (litesvm, quasar)
        // carries an authority story; one that didn't (a raw log-only record)
        // has no inner-frame privilege to draw, so it carries none.
        tx.trace.as_ref()?;
        Some(from_transaction(tx))
    }

    fn push(&mut self, label: String, tx: &Transaction, model: CpiModel) {
        // The neutral record always carries an alias table (well-known at
        // least); refreshed on every append so the fullest table wins, captured
        // for render time since `ToBlock` takes no arguments.
        self.aliases = Some(tx.aliases.clone());
        let spotlight = self.pending_spotlight.take();
        self.sections.push(Section {
            label,
            model,
            spotlight,
        });
    }

    /// Record one submitted transaction as a section with an explicit
    /// label. No-op (with no error) when the result carries no instruction
    /// trace: raw `TransactionHelpers` sends don't, context sends do.
    pub fn section(&mut self, label: impl Into<String>, tx: &Transaction) {
        let Some(prepared) = Self::prepare(tx) else {
            return;
        };
        self.push(label.into(), tx, prepared);
    }

    /// Record a section labelled by the transaction's top-level instruction
    /// names (`Vault::Withdraw`), the zero-ceremony path `AnchorContext`
    /// uses. Failed transactions get a `✗` suffix on the label.
    ///
    /// Labels use the same name-resolution chain as the arrows: the
    /// log-derived instruction name when available (the only way to name
    /// Anchor instructions), else the discriminator decoder against the
    /// trace's data bytes (System, SPL Token, ATA), else the bare program
    /// label.
    pub fn section_auto(&mut self, tx: &Transaction) {
        let Some(prepared) = Self::prepare(tx) else {
            return;
        };
        let model = &prepared;
        let aliases = &tx.aliases;

        // One label part per top-level instruction (multi-ix transactions
        // join with " + ").
        let parts: Vec<String> = model
            .roots
            .iter()
            .map(|root| {
                let program = aliases.label(&root.frame.program);
                match &root.frame.instruction_name {
                    Some(n) => format!("{program}::{n}"),
                    None => program,
                }
            })
            .collect();

        let mut label = if parts.is_empty() {
            "transaction".to_string()
        } else {
            parts.join(" + ")
        };
        if model.error.is_some() {
            label.push_str(" ✗");
        }
        self.push(label, tx, prepared);
    }

    /// Override the alias table used at render time. The context-owned
    /// story refreshes this on read so late `ctx.alias()` calls still name
    /// every lane.
    pub fn with_aliases(mut self, aliases: Aliases) -> Self {
        self.aliases = Some(aliases);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    pub fn len(&self) -> usize {
        self.sections.len()
    }

    /// The [`AccountIndex`](super::AccountIndex) (census by owner + authority class + ATA edges)
    /// for everything this story touched. A different *view* of the same
    /// accumulated traces: where the diagram shows the flow of writes, the
    /// index shows the standing structure of the accounts. Render it with
    /// `index.to_tree(&aliases)`; the context convenience
    /// (`ctx.account_index()`) threads the context's alias table for you.
    pub fn account_index(&self) -> super::AccountIndex {
        let all_tx_signers: Vec<Pubkey> = self
            .sections
            .iter()
            .flat_map(|s| s.model.tx_signers.iter().copied())
            .fold(Vec::new(), |mut acc, pk| {
                if !acc.contains(&pk) {
                    acc.push(pk);
                }
                acc
            });
        let models: Vec<&CpiModel> = self.sections.iter().map(|s| &s.model).collect();
        super::AccountIndex::build(&models, &all_tx_signers)
    }

    /// The alias table captured from the sends (well-known names plus
    /// whatever the context had aliased). Used to render the account index
    /// with the same names as the diagram.
    #[allow(dead_code)] // available for callers rendering the index standalone
    pub(crate) fn aliases(&self) -> Aliases {
        self.aliases.clone().unwrap_or_default()
    }

    /// The unified diagram as a fenced ```mermaid block. Empty string when
    /// no section produced any authority-carrying write.
    pub fn mermaid_string(&self) -> String {
        let body = self.render_body();
        if body.is_empty() {
            return String::new();
        }
        format!("```mermaid\n{body}```\n")
    }

    /// The diagram body (no fence): participants once, then per section a
    /// spanning note carrying its label followed by its arrows.
    fn render_body(&self) -> String {
        let default_aliases;
        let aliases = match &self.aliases {
            Some(a) => a,
            None => {
                default_aliases = Aliases::default();
                &default_aliases
            }
        };

        // Union of every section's tx signers: a signer keeps its lane role
        // across the whole story, even in sections it doesn't sign.
        let all_tx_signers: Vec<Pubkey> = self
            .sections
            .iter()
            .flat_map(|s| s.model.tx_signers.iter().copied())
            .fold(Vec::new(), |mut acc, pk| {
                if !acc.contains(&pk) {
                    acc.push(pk);
                }
                acc
            });

        let per_section: Vec<(&Section, Vec<Arrow>)> = self
            .sections
            .iter()
            .map(|s| (s, build_arrows(&s.model, &all_tx_signers, aliases)))
            .collect();

        let all_arrows: Vec<&Arrow> = per_section.iter().flat_map(|(_, a)| a.iter()).collect();
        if all_arrows.is_empty() {
            return String::new();
        }

        // Participants unified across sections: classify against the
        // flattened arrow list so lanes are stable story-wide.
        let flat: Vec<Arrow> = per_section
            .iter()
            .flat_map(|(_, arrows)| {
                arrows.iter().map(|a| Arrow {
                    source: a.source,
                    target: a.target,
                    label: a.label.clone(),
                    failed: a.failed,
                })
            })
            .collect();
        let participants = classify_participants(&flat, &all_tx_signers);

        let mut out = String::new();
        out.push_str("sequenceDiagram\n");
        out.push_str(INDENT);
        out.push_str("autonumber\n");
        emit_participants(&mut out, &participants, aliases);

        // Section dividers: a note spanning the leftmost-to-rightmost lane.
        let first_id = mermaid_id(&aliases.label(&participants[0].0));
        let last_id = mermaid_id(&aliases.label(&participants[participants.len() - 1].0));
        let span = if first_id == last_id {
            first_id
        } else {
            format!("{first_id},{last_id}")
        };

        for (section, arrows) in &per_section {
            // Tint the section band by outcome (`rect rgb(...)` paints under
            // the divider note and the section's arrows): pale green when the
            // transaction settled, pale red when it reverted.
            // Colour by exception (see the const docs): a failure opens a red
            // band, an author-spotlight a blue one, a routine success no band
            // at all (the norm stays neutral so it isn't visual noise).
            let failed = section.model.error.is_some();
            let spotlit = section.spotlight.is_some();
            let band = if failed {
                Some(SECTION_FAIL_RGBA)
            } else if spotlit {
                Some(SECTION_SPOTLIGHT_RGBA)
            } else {
                None
            };
            if let Some((r, g, b, a)) = band {
                let _ = writeln!(out, "{INDENT}rect rgba({r}, {g}, {b}, {a:.2})");
            }
            // The glyph mirrors the band, for symmetry: 🚩 (red flag) on a
            // failure, 🧐 (monocle, "look closely") on an author-spotlighted
            // step, nothing on routine. A spotlight caption rides after the
            // label so the reader knows *why* it is flagged.
            let marker = if failed {
                "🚩 "
            } else if spotlit {
                "🧐 "
            } else {
                ""
            };
            let caption = match &section.spotlight {
                Some(reason) => format!(" ({reason})"),
                None => String::new(),
            };
            let note = format!("{marker}{}{caption}", section.label);
            let _ = writeln!(out, "{INDENT}note over {span}: {}", note_safe(&note));
            emit_arrows(&mut out, arrows, aliases);
            if band.is_some() {
                let _ = writeln!(out, "{INDENT}end");
            }
        }
        out
    }
}

impl ToBlock for AuthorityStory {
    fn to_block(&self) -> Block {
        Block::Fenced {
            lang: Some("mermaid".to_string()),
            text: self.render_body(),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::cpi::model::{AccountRef, ResolvedFrame, Root},
        crate::trace::{InstructionTrace, TracedAccount, TracedInstruction},
        std::str::FromStr,
    };

    const SYSTEM_ID: &str = "11111111111111111111111111111111";

    fn system_id() -> Pubkey {
        Pubkey::from_str(SYSTEM_ID).unwrap()
    }

    fn acct(pubkey: Pubkey, is_signer: bool, is_writable: bool, owner: Pubkey) -> TracedAccount {
        TracedAccount {
            pubkey,
            is_signer,
            is_writable,
            owner,
        }
    }

    /// Build a [`CpiModel`] from a flat trace fixture: reconstruct the frame
    /// tree from `stack_height`, attaching the resolved instruction name and
    /// the per-frame outcome (`failed[i]`) each test specifies. Lets the
    /// trace-shaped fixtures drive the model-based renderer with minimal
    /// change. (Production builds the model from `cpi_tree` + `fill_from_trace`;
    /// this constructs the same shape directly.)
    fn model_of(
        trace: &InstructionTrace,
        names: &[Option<&str>],
        failed: &[bool],
        tx_signers: &[Pubkey],
    ) -> CpiModel {
        fn take(
            frames: &[TracedInstruction],
            names: &[Option<&str>],
            failed: &[bool],
            pos: &mut usize,
            height: usize,
        ) -> Vec<ResolvedFrame> {
            let mut out = Vec::new();
            while *pos < frames.len() && frames[*pos].stack_height == height {
                let i = *pos;
                let f = &frames[i];
                *pos += 1;
                let children = take(frames, names, failed, pos, height + 1);
                let outcome = if *failed.get(i).unwrap_or(&false) {
                    Outcome::Failed { message: None }
                } else {
                    Outcome::Success
                };
                out.push(ResolvedFrame {
                    program: f.program_id,
                    instruction_name: names.get(i).copied().flatten().map(String::from),
                    operands: vec![],
                    outcome,
                    compute_units: None,
                    accounts: f
                        .accounts
                        .iter()
                        .map(|a| AccountRef {
                            pubkey: a.pubkey,
                            is_signer: a.is_signer,
                            is_writable: a.is_writable,
                            owner: Some(a.owner),
                        })
                        .collect(),
                    logs: Vec::new(),
                    data: Vec::new(),
                    children,
                });
            }
            out
        }
        // Root at the shallowest frame so fragment fixtures (a lone CPI frame
        // at stack_height 2) reconstruct as a one-frame model.
        let root_height = trace.0.iter().map(|f| f.stack_height).min().unwrap_or(1);
        let mut pos = 0;
        let roots = take(&trace.0, names, failed, &mut pos, root_height)
            .into_iter()
            .map(|frame| Root {
                signers: tx_signers.to_vec(),
                frame,
            })
            .collect();
        CpiModel {
            header: None,
            roots,
            tx_signers: tx_signers.to_vec(),
            error: failed.iter().any(|f| *f).then(String::new),
            compute_units: 0,
            fee: 0,
            events: Default::default(),
        }
    }

    /// The vault-withdraw shape: Alice signs the tx; the program transfers
    /// SOL from the vault PDA to Alice via `invoke_signed`. The vault and
    /// Alice are both System-owned (a SystemAccount and a wallet), so the
    /// Vault program owns neither: its top-level frame draws no arrow, and
    /// the only write-arrow is the System::Transfer CPI's payout.
    fn vault_shaped() -> (InstructionTrace, Pubkey, Pubkey, Pubkey, Aliases) {
        let alice = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let system = system_id();

        let trace = InstructionTrace(vec![
            // Withdraw (top level): the Vault program owns neither account it
            // touches (both are System-owned), so it requests write access
            // but performs no direct write.
            TracedInstruction {
                program_id: program,
                stack_height: 1,
                accounts: vec![
                    acct(alice, true, true, system),
                    acct(vault, false, true, system),
                ],
                data: vec![0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 0],
            },
            // System transfer CPI: the vault PDA signs (invoke_signed), Alice
            // is the writable recipient; System owns both.
            TracedInstruction {
                program_id: system,
                stack_height: 2,
                // System transfer discriminator: u32 LE = 2.
                accounts: vec![
                    acct(vault, true, true, system),
                    acct(alice, false, true, system),
                ],
                data: vec![2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
            },
        ]);

        let aliases = Aliases::default()
            .with(alice, "Alice")
            .with(vault, "vault")
            .with(program, "Vault");
        (trace, alice, vault, program, aliases)
    }

    /// Build a top-level System::Transfer frame: `from` (signer) pays `to`.
    /// Both System-owned, so the System program (which owns them) is the
    /// writer.
    fn system_transfer(from: Pubkey, to: Pubkey, from_signs: bool) -> TracedInstruction {
        let system = system_id();
        TracedInstruction {
            program_id: system,
            stack_height: 1,
            accounts: vec![
                acct(from, from_signs, true, system),
                acct(to, false, true, system),
            ],
            data: vec![2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        }
    }

    #[test]
    fn vault_withdraw_renders_program_signed_lane() {
        let (trace, alice, _vault, _program, aliases) = vault_shaped();

        let model = model_of(&trace, &[None, Some("Transfer")], &[false, false], &[alice]);
        let out = render(&model, &[alice], &aliases);

        // Lanes: Alice (tx signer) before vault (program-signed).
        let alice_lane = out
            .find("participant Alice as \"Alice (tx signer)\"")
            .unwrap();
        let vault_lane = out
            .find("participant vault as \"vault (program-signed)\"")
            .unwrap();
        assert!(
            alice_lane < vault_lane,
            "signer lane left of PDA lane:\n{out}"
        );

        // The CPI write: the vault PDA's (program) signature pays Alice.
        // System transfer decodes via the discriminator table.
        assert!(
            out.contains("vault ->> Alice: System::Transfer ✓"),
            "got:\n{out}"
        );

        // The top-level Vault frame owns neither account, so it draws no
        // arrow: no misattribution of the payout to Alice's signature.
        assert!(
            !out.contains("Alice ->> vault: Vault"),
            "top-level frame must not claim Alice wrote the vault:\n{out}"
        );
    }

    #[test]
    fn log_name_labels_an_owned_write() {
        // A program writing its own PDA: the top-level frame's program owns
        // the config account, so the write-arrow is drawn here and carries
        // the log-derived instruction name.
        let user = Pubkey::new_unique();
        let config = Pubkey::new_unique();
        let program = Pubkey::new_unique();

        let trace = InstructionTrace(vec![TracedInstruction {
            program_id: program,
            stack_height: 1,
            // config is owned by the program and written directly.
            accounts: vec![
                acct(user, true, true, system_id()),
                acct(config, false, true, program),
            ],
            data: vec![1, 2, 3, 4, 5, 6, 7, 8],
        }]);

        let aliases = Aliases::default()
            .with(user, "User")
            .with(config, "Config")
            .with(program, "MyProgram");

        let model = model_of(&trace, &[Some("SetParams")], &[false], &[user]);
        let out = render(&model, &[user], &aliases);

        assert!(
            out.contains("User ->> Config: MyProgram::SetParams ✓"),
            "got:\n{out}"
        );
    }

    #[test]
    fn failed_cpi_frame_marks_its_arrow() {
        let (trace, alice, _, _, aliases) = vault_shaped();

        // The transfer CPI failed (and the parent withdraw propagates it); the
        // model carries the outcome per frame, so the CPI's payout is marked
        // failed.
        let model = model_of(&trace, &[None, Some("Transfer")], &[true, true], &[alice]);
        let out = render(&model, &[alice], &aliases);

        assert!(
            out.contains("vault --x Alice: System::Transfer ✗"),
            "got:\n{out}"
        );
    }

    #[test]
    fn parent_failure_after_successful_child_marks_the_parent() {
        // The regression this whole change targets: AttachPolicy does a
        // CreateAccount CPI (which SUCCEEDS), then validates and rejects with
        // InvalidPolicyData. The failure is in the PARENT frame, after its
        // child returned. The old `i == last_frame` heuristic marked the last
        // frame (the successful CreateAccount) and left the parent's writes
        // ✓ — backwards. Per-frame outcome marks the parent's writes ✗ and the
        // child ✓.
        let owner = Pubkey::new_unique();
        let session = Pubkey::new_unique();
        let policy = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let system = system_id();

        let trace = InstructionTrace(vec![
            // AttachPolicy (top): the program owns session + policy and writes
            // both. This frame is the one that failed.
            TracedInstruction {
                program_id: program,
                stack_height: 1,
                accounts: vec![
                    acct(owner, true, true, system),
                    acct(session, false, true, program),
                    acct(policy, false, true, program),
                ],
                data: vec![],
            },
            // CreateAccount CPI: succeeded before the parent's validation
            // rejected. System owns nothing it writes here (the new account is
            // program-assigned), so it draws no arrow.
            TracedInstruction {
                program_id: system,
                stack_height: 2,
                accounts: vec![
                    acct(owner, true, true, system),
                    acct(policy, false, true, program),
                ],
                data: vec![0, 0, 0, 0],
            },
        ]);

        let aliases = Aliases::default()
            .with(owner, "owner")
            .with(session, "Session")
            .with(policy, "Policy")
            .with(program, "Bastion");

        // Parent failed, child succeeded.
        let model = model_of(
            &trace,
            &[Some("AttachPolicy"), Some("CreateAccount")],
            &[true, false],
            &[owner],
        );
        let out = render(&model, &[owner], &aliases);

        // The parent's writes are marked failed.
        assert!(
            out.contains("owner --x Session: Bastion::AttachPolicy ✗"),
            "the failed parent frame's write must be marked ✗:\n{out}"
        );
        assert!(
            out.contains("owner --x Policy: Bastion::AttachPolicy ✗"),
            "the failed parent frame's write must be marked ✗:\n{out}"
        );
        // And nothing is marked succeeded: the whole transaction reverted.
        assert!(
            !out.contains(" ✓"),
            "a reverted transaction draws no successful write:\n{out}"
        );
    }

    #[test]
    fn program_signed_origin_wins_over_extended_signature() {
        // The restake shape: an UpdatePlugin CPI carrying BOTH the owner's
        // extended tx signature (as payer) and the PDA's invoke_signed
        // signature (as authority). The arrow must leave the PDA's lane. The
        // asset is mpl-core-owned, so the mpl-core frame is its writer.
        let owner = Pubkey::new_unique();
        let pda = Pubkey::new_unique();
        let asset = Pubkey::new_unique();
        let mpl = Pubkey::new_unique();

        let trace = InstructionTrace(vec![TracedInstruction {
            program_id: mpl,
            stack_height: 2,
            accounts: vec![
                acct(asset, false, true, mpl),        // mpl-core owns the asset
                acct(owner, true, true, system_id()), // payer: extended tx signature
                acct(pda, true, false, system_id()),  // authority: program-signed
            ],
            data: vec![],
        }]);

        let aliases = Aliases::default()
            .with(owner, "Owner")
            .with(pda, "UpdateAuthorityPDA")
            .with(asset, "Asset")
            .with(mpl, "MplCore");

        let model = model_of(&trace, &[None], &[false], &[owner]);
        let out = render(&model, &[owner], &aliases);

        assert!(
            out.contains("UpdateAuthorityPDA ->> Asset: MplCore ✓"),
            "arrow must leave the PDA lane, not the owner's:\n{out}"
        );
        assert!(
            !out.contains("Owner ->> Asset"),
            "no duplicate arrow from the extended signature:\n{out}"
        );
    }

    #[test]
    fn extended_signature_is_origin_when_no_pda_signs() {
        // The draft-fix shape: the same CPI but the program presented the
        // owner as the authority (no invoke_signed). The arrow leaves the
        // owner's lane: that IS the bug, made visible.
        let owner = Pubkey::new_unique();
        let asset = Pubkey::new_unique();
        let mpl = Pubkey::new_unique();

        let trace = InstructionTrace(vec![TracedInstruction {
            program_id: mpl,
            stack_height: 2,
            accounts: vec![
                acct(asset, false, true, mpl),
                acct(owner, true, true, system_id()),
            ],
            data: vec![],
        }]);

        let aliases = Aliases::default()
            .with(owner, "Owner")
            .with(asset, "Asset")
            .with(mpl, "MplCore");

        let model = model_of(&trace, &[None], &[true], &[owner]);
        let out = render(&model, &[owner], &aliases);

        assert!(out.contains("Owner --x Asset: MplCore ✗"), "got:\n{out}");
        assert!(
            out.contains("participant Owner as \"Owner (tx signer)\""),
            "got:\n{out}"
        );
    }

    #[test]
    fn anchor_init_renders_owned_pda_write_at_the_parent_frame() {
        // Anchor `init`: the parent program frame lists the new PDA as a
        // writable account it OWNS (post-creation), so the write-arrow is
        // drawn there. The System::CreateAccount CPI below owns nothing it
        // touches (the new account was assigned to the program, the payer is
        // System-owned and signs), so it adds no duplicate arrow. The
        // ownership rule places the init write at the frame that actually
        // owns the account, which is the program, not System.
        let payer = Pubkey::new_unique();
        let new_pda = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let system = system_id();

        let trace = InstructionTrace(vec![
            TracedInstruction {
                program_id: program,
                stack_height: 1,
                // new_pda is owned by the program after creation.
                accounts: vec![
                    acct(payer, true, true, system),
                    acct(new_pda, false, true, program),
                ],
                data: vec![9, 9, 9, 9, 9, 9, 9, 9],
            },
            TracedInstruction {
                program_id: system,
                stack_height: 2,
                // CreateAccount: payer signs; the new account is now the
                // program's. System owns neither at trace-read time.
                accounts: vec![
                    acct(payer, true, true, system),
                    acct(new_pda, true, true, program),
                ],
                data: vec![0, 0, 0, 0],
            },
        ]);

        let aliases = Aliases::default()
            .with(payer, "payer")
            .with(new_pda, "vault_state")
            .with(program, "Vault");

        let model = model_of(
            &trace,
            &[Some("Initialize"), None],
            &[false, false],
            &[payer],
        );
        let out = render(&model, &[payer], &aliases);

        assert!(
            out.contains("payer ->> vault_state: Vault::Initialize ✓"),
            "the init write is drawn at the owning program's frame:\n{out}"
        );
        // Exactly one arrow lands on vault_state: the CreateAccount CPI owns
        // nothing it touches, so it adds no duplicate.
        assert_eq!(
            out.matches("->> vault_state:").count(),
            1,
            "no duplicate init arrow from the CreateAccount CPI:\n{out}"
        );
        assert!(
            !out.contains("System::CreateAccount"),
            "CreateAccount owns nothing it touches, so it draws no arrow:\n{out}"
        );
    }

    #[test]
    fn human_only_create_account_emits_nothing() {
        // CreateAccount with a human keypair (not a PDA): both accounts are
        // tx-level signers, so both are excluded as write targets. No
        // authority story to tell.
        let payer = Pubkey::new_unique();
        let new_kp = Pubkey::new_unique();
        let system = system_id();

        let trace = InstructionTrace(vec![TracedInstruction {
            program_id: system,
            stack_height: 1,
            accounts: vec![
                acct(payer, true, true, system),
                acct(new_kp, true, true, system),
            ],
            data: vec![0, 0, 0, 0],
        }]);

        let model = model_of(&trace, &[None], &[false], &[payer, new_kp]);
        let out = render(&model, &[payer, new_kp], &Aliases::default());
        assert_eq!(out, "");
    }

    #[test]
    fn frame_writing_an_account_it_does_not_own_emits_nothing() {
        // A signer is present and an account is writable, but the frame's
        // program does not own it (a top-level Anchor frame passing a token
        // account through to a CPI). The write belongs to the owner's frame,
        // not here.
        let user = Pubkey::new_unique();
        let token_acct = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let token_program = Pubkey::new_unique();

        let trace = InstructionTrace(vec![TracedInstruction {
            program_id: program,
            stack_height: 1,
            accounts: vec![
                acct(user, true, true, system_id()),
                // token_acct is owned by the token program, not `program`.
                acct(token_acct, false, true, token_program),
            ],
            data: vec![],
        }]);

        let model = model_of(&trace, &[None], &[false], &[user]);
        let out = render(&model, &[user], &Aliases::default());
        assert_eq!(out, "", "access without ownership draws no write-arrow");
    }

    #[test]
    fn frames_without_signers_emit_nothing() {
        let a = Pubkey::new_unique();
        let program = Pubkey::new_unique();

        let trace = InstructionTrace(vec![TracedInstruction {
            program_id: program,
            stack_height: 1,
            // owned by the frame's program, but no signer to originate from.
            accounts: vec![acct(a, false, true, program)],
            data: vec![],
        }]);

        let model = model_of(&trace, &[None], &[false], &[a]);
        let out = render(&model, &[a], &Aliases::default());
        assert_eq!(out, "", "no signers -> no authority-carrying writes");
    }

    #[test]
    fn empty_trace_renders_empty_string() {
        let out = render(
            &model_of(&InstructionTrace::default(), &[], &[], &[]),
            &[],
            &Aliases::default(),
        );
        assert_eq!(out, "");
    }

    // --- AuthorityStory -----------------------------------------------------

    /// Build a story directly from sections (bypassing TransactionResult,
    /// which needs a live SVM to construct). The story's public feeding path
    /// is covered by the anchor-litesvm integration test and the vault
    /// dogfood; these tests pin the unified rendering.
    fn story_from(sections: Vec<Section>, aliases: Aliases) -> AuthorityStory {
        AuthorityStory {
            sections,
            aliases: Some(aliases),
            pending_spotlight: None,
        }
    }

    /// A `Section` with no spotlight, the common test shape.
    fn section(label: &str, model: CpiModel) -> Section {
        Section {
            label: label.to_string(),
            model,
            spotlight: None,
        }
    }

    #[test]
    fn story_unifies_lanes_across_sections() {
        let (withdraw_trace, alice, vault, _program, aliases) = vault_shaped();

        // Two sections from the same cast: a deposit (Alice pays the vault)
        // and the withdraw (the vault PDA pays Alice back). Each is a single
        // System::Transfer CPI; the lane each arrow leaves is the whole
        // point of the contrast.
        let deposit_trace = InstructionTrace(vec![system_transfer(alice, vault, true)]);
        let story = story_from(
            vec![
                section(
                    "Vault::Deposit",
                    model_of(&deposit_trace, &[Some("Transfer")], &[false], &[alice]),
                ),
                section(
                    "Vault::Withdraw",
                    model_of(
                        &withdraw_trace,
                        &[None, Some("Transfer")],
                        &[false, false],
                        &[alice],
                    ),
                ),
            ],
            aliases,
        );

        let body = story.mermaid_string();

        // Each participant declared exactly once, despite appearing in both
        // sections.
        assert_eq!(
            body.matches("participant Alice").count(),
            1,
            "unified lanes:\n{body}"
        );
        assert_eq!(body.matches("participant vault").count(), 1, "got:\n{body}");

        // Section dividers in submission order, spanning the lanes.
        let deposit_note = body.find("note over Alice,vault: Vault::Deposit").unwrap();
        let withdraw_note = body.find("note over Alice,vault: Vault::Withdraw").unwrap();
        assert!(deposit_note < withdraw_note, "submission order:\n{body}");

        // The contrast: deposit leaves Alice's lane, withdraw leaves the
        // vault's. Same two accounts, opposite arrow origins.
        let deposit_arrow = body.find("Alice ->> vault: System::Transfer ✓").unwrap();
        let withdraw_arrow = body.find("vault ->> Alice: System::Transfer ✓").unwrap();
        assert!(
            deposit_note < deposit_arrow && deposit_arrow < withdraw_note,
            "got:\n{body}"
        );
        assert!(withdraw_note < withdraw_arrow, "got:\n{body}");

        // vault keeps its program-signed lane even though the deposit section
        // only writes it (Target role would be the misclassification).
        assert!(
            body.contains("participant vault as \"vault (program-signed)\""),
            "strongest role wins across sections:\n{body}"
        );
    }

    #[test]
    fn story_failed_section_keeps_failure_marks() {
        let (trace, alice, _, _, aliases) = vault_shaped();

        let story = story_from(
            vec![section(
                "Vault::Withdraw ✗",
                model_of(&trace, &[None, Some("Transfer")], &[true, true], &[alice]),
            )],
            aliases,
        );

        let body = story.mermaid_string();
        // A reverted section auto-flags with 🚩 (no author spotlight needed).
        assert!(
            body.contains("note over Alice,vault: 🚩 Vault::Withdraw ✗"),
            "got:\n{body}"
        );
        assert!(body.contains("vault --x Alice"), "got:\n{body}");
    }

    #[test]
    fn story_colours_only_the_exceptions() {
        let (withdraw_trace, alice, vault, _program, aliases) = vault_shaped();
        let deposit_trace = InstructionTrace(vec![system_transfer(alice, vault, true)]);
        let story = story_from(
            vec![
                section(
                    "Vault::Deposit",
                    model_of(&deposit_trace, &[Some("Transfer")], &[false], &[alice]),
                ),
                section(
                    "Vault::Withdraw ✗",
                    model_of(
                        &withdraw_trace,
                        &[None, Some("Transfer")],
                        &[true, true],
                        &[alice],
                    ),
                ),
            ],
            aliases,
        );
        let body = story.mermaid_string();

        // Colour by exception: the routine deposit gets NO band; only the
        // reverted withdraw earns one (translucent red), closed by `end`.
        assert_eq!(
            body.matches("rect rgba").count(),
            1,
            "only the failure is banded:\n{body}"
        );
        assert_eq!(
            body.matches("    end\n").count(),
            1,
            "one band, one close:\n{body}"
        );
        assert!(
            body.contains("rect rgba(231, 76, 60, 0.25)"),
            "failure red band:\n{body}"
        );

        // The deposit note is emitted unbanded, before the failure band opens.
        let deposit = body.find("Vault::Deposit").unwrap();
        let fail = body.find("rect rgba(231, 76, 60, 0.25)").unwrap();
        let withdraw = body.find("Vault::Withdraw").unwrap();
        assert!(
            deposit < fail,
            "the routine deposit precedes the failure band:\n{body}"
        );
        assert!(fail < withdraw, "the red band wraps the withdraw:\n{body}");
    }

    #[test]
    fn story_spotlight_flags_a_settled_section_in_blue() {
        // The silent-success case: a deposit that settled but the author
        // flagged as interesting. It is noteworthy, not broken, so it gets a
        // blue band (never red) plus the 🚩 and caption.
        let (_t, alice, vault, _p, aliases) = vault_shaped();
        let deposit_trace = InstructionTrace(vec![system_transfer(alice, vault, true)]);
        let mut spotlit = section(
            "Vault::Deposit",
            model_of(&deposit_trace, &[Some("Transfer")], &[false], &[alice]),
        );
        spotlit.spotlight = Some("the cap charged nothing".to_string());
        let story = story_from(vec![spotlit], aliases);

        let body = story.mermaid_string();
        assert!(
            body.contains("note over Alice,vault: 🧐 Vault::Deposit (the cap charged nothing)"),
            "spotlit success carries 🧐 + caption:\n{body}"
        );
        assert!(
            body.contains("rect rgba(52, 152, 219, 0.18)"),
            "a spotlit success gets a blue band:\n{body}"
        );
        assert!(
            !body.contains("rect rgba(231, 76, 60"),
            "a success is never red:\n{body}"
        );
    }

    #[test]
    fn semicolon_in_a_caption_is_made_mermaid_safe() {
        // A `;` is a Mermaid statement separator: left raw in a note it splits
        // the line and GitHub fails to render the diagram. The renderer swaps it
        // for a comma so any caption parses.
        let (_t, alice, vault, _p, aliases) = vault_shaped();
        let deposit_trace = InstructionTrace(vec![system_transfer(alice, vault, true)]);
        let mut spotlit = section(
            "Vault::Deposit",
            model_of(&deposit_trace, &[Some("Transfer")], &[false], &[alice]),
        );
        spotlit.spotlight = Some("withheld; its restriction never runs".to_string());
        let story = story_from(vec![spotlit], aliases);

        let body = story.mermaid_string();
        assert!(
            !body.contains(';'),
            "no raw semicolon survives in the diagram:\n{body}"
        );
        assert!(
            body.contains("(withheld, its restriction never runs)"),
            "the semicolon was swapped for a comma:\n{body}"
        );
    }

    #[test]
    fn empty_story_renders_empty_markdown() {
        let story = AuthorityStory::new();
        assert!(story.is_empty());
        assert_eq!(story.mermaid_string(), "");
        match story.to_block() {
            Block::Fenced { text, .. } => assert_eq!(text, ""),
            _ => panic!("expected fenced block"),
        }
    }
}
