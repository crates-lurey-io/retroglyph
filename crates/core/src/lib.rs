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
//! Default features: `blend-modes`, `egc`, `indexed-quant`, `std`.
//!
//! ### `blend-modes`
//!
//! 🟢 Enabled by default.
//!
//! Gates the four W3C separable [`BlendMode`] variants (`Screen`/`Dodge`/`Burn`/`Overlay`/
//! `Multiply`) and pulls in the optional `alpha-blend` dependency. Needs a float backend (`std` or
//! `libm`) for its per-channel blend math, so this also turns on `__float`.
//!
//! [`BlendMode::Linear`] and [`Grid::blit_alpha`] are always available regardless of this feature:
//! `Linear` only needs `gem::Mix`, not `alpha-blend`.
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
//! Gates perceptual (Oklab) RGB → Indexed/ANSI quantization (`gem/space`) and [`Color`]'s
//! `gem`-space conversions (`to_srgb`/`from_srgb`/`lerp`/`from_hex`).
//!
//! Without it, [`Color::to_indexed`](color::Color::to_indexed)/
//! [`Color::to_ansi`](color::Color::to_ansi) fall back to euclidean RGB cube-mapping instead of
//! failing to compile.
//!
//! This is a capability flag, not a backend: it only turns on `gem`'s `space` module, and needs
//! `std` or `libm` (below) enabled separately to actually supply that module's float math -- see
//! the `compile_error!` in `src/lib.rs` for what happens if neither is on.
//!
//! ### `libm`
//!
//! ⚪ Optional.
//!
//! Uses `libm`'s software float implementation (`roundf`/`fmaf`/`sinf`/`cosf`/`powf`) for
//! `animate`'s easing curves and `blend-modes`' separable channel math, via this crate's own `math`
//! shim -- the `no_std` side of that split. Turns on `__float`. See `std` above for the alternative
//! that prefers the platform's own float intrinsics when available.
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
//! Adds `Serialize`/`Deserialize` impls for [`Color`], [`Style`], `Size`, and (via `ixy`)
//! `Pos`/`Rect`, so a config file can round-trip a saved camera position, window geometry, or theme
//! color.
//!
//! [`Color`] serializes through its `Display`/`FromStr` round trip (e.g. `"bright-red"`,
//! `"#ff8000"`) rather than a derived structural form, so hand-edited TOML/JSON stays legible.
//!
//! ### `std`
//!
//! 🟢 Enabled by default.
//!
//! Enables `gem/std` and `alpha-blend?/std`, and uses `std`'s float intrinsics (via this crate's
//! `math` shim) instead of `libm`'s software implementation for `animate` and `blend-modes`. Turns
//! on `__float`.
//!
//! Disabling this feature (`--no-default-features`) builds this crate `no_std`.
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

// `indexed-quant` is an explicit opt-in (unlike `animate`, which degrades silently -- see the
// `#[cfg(feature = "__float")]` on `pub mod animate` below) that forwards `gem/space` without
// itself requesting a float backend, since which of `std`/`libm` supplies that module's math is
// meant to be picked independently (see the feature's own doc comment in `Cargo.toml`). So a
// build that enables it without either fails loudly here instead of only inside `gem::space`'s
// own `compile_error!`.
#[cfg(all(feature = "indexed-quant", not(feature = "__float")))]
compile_error!("`indexed-quant` needs a float backend: enable `std` or `libm`.");
// `blend-modes` also lists `__float` directly (see that feature's doc comment), so
// `--features blend-modes` alone -- no `std`, no `libm` -- would otherwise reach `crate::math`'s
// `libm::` branch with no `libm` crate linked and fail with a confusing "unresolved crate" error
// instead of this one.
#[cfg(all(feature = "__float", not(any(feature = "std", feature = "libm"))))]
compile_error!("a float backend is required: enable `std` or `libm`.");

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
#[cfg(feature = "__float")]
#[cfg_attr(docsrs, doc(cfg(feature = "__float")))]
#[allow(clippy::too_long_first_doc_paragraph)]
/// Time-driven value animation: easing curves, a stateful `Tween`, and a periodic oscillator.
///
/// Needs a float backend (`std` or `libm`) for its trig-based easing curves and its oscillator's
/// sine wave. Unlike `indexed-quant`/`blend-modes` (explicit opt-ins that fail loudly via
/// `compile_error!` without one), this module simply isn't compiled in without a backend.
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
// Two declarations of the same module, differing only in visibility: internally, `animate` and
// `grid`'s separable blend math use `crate::math::*` whenever `__float` is on, regardless of
// `__math`, so the module has to exist either way. `__math` only changes whether it's also
// reachable from outside this crate, for `retroglyph-widgets` to share instead of vendoring its
// own copy. `#[doc(hidden)]` keeps the exposed form out of the public API surface as far as
// `cargo-semver-checks` is concerned (see the module's own doc comment for the traps that come
// with that); never add a `pub use` that re-exports its contents through a non-hidden path, and
// never `#[deprecated]` it, both of which would make it public API again despite the hiding.
#[cfg(all(feature = "__float", not(feature = "__math")))]
mod math;
#[cfg(all(feature = "__float", feature = "__math"))]
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

#[cfg(feature = "__float")]
pub use animate::{Easing, Tween, oscillate};
pub use app::{App, Flow, Frame};
#[cfg(feature = "std")]
pub use app::{RunOptions, run_blocking, run_blocking_with};
pub use backend::{Backend, Cursor, CursorStyle, DrawCell, Headless, Input, Output};
pub use camera::Camera;
pub use color::{AnsiColor, Color, InvalidAnsiIndex, ParseColorError, Style, Tint};
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
