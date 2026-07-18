//! Snapshot tests for the `18_text_align` example.
//!
//! Includes the example's own source (rather than reimplementing its logic)
//! via `#[path]`, so these tests exercise exactly what `cargo run --example
//! 18_text_align` runs.
//!
//! The headless text snapshot is the one that actually proves alignment here:
//! unlike color (which [`Headless::format_view`] can't see), a glyph's *column*
//! is exactly what the text view captures, so a title/readout drawn Left vs
//! Center vs Right lands in visibly different columns. The PNG/SVG snapshots
//! additionally pin the per-span colors of the aligned `PrintLine` through the
//! pixel and terminal-I/O render paths.
//!
//! [`Headless::format_view`]: retroglyph_core::Headless::format_view

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/18_text_align.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod text_align;

use text_align::TextAlign;

#[test]
fn headless_snapshot() {
    insta::assert_snapshot!(support::headless_snapshot::<TextAlign>(1));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<TextAlign>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("18_text_align");
    let raw = support::capture_pty(&bin, b"", 25, 50, "Left-aligned label");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("Text widget"),
        "SVG output missing expected panel content"
    );
    support::write_snapshot_file("18_text_align.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
