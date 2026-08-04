//! retroglyph-core: the `no_std`-compatible foundation of retroglyph.
//!
//! Grid, tile, style, color, text, terminal, and event types, plus the
//! [`Output`]/[`Input`]/[`Cursor`] backend facets (bundled together as [`Backend`]) and the
//! dependency-free [`Headless`] test backend, and the `App`/`Flow`/`Frame` game loop contract.
//! Platform backends (`retroglyph-crossterm`, `retroglyph-software`) and drawing helpers
//! (`retroglyph-widgets`) are separate crates that depend on this one.
//!
//! # Features
//!
//! <!-- gen-features:start -->
//! Default features: `egc`, `std`.
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
//! ### `libm`
//!
//! ⚪ Optional.
//!
//! Uses `libm`'s software float implementation (`roundf`/`fmaf`/`sinf`/`cosf`/`powf`) for
//! `animate`'s easing curves and the separable [`BlendMode`] channel math, via this crate's own
//! `math` shim -- the `no_std` side of that split. See `std` below for the alternative that prefers
//! the platform's own float intrinsics when available; a build needs exactly one of the two.
//!
//! ### `serde`
//!
//! ⚪ Optional.
//!
//! Adds `Serialize`/`Deserialize` impls for [`Color`], [`Style`], `Size`, `Offset`, and (via `ixy`)
//! `Pos`/`Rect`, so a config file can round-trip a saved camera position, window geometry, sub-cell
//! pixel offset, or theme color.
//!
//! [`Color`] serializes through its `Display`/`FromStr` round trip (e.g. `"bright-red"`,
//! `"#ff8000"`) rather than a derived structural form, so hand-edited TOML/JSON stays legible.
//!
//! ### `std`
//!
//! 🟢 Enabled by default.
//!
//! Enables `gem/std` and `alpha-blend/std`, and uses `std`'s float intrinsics (via this crate's
//! `math` shim) instead of `libm`'s software implementation for `animate` and the separable
//! [`BlendMode`] channel math.
//!
//! Disabling this feature (`--no-default-features`) builds this crate `no_std`, and then needs
//! `libm` above as the float backend instead: see the crate-level `compile_error!` in `src/lib.rs`.
//!
//! ### `testing`
//!
//! ⚪ Optional.
//!
//! Enables `testing`'s `TestHarness`, which drives an [`App`] against [`Headless`] for tests, with
//! synthetic input queuing and frame-settling helpers.
//!
//! Test-only surface, `no_std` + `alloc` compatible, off by default so it never ships in a release
//! build by accident.
//! <!-- gen-features:end -->
//!
//! # Architecture
//!
//! [`Terminal<B>`](Terminal) owns a double-buffered [`Grid`] and the [`Backend`] lifecycle
//! (resize, present, events). Drawing itself goes entirely through [`Surface`], handed out by
//! [`Terminal::draw`]/[`Terminal::surface`]: a game calls `term.draw(|s| { s.put(...); ... })`
//! once per frame, and [`present`](Terminal::present) diffs the current frame against the
//! previous one, sending only changed cells to the [`Backend`]. `B` is the only thing that
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
//! [`Headless`] stores presented content in memory and lets tests inject
//! synthetic [`Event`]s with [`Headless::push_event`](backend::Headless::push_event);
//! nothing here talks to a real terminal or window. Swapping `Headless` for
//! `Crossterm` or `SoftwareRenderer` changes only the `B` type parameter --
//! `App` implementations, [`Terminal`] calls, and game logic are unchanged.
//! `run_blocking` drives `Terminal<Headless>` and `Terminal<Crossterm>`
//! identically; the software backend's windowed loop drives `Terminal<SoftwareRenderer>`
//! through the same [`App`] contract, inverted because winit owns the
//! event loop instead of handing control back to a driver function.
//!
//! See `examples/headless.rs` (`cargo run -p retroglyph-core --example
//! headless`) for the smallest possible use of [`Headless`], depending on
//! nothing but this crate.
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
extern crate alloc;

// A float backend is not optional (retroglyph#903): `animate`'s easing curves and the separable
// `BlendMode` channel math dispatch through `crate::math`, which has nothing to dispatch *to*
// without one, and `Color`'s color-space conversions go through `gem/space`, which needs
// `gem/std` or `gem/libm` for the same reason. Failing here names the two features that fix it,
// ahead of the same build failing as an unresolved `libm::` path inside `math.rs` or inside
// `gem::space`'s own `compile_error!`.
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
/// Pluggable rendering backends.
pub mod backend;
/// A scrolling viewport into a world larger than the screen.
pub mod camera;
pub mod color;
/// Which diagnostics a build compiles in.
pub mod dev;
pub mod event;
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
/// The one grid-drawing primitive: an area-clipped, single-layer view over a [`Grid`].
pub mod surface;
#[allow(clippy::too_long_first_doc_paragraph)]
/// Border, gridline, and partial-block `char` data shared by widgets and backends.
pub mod symbols;
pub mod terminal;
/// Headless test harness driving an `App` with synthetic input.
#[cfg(feature = "testing")]
pub mod testing;
pub mod text;
/// The atomic drawable unit (glyph, style, sub-cell offsets).
pub mod tile;

pub use animate::{Easing, Tween, oscillate, oscillate_with_phase};
pub use app::{App, Flow, Frame};
#[cfg(feature = "std")]
pub use app::{RunOptions, run, run_blocking, run_blocking_with, run_with};
pub use backend::{Backend, Cursor, CursorStyle, DrawCell, Headless, Input, Output};
pub use camera::Camera;
pub use color::{AnsiColor, Color, InvalidAnsiIndex, ParseColorError, Quantize, Style, Tint};
pub use dev::{BuildMode, DEV};
pub use event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyLocation, KeyModifiers, KeyState, MouseButton,
    MouseEvent, MouseEventKind, PhysicalPos, SystemTheme,
};
pub use frames::{FrameClock, FrameStats};
pub use grid::{BlendMode, Grid, Offset, Pos, Rect, Size};
/// `.width()`/`.height()` accessors for [`Size`] (and [`Rect`]): re-exported so callers don't need
/// a direct `ixy` dependency just to call them on this crate's own type aliases.
pub use ixy::HasSize;
#[cfg(feature = "egc")]
pub use layout::TextLayout;
pub use layout::{HAlign, VAlign};
pub use surface::{Layer, StyledSurface, Surface};
pub use symbols::{Glyph, quantize_half_block, quantize_quadrant, quantize_sextant};
pub use terminal::Terminal;
#[cfg(feature = "testing")]
pub use testing::{RunError, TestHarness};
pub use text::{Line, Span};
pub use tile::Tile;
