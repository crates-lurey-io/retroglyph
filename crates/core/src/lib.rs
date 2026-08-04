//! retroglyph-core: the `no_std`-compatible foundation of retroglyph.
//!
//! Grid, tile, style, color, text, terminal, and event types, plus the
//! [`Output`](crate::backend::Output)/[`Input`](crate::backend::Input)/[`Cursor`](crate::backend::Cursor) backend
//! facets (bundled together as [`Backend`](crate::backend::Backend)) and the dependency-free
//! [`Headless`](crate::backend::Headless) test backend, and the `App`/`Flow`/`Frame` game loop contract.
//! Platform backends (`retroglyph-crossterm`, `retroglyph-software`) and drawing helpers
//! (`retroglyph-widgets`) are separate crates that depend on this one.
//!
//! # Features
//!
//! <!-- gen-features:start -->
//! Default features: `egc`, `indexed-quant`, `std`.
//!
//! ### `dev`
//!
//! ⚪ Optional.
//!
//! Forces `BuildMode::Dev` on in a build that would otherwise resolve to `Release`.
//!
//! Can be used so an optimized build still reports development diagnostics (see the [`dev`]
//! module).
//!
//! ### `egc`
//!
//! 🟢 Enabled by default.
//!
//! Enables grapheme-cluster-aware text handling (via `unicode-segmentation`) for EGC-correct cell
//! diffing and layout.
//!
//! ### `indexed-quant`
//!
//! 🟢 Enabled by default.
//!
//! Gates perceptual (Oklab) RGB → Indexed/ANSI quantization (`gem/space`) and [`Color`](crate::color::Color)'s
//! `gem`-space conversions (`to_srgb`/`from_srgb`/`lerp`/`from_hex`).
//!
//! Without it, [`Color::to_indexed`](crate::color::Color::to_indexed)/
//! [`Color::to_ansi`](crate::color::Color::to_ansi) fall back to euclidean RGB cube-mapping instead of
//! failing to compile.
//!
//! This is a capability flag, not a backend: it only turns on `gem`'s `space` module, whose float
//! math the crate's mandatory `std`-or-`libm` backend already supplies.
//!
//! ### `libm`
//!
//! ⚪ Optional.
//!
//! Uses `libm`'s software float implementation (`roundf`/`fmaf`/`sinf`/`cosf`/`powf`) for
//! `animate`'s easing curves and the separable [`BlendMode`](crate::grid::BlendMode) channel math, via
//! this crate's own `math` shim -- the `no_std` side of that split. See `std` below for the alternative that prefers
//! the platform's own float intrinsics when available; a build needs exactly one of the two.
//!
//! ### `libm-arch`
//!
//! ⚪ Optional.
//!
//! Alias for `libm`, matching `gem`'s and `alpha-blend`'s own `libm-arch` feature name so a reader
//! following their docs finds the name they expect. Already implied by `libm` above (which always
//! requests the `arch`-intrinsified `libm` dependency; see the `[dependencies.libm]` comment
//! below), so this exists purely for discoverability and never needs to be enabled on its own.
//!
//! ### `serde`
//!
//! ⚪ Optional.
//!
//! Adds `Serialize`/`Deserialize` impls for [`Color`](crate::color::Color), [`Style`](crate::color::Style), `Size`,
//! `Offset`, and (via `ixy`) `Pos`/`Rect`, so a config file can round-trip a saved camera position,
//! window geometry, sub-cell pixel offset, or theme color.
//!
//! [`Color`](crate::color::Color) serializes through its `Display`/`FromStr` round trip (e.g. `"bright-red"`,
//! `"#ff8000"`) rather than a derived structural form, so hand-edited TOML/JSON stays legible.
//!
//! ### `std`
//!
//! 🟢 Enabled by default.
//!
//! Enables `gem/std` and `alpha-blend/std`, and uses `std`'s float intrinsics (via this crate's
//! `math` shim) instead of `libm`'s software implementation for `animate` and the separable
//! [`BlendMode`](crate::grid::BlendMode) channel math.
//!
//! Disabling this feature (`--no-default-features`) builds this crate `no_std`, and then needs
//! `libm` above as the float backend instead: see the crate-level `compile_error!` in `src/lib.rs`.
//!
//! ### `testing`
//!
//! ⚪ Optional.
//!
//! Enables `testing`'s `TestHarness`, which drives an [`App`](crate::app::App) against
//! [`Headless`](crate::backend::Headless) for tests, with
//! synthetic input queuing and frame-settling helpers.
//!
//! Test-only surface, `no_std` + `alloc` compatible, off by default so it never ships in a release
//! build by accident.
//! <!-- gen-features:end -->
//!
//! # Architecture
//!
//! [`Terminal<B>`](crate::terminal::Terminal) owns a double-buffered [`Grid`](crate::grid::Grid) and the
//! [`Backend`](crate::backend::Backend) lifecycle (resize, present, events). Drawing itself goes entirely
//! through [`Surface`](crate::surface::Surface), handed out by
//! [`Terminal::draw`](crate::terminal::Terminal::draw)/[`Terminal::surface`](crate::terminal::Terminal::surface):
//! a game calls `term.draw(|s| { s.put(...); ... })`
//! once per frame, and [`present`](crate::terminal::Terminal::present) diffs the current frame against the
//! previous one, sending only changed cells to the [`Backend`](crate::backend::Backend). `B` is the only thing that
//! changes between a headless test and a real window or terminal:
//!
//! ```text
//!               ┌───────────────────────────┐
//!               │      App::update(...)      │  game logic, once, generic over B
//!               └──────────────┬─────────────┘
//!                              │ term.draw(|s| ...): writes through Surface
//!                              ▼
//!               ┌───────────────────────────┐
//!               │       Terminal<B>          │  double-buffered Grid, cell diff
//!               └──────────────┬─────────────┘
//!                              │ draw / draw_layers / poll_event
//!                              ▼
//!               ┌───────────────────────────┐
//!               │  B: Output + Input + Cursor │  the only piece that swaps out
//!               └──────────────┬─────────────┘
//!                              │
//!        ┌─────────────────────┼─────────────────────┐
//!        ▼                     ▼                      ▼
//!  Headless (here)      Crossterm                SoftwareRenderer
//!  in-memory grid,      (retroglyph-crossterm)   (retroglyph-software)
//!  synthetic events     real TTY, ANSI output    winit window, pixels
//! ```
//!
//! [`Headless`](crate::backend::Headless) stores presented content in memory and lets tests inject
//! synthetic [`Event`](crate::event::Event)s with [`Headless::push_event`](crate::backend::Headless::push_event);
//! nothing here talks to a real terminal or window. Swapping `Headless` for
//! `Crossterm` or `SoftwareRenderer` changes only the `B` type parameter --
//! `App` implementations, [`Terminal`](crate::terminal::Terminal) calls, and game logic are unchanged.
//! `run_blocking` drives `Terminal<Headless>` and `Terminal<Crossterm>`
//! identically; the software backend's windowed loop drives `Terminal<SoftwareRenderer>`
//! through the same [`App`](crate::app::App) contract, inverted because winit owns the
//! event loop instead of handing control back to a driver function.
//!
//! See `examples/headless.rs` (`cargo run -p retroglyph-core --example
//! headless`) for the smallest possible use of [`Headless`](crate::backend::Headless), depending on
//! nothing but this crate.
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
// A `pub mod` line's own outer doc comment and its target module's inner `//!` doc concatenate
// into one rendered page, but intra-doc links in that combined block resolve against the scope
// where the *outer* comment lives (this file, the crate root) rather than the module's own scope.
// Every module doc below that also carries an outer doc comment on its `pub mod` line therefore
// needs fully qualified links even for types the module defines itself, which then reads as
// "redundant" from the module file's own point of view. Rather than track that split per link,
// every intra-doc link in this crate is fully qualified and this lint is off crate-wide.
#![allow(rustdoc::redundant_explicit_links)]
extern crate alloc;

// A float backend is not optional (retroglyph#903): `animate`'s easing curves and the separable
// `BlendMode` channel math dispatch through `crate::math`, which has nothing to dispatch *to*
// without one, and `indexed-quant` forwards `gem/space`, which needs `gem/std` or `gem/libm` for
// the same reason. Failing here names the two features that fix it, ahead of the same build
// failing as an unresolved `libm::` path inside `math.rs` or inside `gem::space`'s own
// `compile_error!`. `libm-arch` needs no mention: it turns on `libm` itself.
#[cfg(not(any(feature = "std", feature = "libm")))]
compile_error!("retroglyph-core needs a float backend: enable `std` or `libm`.");

// Compile the code blocks in this crate's own README as doctests so its quick start is
// type-checked on every test run and cannot silently rot. The `cfg(doctest)` gate keeps this out
// of the rendered crate documentation: see `retroglyph-crossterm`'s matching include for the
// same pattern applied to the workspace root README.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

// clippy::too_long_first_doc_paragraph is a known-noisy nursery lint (rust-lang/rust-clippy#13441)
// that here misattributes its span across every subsequent `pub mod`/`pub use` declaration below
// (through to the next blank line) rather than just this one doc comment, which is well under
// its own 100-char threshold in isolation: confirmed by testing shorter wording alone, which
// silences it despite touching nothing else in that byte range.
#[allow(clippy::too_long_first_doc_paragraph)]
/// Time-driven value animation: easing curves, a stateful `Tween`, and a periodic oscillator.
///
/// The trig-based easing curves and the oscillator's sine wave go through this crate's `math`
/// shim, so they use `std`'s float intrinsics or `libm`'s software implementation depending on
/// which backend feature is on.
pub mod animate;
/// The `App`-driven game loop.
pub mod app;
// See the `too_long_first_doc_paragraph` comment above `animate`: same noisy-lint mis-attribution,
// here because this module's own first doc paragraph grew past the threshold once its intra-doc
// links became fully qualified (retroglyph#1035).
#[allow(clippy::too_long_first_doc_paragraph)]
/// Pluggable rendering backends.
pub mod backend;
// See the `too_long_first_doc_paragraph` comment above `animate`: same noisy-lint mis-attribution.
#[allow(clippy::too_long_first_doc_paragraph)]
/// A scrolling viewport into a world larger than the screen.
pub mod camera;
pub mod color;
// See the `too_long_first_doc_paragraph` comment above `animate`: same noisy-lint mis-attribution.
#[allow(clippy::too_long_first_doc_paragraph)]
/// Which diagnostics a build compiles in.
pub mod dev;
pub mod event;
// See the `too_long_first_doc_paragraph` comment above `animate`: same noisy-lint mis-attribution.
#[allow(clippy::too_long_first_doc_paragraph)]
/// `FrameClock`/`FrameStats` accumulators for the `App`/`Frame` game loop.
pub mod frames;
pub mod grid;
pub mod layout;
// `pub` so `retroglyph-widgets` can share this crate's one std-or-libm dispatch point instead of
// vendoring its own copy, `#[doc(hidden)]` so that sharing costs no public API surface:
// `cargo-semver-checks` ignores hidden items (see the module's own doc comment for the traps that
// come with that). Never add a `pub use` that re-exports its contents through a non-hidden path,
// and never `#[deprecated]` it, both of which would make it public API again despite the hiding.
#[doc(hidden)]
pub mod math;
// See the `too_long_first_doc_paragraph` comment above `animate`: same noisy-lint mis-attribution.
#[allow(clippy::too_long_first_doc_paragraph)]
/// The one grid-drawing primitive: an area-clipped, single-layer view over a [`Grid`](crate::grid::Grid).
pub mod surface;
#[allow(clippy::too_long_first_doc_paragraph)]
/// Border, gridline, and partial-block `char` data shared by widgets and backends.
pub mod symbols;
pub mod terminal;
// See the `too_long_first_doc_paragraph` comment above `animate`: same noisy-lint mis-attribution.
#[allow(clippy::too_long_first_doc_paragraph)]
/// Headless test harness driving an `App` with synthetic input.
#[cfg(feature = "testing")]
pub mod testing;
pub mod text;
/// The atomic drawable unit (glyph, style, sub-cell offsets).
pub mod tile;

// No root re-exports below this line by design (retroglyph#1035): every public item lives at its
// module path, matching `ratatui-core`. `dev_only!` (`dev.rs`) and `spans!` (`text.rs`) still
// resolve at the crate root regardless, since `#[macro_export]` always places a macro there; that's
// a macro-export constraint, not a re-export choice.
