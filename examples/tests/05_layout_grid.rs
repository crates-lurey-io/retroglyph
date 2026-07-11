//! Snapshot tests for the `05_layout_grid` example.
//!
//! Includes the example's own source (rather than reimplementing its logic)
//! via `#[path]`, so these tests exercise exactly what `cargo run --example
//! 05_layout_grid` runs.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/05_layout_grid.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod layout_grid;

use layout_grid::LayoutGrid;

#[test]
fn headless_snapshot() {
    insta::assert_snapshot!(support::headless_snapshot::<LayoutGrid>(1));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<LayoutGrid>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("05_layout_grid");
    let raw = support::capture_pty(&bin, b"", 25, 50, "Pane D");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("Pane A"),
        "SVG output missing expected pane label"
    );
    support::write_snapshot_file("05_layout_grid.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
