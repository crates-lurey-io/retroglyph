//! Snapshot tests for the `22_random_tiles` example.
//!
//! Includes the example's own source (rather than reimplementing its logic)
//! via `#[path]`, so these tests exercise exactly what `cargo run --example
//! 22_random_tiles` runs.

// `unreachable_pub`: every `pub` item in this test binary (in `support` and
// in the included example module) is unreachable from other crates by
// construction -- `--test` targets have no external consumers, ever. The
// items still need at least `pub(crate)`-equivalent visibility to cross the
// module boundary between this file and the `#[path]`-included `support`/
// `random_tiles` modules, which is exactly what the lint is (correctly, for
// a lib) warning about here.
#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/22_random_tiles.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod random_tiles;

use random_tiles::RandomTiles;

#[test]
fn headless_snapshot() {
    // Every cell is randomized every frame, but the PRNG is reseeded from the frame number
    // (not wall-clock time or carried-over state -- see the example's top doc comment), so two
    // frames from the same run render two different, but individually reproducible, grids.
    insta::assert_snapshot!(support::headless_snapshot::<RandomTiles>(2));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<RandomTiles>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("22_random_tiles");
    // The randomized grid itself has no fixed marker to wait on, unlike every other example's
    // title/legend text -- `wait_for_marker` needs *something* in the captured bytes, so this
    // waits on the ANSI SGR truecolor escape (`\x1b[38;2;`) every styled cell emits, which
    // appears on the very first frame.
    let raw = support::capture_pty(&bin, b"", 25, 50, "\u{1b}[38;2;");
    let svg = support::svg_snapshot(&raw, 25, 50);

    // Not `insta::assert_snapshot!(svg)`, unlike every other example's `svg_snapshot`: like
    // `20_overworld`'s ambient animations, this example's whole point is re-randomizing every
    // cell for as long as it runs, with no settled state to park at. Unlike the headless/PNG
    // snapshots above (which drive a fixed, known number of `tick`s and so land on a
    // reproducible frame), the crossterm binary here is a real unthrottled spin loop (see
    // `run_blocking`), so the exact frame this capture lands on -- and therefore its exact
    // glyphs and colors -- is real-wall-clock dependent and not reproducible byte-for-byte.
    // Assert on the shape of the output instead: a full grid of individually, truly-colored
    // cells, which is what this example actually proves.
    let tspan_count = svg.matches("<tspan").count();
    assert!(
        tspan_count > 25 * 40,
        "expected close to one styled run per grid cell (50x25 = 1250), got {tspan_count} <tspan> runs"
    );
    assert!(
        svg.contains("fill=\"#"),
        "expected truecolor RGB fills in the SVG"
    );

    // Scratch, not `write_snapshot_file`, for the same reason as `20_overworld`: nothing here
    // is pinned byte-for-byte, so a tracked copy would be rewritten with different bytes on
    // every `cargo test` run and show up as a spurious diff in whatever commit came next.
    let path = support::write_scratch_file("22_random_tiles.svg", svg.as_bytes());
    println!("wrote {} for visual review", path.display());
}
