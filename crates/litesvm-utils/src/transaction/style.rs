//! ANSI color styling for the structured-log printer.
//!
//! The structured-log printer writes plain text by default. Set
//! `ANCHOR_LITESVM_COLOR=1` in the environment to wrap status glyphs
//! (`✓` / `✗`), the `(no cu)` / `(truncated)` markers, and error lines
//! with ANSI SGR codes so a terminal renders them in color. The
//! `NO_COLOR` env var (no-color.org) takes precedence and forces plain
//! output even if `ANCHOR_LITESVM_COLOR` is also set.
//!
//! ## Why an env var, not a flag
//!
//! The printer is called from inside `#[test]` functions; `cargo test`
//! doesn't forward arbitrary CLI flags to test code. An env var is the
//! mechanism that survives that boundary: `ANCHOR_LITESVM_COLOR=1
//! cargo test -- --nocapture`.
//!
//! ## Why opt-in by default
//!
//! Test runners often capture stdout to log files or display in CI
//! consoles that don't interpret ANSI. Defaulting to plain text means
//! captured output stays readable and snapshots stay stable. Opting
//! in once in your shell is a small price for the readability win
//! when running tests interactively.

/// Whether the printer should emit ANSI SGR codes.
///
/// Detected via [`Style::detect`] using environment variables; threaded
/// explicitly through the render functions rather than read from env
/// at every glyph (so a single render uses one consistent decision).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Style {
    /// No ANSI codes; output is plain text.
    Off,
    /// Wrap status markers with SGR codes.
    On,
}

impl Style {
    /// `NO_COLOR` (universal off-switch, https://no-color.org) wins; then
    /// `ANCHOR_LITESVM_COLOR=1` opts in; otherwise off.
    ///
    /// Both env vars are checked by *presence* of the variable in the
    /// environment, regardless of value (matching `NO_COLOR`'s own
    /// convention). Set `ANCHOR_LITESVM_COLOR` to any non-empty value to
    /// enable; unset it to disable.
    pub(super) fn detect() -> Self {
        if std::env::var_os("NO_COLOR").is_some() {
            return Self::Off;
        }
        if std::env::var_os("ANCHOR_LITESVM_COLOR").is_some() {
            return Self::On;
        }
        Self::Off
    }

    /// Wrap `s` with the green SGR code (or return it unchanged when
    /// `Off`). One allocation per call; not on a hot path.
    pub(super) fn green(self, s: &str) -> String {
        self.wrap("32", s)
    }

    /// Wrap `s` with the red SGR code.
    pub(super) fn red(self, s: &str) -> String {
        self.wrap("31", s)
    }

    /// Wrap `s` with the dim (faint) SGR code. Used for parenthetical
    /// status markers like `(no cu)` and `(truncated)` that carry
    /// information but shouldn't visually compete with the main glyph.
    pub(super) fn dim(self, s: &str) -> String {
        self.wrap("2", s)
    }

    fn wrap(self, sgr: &str, s: &str) -> String {
        match self {
            Self::Off => s.to_string(),
            // `\x1b[<sgr>m` opens, `\x1b[0m` resets. The reset is always
            // 0 (full reset) rather than "undo just this attribute" so
            // overlapping styles can't leak past their wrapper.
            Self::On => format!("\x1b[{sgr}m{s}\x1b[0m"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_returns_input_unchanged() {
        assert_eq!(Style::Off.green("✓"), "✓");
        assert_eq!(Style::Off.red("✗"), "✗");
        assert_eq!(Style::Off.dim("(no cu)"), "(no cu)");
    }

    #[test]
    fn on_wraps_with_sgr() {
        assert_eq!(Style::On.green("✓"), "\x1b[32m✓\x1b[0m");
        assert_eq!(Style::On.red("Error"), "\x1b[31mError\x1b[0m");
        assert_eq!(Style::On.dim("(no cu)"), "\x1b[2m(no cu)\x1b[0m");
    }
}
