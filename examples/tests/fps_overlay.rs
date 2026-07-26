//! End-to-end coverage for the FPS overlay's default and its runtime toggle, driven through a real
//! PTY so the whole path is exercised: the driver draining the backend's queue, consuming its own
//! key, re-pushing everything else, and the example still seeing an untouched event stream.
//!
//! The default is the point of the first test: the overlay was originally behind an opt-in `fps`
//! Cargo feature, so nothing that ran an example the documented way (`cargo run --example ...
//! --features crossterm`) ever saw it. Every other test in this directory routes through
//! [`support::capture_pty`], which pins `RG_FPS=0` so the committed SVG snapshots stay
//! reproducible -- so without this file, a regression back to "off unless you know a flag" would
//! be invisible to the whole suite.
//!
//! Nothing here re-tests that non-toggle input still reaches the example after the driver drains
//! the queue looking for its own key: `17_theme_switch`'s existing `svg_snapshot` sends a real `t`
//! through the same path and snapshots the themed result, so it fails loudly if the re-push ever
//! breaks.

#![cfg(not(target_arch = "wasm32"))]
// See `01_hello_world.rs`'s copy of this allow for the rationale: `--test` targets have no
// external consumers, so `support`'s `pub` items are unreachable by construction.
#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

/// Any example works here; `01_hello_world` is the cheapest to build, settles immediately, and
/// quits only on `q`/Escape, so the toggle key can't be mistaken for a quit.
const EXAMPLE: &str = "01_hello_world";
const READY_MARKER: &str = "Hello, world!";
const ROWS: u16 = 25;
const COLS: u16 = 50;

/// The overlay's toggle key. Written into the PTY the same way a person would press it.
const TOGGLE: &[u8] = b"`";

/// Only the fixed parts of `NNN fps  MM.M ms  <backend>`: the numbers are wall-clock dependent,
/// and crossterm's driver is an unthrottled spin loop, so neither is pinnable.
const READOUT_PARTS: [&str; 3] = ["fps", "ms", "crossterm"];

/// Captures a run of [`EXAMPLE`] with `input` typed into it, as SVG.
///
/// `settled` is the handoff to `capture_pty_until`: it decides when the typed key has visibly
/// landed, so the harness doesn't send the quit key while the toggle is still in flight. Without
/// it the two can be read in the same frame, and the driver never presents the quit frame -- see
/// [`support::capture_pty_until`] for the full failure.
fn run(input: &[u8], env: &[(&str, &str)], settled: &dyn Fn(&str) -> bool) -> String {
    let bin = support::build_crossterm_example(EXAMPLE);
    let raw = support::capture_pty_until(&bin, input, ROWS, COLS, READY_MARKER, env, settled);
    let svg = support::svg_snapshot(&raw, ROWS, COLS);
    assert!(
        svg.contains(READY_MARKER),
        "the example itself should always render"
    );
    svg
}

/// Whether the FPS readout is on `screen`, for the settle waits below. Matches `capture_pty_until`
/// against the same `"fps"` substring the assertions use, so "the harness saw it land" and "the
/// test accepts it" can't drift apart.
fn overlay_visible(screen: &str) -> bool {
    screen.contains("fps")
}

#[test]
fn overlay_is_drawn_without_opting_in() {
    // No `RG_FPS` at all: exactly what a plain `cargo run --example ... --features crossterm` gets.
    let svg = run(b"", &[], &|_| true);
    for expected in READOUT_PARTS {
        assert!(
            svg.contains(expected),
            "FPS overlay missing {expected:?} from a default run"
        );
    }
}

#[test]
fn the_toggle_key_hides_a_visible_overlay() {
    let svg = run(TOGGLE, &[], &|screen| !overlay_visible(screen));
    assert!(
        !svg.contains("fps"),
        "the toggle key should have hidden the overlay"
    );
}

#[test]
fn the_toggle_key_shows_an_overlay_that_started_hidden() {
    // `RG_FPS=0` picks the starting state; it is not a lock.
    let svg = run(TOGGLE, &[("RG_FPS", "0")], &overlay_visible);
    for expected in READOUT_PARTS {
        assert!(
            svg.contains(expected),
            "FPS overlay missing {expected:?} after toggling a hidden overlay on"
        );
    }
}

#[test]
fn rg_fps_0_starts_hidden() {
    let svg = run(b"", &[("RG_FPS", "0")], &|_| true);
    assert!(
        !svg.contains("fps"),
        "RG_FPS=0 should start with the overlay hidden"
    );
}
