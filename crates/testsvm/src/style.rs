//! ANSI color styling, shared by the console CPI renders and the console report.
//!
//! Plain text by default; set `ANCHOR_LITESVM_COLOR` to wrap glyphs/markers in
//! ANSI SGR codes. `NO_COLOR` (no-color.org) always wins. An env var (not a
//! flag) because `cargo test` does not forward CLI flags into test code.

/// Whether to emit ANSI SGR codes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Style {
    Off,
    On,
}

impl Style {
    /// `NO_COLOR` wins; then `ANCHOR_LITESVM_COLOR` opts in; else off. Both are
    /// checked by presence, matching `NO_COLOR`'s convention.
    pub(crate) fn detect() -> Self {
        if std::env::var_os("NO_COLOR").is_some() {
            return Self::Off;
        }
        if std::env::var_os("ANCHOR_LITESVM_COLOR").is_some() {
            return Self::On;
        }
        Self::Off
    }

    pub(crate) fn green(self, s: &str) -> String {
        self.wrap("32", s)
    }

    pub(crate) fn red(self, s: &str) -> String {
        self.wrap("31", s)
    }

    pub(crate) fn dim(self, s: &str) -> String {
        self.wrap("2", s)
    }

    /// Bold (SGR 1), for headings and the report title.
    #[allow(dead_code)]
    pub(crate) fn bold(self, s: &str) -> String {
        self.wrap("1", s)
    }

    fn wrap(self, sgr: &str, s: &str) -> String {
        match self {
            Self::Off => s.to_string(),
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
        assert_eq!(Style::Off.bold("Title"), "Title");
    }

    #[test]
    fn on_wraps_with_sgr() {
        assert_eq!(Style::On.green("✓"), "\x1b[32m✓\x1b[0m");
        assert_eq!(Style::On.red("Error"), "\x1b[31mError\x1b[0m");
        assert_eq!(Style::On.dim("(no cu)"), "\x1b[2m(no cu)\x1b[0m");
        assert_eq!(Style::On.bold("Title"), "\x1b[1mTitle\x1b[0m");
    }
}
