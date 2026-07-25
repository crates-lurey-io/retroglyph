//! End-to-end coverage for the FPS overlay's *default*, which is the whole point of it existing:
//! it was originally behind an opt-in `fps` Cargo feature, so nothing that ran an example the
//! documented way (`cargo run --example ... --features crossterm`) ever saw it.
//!
//! Every other test in this directory routes through [`support::capture_pty`], which pins
//! `RG_FPS=0` so the committed SVG snapshots stay reproducible -- so without this file, a
//! regression back to "off unless you know a flag" would be invisible to the whole suite.

#![cfg(not(target_arch = "wasm32"))]
// See `01_hello_world.rs`'s copy of this allow for the rationale: `--test` targets have no
// external consumers, so `support`'s `pub` items are unreachable by construction.
#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

/// Any example works here; `01_hello_world` is the cheapest to build and settles immediately.
const EXAMPLE: &str = "01_hello_world";
const READY_MARKER: &str = "Hello, world!";
const ROWS: u16 = 25;
const COLS: u16 = 50;

#[test]
fn overlay_is_drawn_without_opting_in() {
    let bin = support::build_crossterm_example(EXAMPLE);
    // No `RG_FPS` at all: exactly what a plain `cargo run --example ... --features crossterm` gets.
    let raw = support::capture_pty_with_env(&bin, b"", ROWS, COLS, READY_MARKER, &[]);
    let svg = support::svg_snapshot(&raw, ROWS, COLS);

    // Only the fixed parts of `NNN fps  MM.M ms  <backend>` -- the numbers are wall-clock
    // dependent, and crossterm's driver is an unthrottled spin loop, so neither is pinnable.
    for expected in ["fps", "ms", "crossterm"] {
        assert!(
            svg.contains(expected),
            "FPS overlay missing {expected:?} from a default run"
        );
    }
}

#[test]
fn rg_fps_0_suppresses_the_overlay() {
    let bin = support::build_crossterm_example(EXAMPLE);
    let raw =
        support::capture_pty_with_env(&bin, b"", ROWS, COLS, READY_MARKER, &[("RG_FPS", "0")]);
    let svg = support::svg_snapshot(&raw, ROWS, COLS);

    assert!(
        svg.contains(READY_MARKER),
        "the example itself should still render"
    );
    assert!(
        !svg.contains("fps"),
        "RG_FPS=0 should suppress the overlay entirely"
    );
}
