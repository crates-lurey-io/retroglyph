//! Snapshot tests for the `23_burning_keep` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via `#[path]`, so
//! these tests exercise exactly what `cargo run --example 23_burning_keep` runs.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/23_burning_keep.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod burning_keep;

use burning_keep::BurningKeep;

#[test]
fn headless_snapshot() {
    // The headless/PNG paths both drive `tick` with the fixed synthetic `HEADLESS_FRAME_DELTA`
    // (see `render_headless_frames`'s doc comment), not a real clock, so the fire's flicker and
    // the dragons' wingbeats -- driven entirely by accumulated `elapsed`, per the module doc
    // comment -- land on the same frame every run. Unlike `20_overworld`'s ambient flourishes,
    // that makes this pinnable byte-for-byte despite never settling.
    insta::assert_snapshot!(support::headless_snapshot::<BurningKeep>(3));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<BurningKeep>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("23_burning_keep");
    let raw = support::capture_pty(&bin, b"", 25, 50, "burns in the night");
    let svg = support::svg_snapshot(&raw, 25, 50);

    // Not `insta::assert_snapshot!(svg)`: like `20_overworld`'s ambient flourishes, this scene's
    // fire/smoke/dragons animate for as long as the app runs with no settled state to park at
    // (unlike `06_layers`/`08_animation`, which finish a bounded motion and park), so the exact
    // frame the crossterm binary's unthrottled spin loop lands on by the time the PTY capture's
    // ready marker fires is real-wall-clock dependent. Assert on the chrome that layout, not the
    // clock, controls instead.
    for expected in ["burns in the night", "Escape quits"] {
        assert!(
            svg.contains(expected),
            "SVG output missing expected text {expected:?}"
        );
    }

    // Scratch, not `write_snapshot_file`, for the same reason `20_overworld` uses it: nothing
    // pins this SVG byte-for-byte, so a tracked copy would show a spurious diff on every run.
    let path = support::write_scratch_file("23_burning_keep.svg", svg.as_bytes());
    println!("wrote {} for visual review", path.display());
}
