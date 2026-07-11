//! Snapshot tests for the `01_hello_world` example.
//!
//! Includes the example's own source (rather than reimplementing its logic)
//! via `#[path]`, so these tests exercise exactly what `cargo run --example
//! 01_hello_world` runs.

// `unreachable_pub`: every `pub` item in this test binary (in `support` and
// in the included example module) is unreachable from other crates by
// construction -- `--test` targets have no external consumers, ever. The
// items still need at least `pub(crate)`-equivalent visibility to cross the
// module boundary between this file and the `#[path]`-included `support`/
// `hello_world` modules, which is exactly what the lint is (correctly, for a
// lib) warning about here.
#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/01_hello_world.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod hello_world;

use hello_world::HelloWorld;

#[test]
fn headless_snapshot() {
    insta::assert_snapshot!(support::headless_snapshot::<HelloWorld>(1));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<HelloWorld>(50, 25, 2);
    // Binary snapshot (insta core, no extra dependency): byte-for-byte
    // compared against the committed `01_hello_world.png`, so a pixel
    // regression in the software renderer actually fails this test instead
    // of silently passing (the previous `write_snapshot_file` +
    // `assert!(!png.is_empty())` version only ever checked the render
    // produced *something*, never that it matched a known-good image).
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("01_hello_world");
    let raw = support::capture_pty(&bin, b"", 25, 50, "Hello, world!");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("Hello, world!"),
        "SVG output missing expected text"
    );
    support::write_snapshot_file("01_hello_world.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
