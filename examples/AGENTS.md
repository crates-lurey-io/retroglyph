# AGENTS.md (examples)

`examples/examples/*.rs` is dual-purpose, not documentation-only: `.github/workflows/docs.yml`
builds every file here to four WASM variants (headless text, xterm.js terminal, software canvas,
WebGL2 canvas) and deploys them to the docs gallery, and each example's three committed snapshots
pin its rendered output as a cross-backend regression suite. A change that breaks color mapping,
event decoding, or layer compositing on any backend fails
`cargo test -p retroglyph-examples --all-features` before it fails a user's build. Because every
example carries a real, measured CI cost (four WASM builds plus a snapshot triple), adding one is a
deliberate decision, not "the more the better."

## Per-example validation gates

Every example must pass all of the following before merge:

1. Compiles for all four WASM variants (`wasm-headless`, `wasm-terminal`, `software`, `gl`); a
   failure here blocks the docs deploy workflow, so verify locally before pushing.
2. Builds and runs on all native backends (`--features crossterm`, `--features software`,
   `--features gl`, and the headless-stdout fallback with no backend feature enabled).
3. All three snapshots are committed (headless text via `insta`, software PNG, crossterm SVG), and
   `cargo test -p retroglyph-examples --all-features` is green.
4. Any backend-specific capability gap degrades visibly (an on-screen note) rather than panicking or
   rendering a blank frame -- verified by hand for examples that have a fallback path. `04_mouse` is
   the reference implementation: when motion reporting is unavailable, it falls back to click-only
   tracking with an on-screen note instead of failing or blanking.
5. `cargo run -p retroglyph-examples --bin runner` lists the example (it discovers
   `examples/examples/*.rs` at runtime, so this is a smoke check that the file compiles and is in
   the right place) and can launch it on each backend.
6. `just check` is green (fmt, clippy including `pedantic`/`nursery`, compile, tests, doc, llms).
7. The example carries a top doc comment stating what it proves and how to run it.

## Conventions

- **Naming:** zero-padded `NN_name.rs` under `examples/examples/`, sorted so
  `cargo run -p retroglyph-examples --bin runner`'s discovery/listing reads in order.
- **Shape:** one file, `#[derive(Default)]` state where the example allows it, `impl Example`,
  terminated with `retroglyph_examples::example_main!(Type)`. A sibling `examples/tests/NN_name.rs`
  produces the three snapshots through `examples/tests/support`.
- **Size:** simple, single-capability examples (colors, keyboard, mouse, layout, layers) stay around
  ~150 lines, including the top doc comment, so they read as copy-paste templates, not applications.
  Multi-capability showcases (widgets dashboards, tileset/animation demos) run closer to ~300 lines;
  don't force a genuine capability proof into the smaller ceiling at the cost of readability. Small
  game examples are not size-constrained the same way, but should still stay focused on the
  mechanics they're demonstrating rather than growing into a full application.
- **Headless snapshots for input-driven examples** drive synthetic events through
  `Headless::push_event` rather than snapshotting an idle/legend frame -- this is what actually
  proves decode-and-echo correctness, and what exercises the WASM `decode_key`/`decode_mouse` FFI
  paths through a real example rather than only through their unit tests.
- **Widgets dependency:** `retroglyph-widgets` is a plain unconditional dependency of this crate
  (like `retroglyph-core`), not behind a Cargo feature. Widgets is backend-generic, so there is no
  backend axis to gate it on.

## No perf/benchmark examples here

Performance measurement and this crate's docs-gallery/regression-suite purpose are different
concerns and should not be conflated: an example is judged by its committed snapshots (does it
render correctly), not by a throughput number, and the snapshot harness (`insta`, PNG, SVG) is not
built to gate on performance. A perf-regression benchmark belongs in a `cargo bench`/criterion
setup, not as an entry in this crate.
