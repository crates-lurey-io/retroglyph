//! Immediate-mode drawing helpers over a [`Rect`](retroglyph_core::grid::Rect).
//!
//! Box borders, filled panels, gauges, tables, sparklines, and a small
//! constraint-based [`Rect`](retroglyph_core::grid::Rect) splitter with
//! ratatui-style `Fixed`/`Percent`/`Fill`/`Min`/`Max` constraints and `Flex`
//! alignment ([`layout`]).
//!
//! Every widget ([`widget`]) is a builder struct that draws itself into a
//! [`Grid`](retroglyph_core::grid::Grid) via [`Widget`](widget::Widget)/[`StatefulWidget`](widget::StatefulWidget) and
//! retains no state of its own: state that outlives one render call (a
//! selection index, a scroll offset, a text field's value and cursor)
//! lives in [`ListState`](state::ListState)/[`TextInputState`](state::TextInputState) instead. A
//! handful of things that are genuinely just functions ([`fill_rect`](draw::fill_rect),
//! [`thumb_geometry`](draw::thumb_geometry)/[`offset_for_pos`](draw::offset_for_pos) in [`draw`]; [`truncate`](text::truncate)/[`truncate_owned`](text::truncate_owned) in
//! [`text`]) stay free functions rather than pretending to be widgets. Three more independent
//! layers build on top:
//!
//! - [`Widget`](widget::Widget)/[`StatefulWidget`](widget::StatefulWidget) ([`widget`]) render into a [`Surface`],
//!   an area-relative, single-layer view over a [`Grid`](retroglyph_core::grid::Grid), and let
//!   callers box or store heterogeneous widgets, e.g. a `Vec<Box<dyn Widget>>` of panes to
//!   render each frame, with no `Backend` type parameter, since drawing touches nothing but
//!   cells. [`AnimatedWidget`](widget::AnimatedWidget) is `StatefulWidget`'s sibling for state that evolves with
//!   wall-clock time (e.g. [`ScrollState`](state::ScrollState)'s momentum physics) instead of only in response to
//!   input; see its own docs. [`InteractiveWidget`](widget::InteractiveWidget) is the sibling trait for widgets that also
//!   read a [`Response`](interact::Response) (`Button`, `Scrollbar`, `List`, `Tabs`). For a
//!   bordered box drawn straight into that `Surface` -- with titles, [`BorderType`](widget::BorderType)
//!   glyph sets, `padding`, and theming -- reach for [`Panel`](widget::Panel) first; it's the
//!   default answer to "draw me a box".
//! - [`Interaction`](interact::Interaction) ([`interact`]) for hover/click/drag/focus tracking
//!   without a retained widget tree: the sibling of [`ListState`](state::ListState) for
//!   widgets that don't have a natural selection index of their own. [`Ui`](ui::Ui) pairs one frame's
//!   [`Surface`] with an `Interaction`, via [`Interaction::frame`](interact::Interaction::frame): [`Ui::show`](ui::Ui::show) is how an
//!   [`InteractiveWidget`](widget::InteractiveWidget) gets hit-tested and drawn from the one area/id a call site names.
//! - [`BoxStyle`](style::BoxStyle) ([`style`]) is the standalone alternative to
//!   [`Panel`](widget::Panel): a Lip-Gloss-style box model (padding, border, margin) rendered
//!   into its own `Grid`, independent of any `Surface`/`Backend`/`Terminal`. Reach for it instead
//!   of `Panel` when composing a box outside a frame, laying several boxes out with `join_h`/
//!   `join_v` below, or needing `margin` (space outside the border, which `Panel` has no concept
//!   of); it has no titles, `BorderType`, or theming.
//! - [`join_h`](block::join_h)/[`join_v`](block::join_v) ([`block`]) to compose several `Grid`s (e.g. `BoxStyle::render`
//!   output) into one; `retroglyph_core::surface::Surface::blit` stamps the result onto a surface.
//! - [`Theme`](theme::Theme) ([`theme`]) for named color roles (an app picks
//!   [`Theme::DARK`](theme::Theme::DARK)/[`Theme::LIGHT`](theme::Theme::LIGHT), or builds its own), independent of
//!   how the app decides which one is active.
//! - [`HalfBlockCanvas`](canvas::HalfBlockCanvas)/[`QuadrantCanvas`](canvas::QuadrantCanvas)/
//!   [`SextantCanvas`](canvas::SextantCanvas)/[`BrailleCanvas`](canvas::BrailleCanvas) ([`canvas`]) for sub-cell
//!   drawing: plot points at higher-than-one-per-cell resolution, then read a frame's worth of
//!   glyphs back out, one per cell.
//!
//! This crate is itself optional: games that draw manually depend only on
//! `retroglyph-core`.
//!
//! This crate is `no_std`-compatible: disable the `std` feature and enable `libm` instead (also
//! requires an allocator). See the `std` and `libm` features below.
//!
//! # Features
//!
//! <!-- gen-features:start -->
//! Default features: `std`.
//!
//! ### `dev`
//!
//! ⚪ Optional.
//!
//! Forwards `retroglyph-core`'s `dev` feature, which forces development diagnostics on in a build
//! that would otherwise compile them out (see [`retroglyph_core::dev`](retroglyph_core::dev)).
//!
//! ### `egc`
//!
//! ⚪ Optional.
//!
//! Forwards to `retroglyph-core`'s `egc` feature.
//!
//! Upgrades [`Paragraph`](crate::widget::Paragraph)'s word-wrap (always available) to grapheme-cluster-aware correctness.
//!
//! ### `libm`
//!
//! ⚪ Optional.
//!
//! Uses `retroglyph-core`'s `libm` feature (the `no_std` float backend: easing curves and
//! tweens, scrollbar geometry, gauge/sparkline/bar percentage rounding, scroll momentum decay)
//! instead of `std`'s own float intrinsics. See `std` below; a build needs exactly one of the two.
//!
//! ### `serde`
//!
//! ⚪ Optional.
//!
//! Adds `Serialize`/`Deserialize` impls for [`Theme`](crate::theme::Theme) and `Density`, forwarding to
//! `retroglyph-core`'s `serde` feature.
//!
//! [`Theme`](crate::theme::Theme) round-trips through `Color`'s own `serde` impl.
//!
//! ### `std`
//!
//! 🟢 Enabled by default.
//!
//! Enables `retroglyph-core/std`, whose float intrinsics back this crate's own float use (see
//! `libm` above for the `no_std` alternative).
//!
//! Disabling this feature (`--no-default-features`) builds this crate `no_std` (requires an
//! allocator and one of `std`/`libm`; see the crate-level `compile_error!` in `src/lib.rs`).
//!
//! ### `testing`
//!
//! ⚪ Optional.
//!
//! Enables `testing`'s `WidgetHarness`, the sibling of `retroglyph-core`'s
//! `testing::TestHarness` for driving a bare `Interaction<Id>`/`Ui` (no `App` required) against
//! `Headless` for tests, with synthetic input queuing and a `step` that drains it.
//!
//! Test-only surface, off by default so it never ships in a release build by accident. `Headless`
//! is unconditionally available in `retroglyph-core`, so this needs no feature forwarding there.
//! <!-- gen-features:end -->

#![cfg_attr(not(feature = "std"), no_std)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/crates-lurey-io/retroglyph/main/docs/public/assets/logo.svg"
)]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/crates-lurey-io/retroglyph/main/docs/public/assets/logo.svg"
)]
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

// This crate's float use -- easing curves and tweens, scrollbar geometry, gauge/sparkline/bar
// percentage rounding, scroll momentum decay -- is load-bearing across the widget set, not an
// opt-in extra. There is no equivalent of "the affected module just isn't compiled in", so a
// build with neither backend fails loudly here instead of hitting `retroglyph_core::math`'s own
// unresolved-`libm` error deep in a call site.
#[cfg(not(any(feature = "std", feature = "libm")))]
compile_error!("retroglyph-ui needs a float backend: enable `std` or `libm`.");

// Compile the code blocks in this crate's own README as doctests so its quick start is
// type-checked on every test run and cannot silently rot. The `cfg(doctest)` gate keeps this out
// of the rendered crate documentation: see `retroglyph-crossterm`'s matching include for the
// same pattern applied to the workspace root README.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod align;
// A `pub mod` line's own doc comment concatenates with the module's inner `//!` doc and resolves
// against the crate root, so this module's own types need to be qualified in it even though the
// module defines them itself; that inflation is what pushes the paragraph below past the
// `too_long_first_doc_paragraph` threshold (retroglyph#1273).
#[allow(clippy::too_long_first_doc_paragraph)]
/// Time-driven value animation: easing curves, a stateful `Tween`, and a periodic oscillator.
///
/// The trig-based easing curves and the oscillator's sine wave go through `retroglyph_core`'s
/// `math` shim, so they use `std`'s float intrinsics or `libm`'s software implementation
/// depending on which backend feature is on.
pub mod animate;
pub mod block;
/// A scrolling viewport into a world larger than the screen.
pub mod camera;
// See the `too_long_first_doc_paragraph` comment above `animate`: same noisy-lint mis-attribution.
#[allow(clippy::too_long_first_doc_paragraph)]
/// Sub-cell drawing: plot points/pixels at higher-than-one-per-cell resolution and read them
/// back as glyphs, via `retroglyph_core::symbols`'s quantizers.
pub mod canvas;
pub mod draw;
pub mod interact;
pub mod layout;
pub mod state;
pub mod style;
#[cfg(feature = "testing")]
pub mod testing;
pub mod text;
pub mod theme;
pub mod ui;
pub mod widget;

// No root re-exports below this line by design (retroglyph#1273, matching retroglyph-core's
// retroglyph#1035): every public item lives at its module path. `retroglyph_core::surface`'s
// types are the one exception, kept per retroglyph#1035's own reasoning for this line: they spare
// callers from tracking which crate a type lives in, which is a different, legitimate concern
// from this crate's own root re-exports.
pub use retroglyph_core::surface::{Layer, StyledSurface, Surface};
