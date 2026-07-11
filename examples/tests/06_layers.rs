//! Snapshot tests for the `06_layers` example.
//!
//! Includes the example's own source (rather than reimplementing its logic)
//! via `#[path]`, so these tests exercise exactly what `cargo run --example
//! 06_layers` runs.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/06_layers.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod layers;

use layers::Layers;

#[test]
fn headless_snapshot() {
    // Three frames: the moving layer-1 glyph's column changes each tick while the
    // layer-0 background fill stays put underneath it -- pins both z-order (the
    // glyph draws over the fill) and transparency (the fill shows through every
    // other layer-1 cell, which stays the default empty tile).
    insta::assert_snapshot!(support::headless_snapshot::<Layers>(3));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<Layers>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    // The crossterm binary's event loop is a tight spin with no fixed frame rate (see
    // `04_mouse`'s test for the same caveat), so the moving glyph's column at any given
    // instant is machine-speed dependent. `06_layers` parks the glyph at the end of its
    // track once it gets there (see the example's own doc comment) specifically so this
    // capture has a stable frame to wait for: "(parked at track end)" only ever appears
    // once the animation has genuinely finished, giving a reproducible ready_marker.
    let bin = support::build_crossterm_example("06_layers");
    let raw = support::capture_pty(&bin, b"", 25, 50, "parked at track end");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("moving glyph"),
        "SVG output missing expected text"
    );
    support::write_snapshot_file("06_layers.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
