//! Snapshot tests for the `18_weighted_fill` example.
//!
//! Includes the example's own source (rather than reimplementing its logic)
//! via `#[path]`, so these tests exercise exactly what `cargo run --example
//! 18_weighted_fill` runs.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/18_weighted_fill.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod weighted_fill;

use weighted_fill::WeightedFill;

#[test]
fn headless_snapshot() {
    insta::assert_snapshot!(support::headless_snapshot::<WeightedFill>(1));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<WeightedFill>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("18_weighted_fill");
    let raw = support::capture_pty(&bin, b"", 25, 50, "Max(12)");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("Fill(2)"),
        "SVG output missing expected pane label"
    );
    support::write_snapshot_file("18_weighted_fill.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
