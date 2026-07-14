//! ANSI color gating (RFC-0008 CLI9).
//!
//! Color is enabled only when stdout is a terminal and `NO_COLOR` is unset. A
//! non-TTY (piped/redirected) or `NO_COLOR=1` both produce ANSI-free output, so
//! captured or machine-consumed output is never polluted with escape codes.

use std::io::IsTerminal;

/// Whether ANSI color should be enabled, from the given signals. Pure so it can
/// be tested without touching the real terminal/environment.
#[must_use]
pub fn resolve(stdout_is_tty: bool, no_color_set: bool) -> bool {
    stdout_is_tty && !no_color_set
}

/// Whether ANSI color should be enabled for the current process: stdout is a
/// terminal and `NO_COLOR` is unset.
#[must_use]
pub fn enabled() -> bool {
    resolve(
        std::io::stdout().is_terminal(),
        std::env::var_os("NO_COLOR").is_some(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_gating_matrix() {
        assert!(resolve(true, false)); // TTY, no NO_COLOR -> colored
        assert!(!resolve(true, true)); // NO_COLOR set -> plain
        assert!(!resolve(false, false)); // not a TTY -> plain
        assert!(!resolve(false, true)); // both -> plain
    }
}
