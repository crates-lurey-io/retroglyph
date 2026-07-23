//! Integration tests for restricted/non-TTY terminal contexts (pipes, CI runners, `ssh` piped
//! through a non-interactive session, Nix sandboxes). `cargo test`'s own process already runs
//! with captured, non-TTY stdout/stderr, so these assertions hold under normal `cargo test`/CI
//! runs without any extra harness; they also degrade gracefully (see each test's comment) on a
//! developer machine where a real controlling terminal is still reachable via `/dev/tty`.

use retroglyph_core::backend::Backend;
use retroglyph_crossterm::Crossterm;
use std::io::IsTerminal;

/// `Crossterm::new()` must never panic in a restricted context: on a real non-TTY (no
/// controlling terminal reachable at all, e.g. a detached CI runner), raw-mode/terminal setup
/// fails and this returns a clean `Err`. On a machine where a controlling terminal is still
/// reachable via `/dev/tty` despite this process's own stdio being redirected (common when
/// running tests from an interactive shell), construction can still succeed; either outcome is
/// acceptable here, since the only invariant under test is "no panic".
#[test]
fn new_does_not_panic_when_terminal_unavailable() {
    match Crossterm::new() {
        Ok(term) => {
            // A real controlling terminal was still reachable; drop it to restore any state
            // (raw mode/alternate screen) this test's construction changed.
            drop(term);
        }
        Err(err) => {
            // Constructing over a restricted/non-TTY context must fail cleanly, not panic.
            // Any `io::Error` is acceptable; the point is that this arm is reachable at all
            // without a panic unwinding out of `new()`.
            let _ = err;
        }
    }
}

/// Same as [`new_does_not_panic_when_terminal_unavailable`], but exercised twice in the same
/// process to make sure the process-wide panic hook (registered via `std::sync::Once` on first
/// construction) doesn't itself misbehave or panic on a repeated/failed initialization attempt.
#[test]
fn repeated_construction_does_not_panic() {
    for _ in 0..2 {
        match Crossterm::new() {
            Ok(term) => drop(term),
            Err(err) => {
                let _ = err;
            }
        }
    }
}

/// `Backend::size()` must never panic and must fall back to a sane default when the underlying
/// `crossterm::terminal::size()` query fails (no controlling terminal, e.g. piped/CI stdio).
/// This exercises the fallback directly against a constructed backend when one is available;
/// on a restricted context where construction itself fails, this test is skipped since there's no
/// `Backend` instance to query (covered instead by `new_does_not_panic_when_terminal_unavailable`
/// above, and by the crate's own `Backend::size()` implementation using `unwrap_or((80, 25))`).
#[test]
fn size_falls_back_instead_of_panicking() {
    if let Ok(mut term) = Crossterm::new() {
        let size = term.size();
        assert!(size.width > 0, "fallback width must be nonzero");
        assert!(size.height > 0, "fallback height must be nonzero");
        // Drawing an empty iterator must be a no-op, not a panic, even in a context where the
        // underlying terminal is unusual (e.g. a pipe masquerading as a TTY under `script`/`pty`
        // test harnesses). Reaching this line at all (rather than unwinding) is the assertion;
        // either `Result` variant is an acceptable outcome.
        let _ = term.draw(std::iter::empty());
    }
}

/// `CrosstermOptions::build()`/`Crossterm::new()` must wire up pipe-safe plain mode (see
/// `retroglyph_terminal::TerminalRenderer::auto`) based on whether the real process stdout is
/// an interactive terminal, rather than always leaving ANSI/SGR escapes on. Under `cargo test`
/// (default, captured) this process's real stdout is a pipe, not a TTY, so `plain_mode()` should
/// be `true`; run with `--nocapture` on a machine with a real controlling terminal still
/// reachable, it could observe the other outcome instead -- either way, the assertion compares
/// against a fresh, independent `is_terminal()` check so it holds under both.
#[test]
fn build_auto_detects_plain_mode_from_real_stdout() {
    if let Ok(term) = Crossterm::new() {
        let expected_plain = !std::io::stdout().is_terminal();
        assert_eq!(
            term.plain_mode(),
            expected_plain,
            "Crossterm::new()'s plain mode must track stdout's actual terminal status"
        );
    }
}
