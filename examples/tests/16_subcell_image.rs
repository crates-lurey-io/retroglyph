//! Snapshot tests for the `16_subcell_image` example.
//!
//! Includes the example's own source (rather than reimplementing its logic)
//! via `#[path]`, so these tests exercise exactly what `cargo run --example
//! 16_subcell_image` runs.

// `unreachable_pub`: every `pub` item in this test binary (in `support` and
// in the included example module) is unreachable from other crates by
// construction -- `--test` targets have no external consumers, ever. The
// items still need at least `pub(crate)`-equivalent visibility to cross the
// module boundary between this file and the `#[path]`-included `support`/
// `subcell_image` modules, which is exactly what the lint is (correctly,
// for a lib) warning about here.
#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/16_subcell_image.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod subcell_image;

use subcell_image::SubcellImage;

#[test]
fn headless_snapshot() {
    // Static content (the procedural scene never changes frame to frame), so one frame is
    // enough -- unlike an animated example, additional frames would only duplicate identical
    // text.
    insta::assert_snapshot!(support::headless_snapshot::<SubcellImage>(1));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<SubcellImage>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("16_subcell_image");
    let raw = support::capture_pty(&bin, b"", 25, 50, "Sextant");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(svg.contains("Sextant"), "SVG output missing expected text");
    support::write_snapshot_file("16_subcell_image.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
