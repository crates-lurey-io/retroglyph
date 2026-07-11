//! Snapshot tests for the `08_animation` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example
//! 08_animation` runs.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/08_animation.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod animation;

use animation::Animation;

#[test]
fn headless_snapshot() {
    // 12 frames at the headless harness's fixed 100ms delta (1.2s of simulated time): pins
    // `Easing::EaseInOutCubic`'s non-linear progress across the track (whole-cell fallback,
    // since headless ignores `put_offset`'s sub-cell dx) through the first bounce at the 1s
    // mark, and the first couple of frames heading back left afterward. The full round trip
    // (2s) and the resulting park at the left end are past this snapshot's window -- see
    // `svg_snapshot` below for a capture of that settled state instead.
    insta::assert_snapshot!(support::headless_snapshot::<Animation>(12));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<Animation>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    // Real-time-paced like `06_layers` (see that test's own comment on why the crossterm
    // binary's unthrottled event loop needs a stable end state, not an arbitrary mid-motion
    // frame, to capture deterministically): `08_animation` parks at the left end once its one
    // round trip finishes, so "(parked at left end)" is a marker that only ever appears once
    // the animation has genuinely settled.
    let bin = support::build_crossterm_example("08_animation");
    let raw = support::capture_pty(&bin, b"", 25, 50, "parked at left end");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("travels the track"),
        "SVG output missing expected text"
    );
    support::write_snapshot_file("08_animation.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
