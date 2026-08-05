//! End-to-end and snapshot coverage for the perf overlay: its default visibility, its runtime
//! toggle key, and what it actually renders in both modes.
//!
//! The default is the point of the first PTY test: the overlay was originally behind an opt-in
//! `fps` Cargo feature, so nothing that ran an example the documented way (`cargo run --example
//! ... --features crossterm`) ever saw it. Every other PTY test in this directory routes through
//! [`support::capture_pty`], which pins `RG_FPS=0` so the committed SVG snapshots stay
//! reproducible -- so without this file, a regression back to "off unless you know a flag" would
//! be invisible to the whole suite.
//!
//! The toggle key cycles `Off -> Compact -> Full -> Off`
//! ([`retroglyph_ui::PerfOverlayMode`]); the PTY tests below only prove the first step of
//! that cycle from both the hidden (`Off`) and visible (`Compact`) default starting states (real
//! key presses through a live terminal are the thing worth end-to-end coverage, not an
//! exhaustive re-walk of the state machine, which
//! `retroglyph_ui`'s own `perf` unit tests already cover deterministically).
//! `svg_snapshot_compact`/`svg_snapshot_full` and `png_snapshot_compact`/`png_snapshot_full`
//! commit reviewable artifacts (`tests/snapshots/fps_overlay_*.svg`, insta-managed PNGs) of both
//! modes as actually rendered, the same way every numbered example's own snapshot tests do.
//!
//! Nothing here re-tests that non-toggle input still reaches the example after the driver drains
//! the queue looking for its own key: `17_theme_switch`'s existing `svg_snapshot` sends a real `t`
//! through the same path and snapshots the themed result, so it fails loudly if the re-push ever
//! breaks.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[path = "../examples/01_hello_world.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod hello_world;

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
use hello_world::HelloWorld;

/// Any example works here; `01_hello_world` is the cheapest to build, settles immediately, and
/// quits only on `q`/Escape, so the toggle key can't be mistaken for a quit.
#[cfg(not(target_arch = "wasm32"))]
const EXAMPLE: &str = "01_hello_world";
#[cfg(not(target_arch = "wasm32"))]
const READY_MARKER: &str = "Hello, world!";
#[cfg(not(target_arch = "wasm32"))]
const ROWS: u16 = 25;
#[cfg(not(target_arch = "wasm32"))]
const COLS: u16 = 50;

/// The overlay's toggle key. Written into the PTY the same way a person would press it.
#[cfg(not(target_arch = "wasm32"))]
const TOGGLE: &[u8] = b"`";

/// Only the fixed parts of `NNNfps MM.Mms minMM.M maxMM.M <backend>`: the numbers are wall-clock
/// dependent, and crossterm's driver is an unthrottled spin loop, so neither is pinnable.
#[cfg(not(target_arch = "wasm32"))]
const READOUT_PARTS: [&str; 3] = ["fps", "ms", "crossterm"];

/// Captures a run of [`EXAMPLE`] with `input` typed into it, as SVG.
///
/// `settled` is the handoff to `capture_pty_until`: it decides when the typed key has visibly
/// landed, so the harness doesn't send the quit key while the toggle is still in flight. Without
/// it the two can be read in the same frame, and the driver never presents the quit frame -- see
/// [`support::capture_pty_until`] for the full failure.
#[cfg(not(target_arch = "wasm32"))]
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

/// Whether the perf overlay is on `screen` at all (`Compact` or `Full`), for the settle waits
/// below. Matches `capture_pty_until` against the same `"fps"` substring the assertions use, so
/// "the harness saw it land" and "the test accepts it" can't drift apart.
#[cfg(not(target_arch = "wasm32"))]
fn overlay_visible(screen: &str) -> bool {
    screen.contains("fps")
}

/// Whether the richer `Full` mode specifically (not just `Compact`) is on `screen`: its bordered
/// panel's top-left corner glyph, which `Compact`'s single-row readout never draws.
#[cfg(not(target_arch = "wasm32"))]
fn full_mode_visible(screen: &str) -> bool {
    screen.contains('┌')
}

/// Also a `Compact`-mode visual-review artifact, written to `CARGO_TARGET_TMPDIR` (not committed:
/// see [`support::write_scratch_file`]) rather than insta-tracked, because the overlay's whole
/// point is a live, changing readout -- an fps/ms number or a sparkline bar height would fail this
/// assertion on every single run if pinned byte-for-byte, unlike every other example's snapshot,
/// which shows genuinely static content. [`png_snapshot_compact`] below is the byte-stable,
/// insta-tracked counterpart, via a synthetic (not wall-clock) frame delta.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn overlay_is_drawn_without_opting_in() {
    // No `RG_FPS` at all: exactly what a plain `cargo run --example ... --features crossterm` gets.
    let svg = run(b"", &[], &|_| true);
    for expected in READOUT_PARTS {
        assert!(
            svg.contains(expected),
            "perf overlay missing {expected:?} from a default run"
        );
    }
    assert!(
        !svg.contains('┌'),
        "a default run should start at Compact, not Full"
    );
    let path = support::write_scratch_file("fps_overlay_compact.svg", svg.as_bytes());
    println!("wrote {}", path.display());
}

/// Also a `Full`-mode visual-review artifact -- see [`overlay_is_drawn_without_opting_in`]'s docs
/// for why this is a scratch file, not an insta-tracked one. [`png_snapshot_full`] below is the
/// byte-stable, insta-tracked counterpart.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_toggle_key_advances_a_visible_overlay_from_compact_to_full() {
    let svg = run(TOGGLE, &[], &full_mode_visible);
    assert!(
        svg.contains('┌'),
        "one toggle press from the default (Compact) should reach Full, {svg}"
    );
    // Full still carries the same readout Compact does, composed via `retroglyph-ui`.
    for expected in READOUT_PARTS {
        assert!(svg.contains(expected), "Full mode missing {expected:?}");
    }
    let path = support::write_scratch_file("fps_overlay_full.svg", svg.as_bytes());
    println!("wrote {}", path.display());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_toggle_key_shows_an_overlay_that_started_hidden() {
    // `RG_FPS=0` picks the starting state; it is not a lock.
    let svg = run(TOGGLE, &[("RG_FPS", "0")], &overlay_visible);
    for expected in READOUT_PARTS {
        assert!(
            svg.contains(expected),
            "perf overlay missing {expected:?} after toggling a hidden overlay on"
        );
    }
    assert!(
        !svg.contains('┌'),
        "one toggle press from Off should land on Compact, not Full"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn rg_fps_0_starts_hidden() {
    let svg = run(b"", &[("RG_FPS", "0")], &|_| true);
    assert!(
        !svg.contains("fps"),
        "RG_FPS=0 should start with the overlay hidden"
    );
}

/// `render_perf_overlay_rgb`'s standard grid: the same 50x25-at-2x every windowed example in the
/// gallery uses by default (see `run_software`), so this snapshot reflects what a real example
/// actually renders, not an arbitrarily chosen size.
#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
const PNG_COLS: u16 = 50;
#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
const PNG_ROWS: u16 = 25;
#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
const PNG_SCALE: u8 = 2;
/// Enough settled frames for `FrameStats`' 120-sample window to carry real (if synthetic, fixed
/// per-frame delta) data, so `Full` mode's sparkline draws something instead of an empty row.
#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
const PNG_SETTLE_FRAMES: u32 = 40;

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot_compact() {
    let (width, height, rgb) = retroglyph_examples::render_perf_overlay_rgb::<HelloWorld>(
        PNG_COLS,
        PNG_ROWS,
        PNG_SCALE,
        PNG_SETTLE_FRAMES,
        0, // no toggle presses: stays at the default Compact mode
    );
    let png = support::encode_png(width, height, &rgb);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot_full() {
    let (width, height, rgb) = retroglyph_examples::render_perf_overlay_rgb::<HelloWorld>(
        PNG_COLS,
        PNG_ROWS,
        PNG_SCALE,
        PNG_SETTLE_FRAMES,
        1, // one toggle press: Compact -> Full
    );
    let png = support::encode_png(width, height, &rgb);
    insta::assert_binary_snapshot!(".png", png);
}
