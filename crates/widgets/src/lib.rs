//! Immediate-mode drawing helpers over a [`Rect`](retroglyph_core::Rect).
//!
//! Box borders, filled panels, gauges, tables, sparklines, and a small
//! constraint-based [`Rect`](retroglyph_core::Rect) splitter with
//! ratatui-style `Fixed`/`Percent`/`Fill`/`Min`/`Max` constraints and `Flex`
//! alignment ([`layout`]).
//!
//! Every widget ([`widget`]) is a builder struct that draws itself into a
//! [`Grid`](retroglyph_core::Grid) via [`Widget`]/[`StatefulWidget`] and
//! retains no state of its own: state that outlives one render call (a
//! selection index, a scroll offset, a text field's value and cursor)
//! lives in [`ListState`]/[`TextInputState`] instead. A
//! handful of things that are genuinely just functions ([`fill_rect`],
//! [`thumb_geometry`]/[`offset_for_pos`] in [`draw`]; [`truncate`]/[`truncate_owned`] in
//! [`text`]) stay free functions rather than pretending to be widgets. Three more independent
//! layers build on top:
//!
//! - [`Widget`]/[`StatefulWidget`] ([`widget`]) render into a [`Surface`],
//!   an area-relative, single-layer view over a [`Grid`](retroglyph_core::Grid), and let
//!   callers box or store heterogeneous widgets, e.g. a `Vec<Box<dyn Widget>>` of panes to
//!   render each frame, with no `Backend` type parameter, since drawing touches nothing but
//!   cells. [`AnimatedWidget`] is `StatefulWidget`'s sibling for state that evolves with
//!   wall-clock time (e.g. [`ScrollState`]'s momentum physics) instead of only in response to
//!   input; see its own docs. [`InteractiveWidget`] is the sibling trait for widgets that also
//!   read a [`Response`] (`Button`, `Scrollbar`, `List`, `Tabs`).
//! - [`Interaction`] ([`interact`]) for hover/click/drag/focus tracking
//!   without a retained widget tree: the sibling of [`ListState`] for
//!   widgets that don't have a natural selection index of their own. [`Ui`] pairs one frame's
//!   [`Surface`] with an `Interaction`, via [`Interaction::frame`]: [`Ui::show`] is how an
//!   [`InteractiveWidget`] gets hit-tested and drawn from the one area/id a call site names.
//! - [`BoxStyle`] ([`style`]) for a Lip-Gloss-style box model (padding,
//!   border, margin) rendered into a standalone `Grid`.
//! - [`join_h`]/[`join_v`] ([`block`]) to compose several `Grid`s (e.g. `BoxStyle::render`
//!   output) into one; `retroglyph_core::Surface::blit` stamps the result onto a surface.
//! - [`Theme`] ([`theme`]) for named color roles (an app picks
//!   [`Theme::DARK`]/[`Theme::LIGHT`], or builds its own), independent of
//!   how the app decides which one is active.
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
//! that would otherwise compile them out (see [`retroglyph_core::dev`]).
//!
//! ### `egc`
//!
//! ⚪ Optional.
//!
//! Forwards to `retroglyph-core`'s `egc` feature.
//!
//! Upgrades [`Paragraph`]'s word-wrap (always available) to grapheme-cluster-aware correctness.
//!
//! ### `libm`
//!
//! ⚪ Optional.
//!
//! Uses `retroglyph-core`'s `libm` feature (the `no_std` float backend: scrollbar geometry,
//! gauge/sparkline/bar percentage rounding, scroll momentum decay) instead of `std`'s own float
//! intrinsics. See `std` below; a build needs exactly one of the two.
//!
//! ### `libm-arch`
//!
//! ⚪ Optional.
//!
//! Alias for `libm`, matching `retroglyph-core`'s own `libm-arch` feature name.
//!
//! ### `serde`
//!
//! ⚪ Optional.
//!
//! Adds `Serialize`/`Deserialize` impls for [`Theme`] and `Density`, forwarding to
//! `retroglyph-core`'s `serde` feature.
//!
//! [`Theme`] round-trips through `Color`'s own `serde` impl.
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
//! <!-- gen-features:end -->

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
extern crate alloc;

// Unlike `retroglyph-core`'s `animate` (which simply disappears without a float backend), this
// crate's float use -- scrollbar geometry, gauge/sparkline/bar percentage rounding, scroll
// momentum decay -- is load-bearing across the widget set, not an opt-in extra. There is no
// widgets equivalent of "the affected module just isn't compiled in", so a build with neither
// backend fails loudly here instead of hitting `retroglyph_core::math`'s own unresolved-`libm`
// error deep in a call site. `libm-arch` counts too: it only forwards to
// `retroglyph-core/libm-arch` (this crate has no `libm` feature of its own turned on by it), so
// without it here a lone `--features libm-arch` build would trip this check despite
// `retroglyph-core` actually having a real backend.
#[cfg(not(any(feature = "std", feature = "libm", feature = "libm-arch")))]
compile_error!("retroglyph-widgets needs a float backend: enable `std` or `libm`.");

// Compile the code blocks in this crate's own README as doctests so its quick start is
// type-checked on every test run and cannot silently rot. The `cfg(doctest)` gate keeps this out
// of the rendered crate documentation: see `retroglyph-crossterm`'s matching include for the
// same pattern applied to the workspace root README.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod align;
pub mod block;
pub mod draw;
pub mod interact;
pub mod layout;
/// A live frame-time/FPS overlay: [`PerfOverlayApp`] wraps any `App` with one, on any `Backend`.
pub mod perf;
pub mod state;
pub mod style;
pub mod text;
pub mod theme;
pub mod ui;
pub mod widget;

pub use align::Align;
pub use block::{join_h, join_v};
pub use draw::{fill_rect, offset_for_pos, thumb_geometry};
pub use interact::{
    Consumed, DEFAULT_DRAG_THRESHOLD, Density, FocusRing, HitTester, Interaction, Pointer,
    Response, Sense, Shortcuts,
};
pub use layout::{
    Constraint, Flex, Side, Spacing, anchored_rect, centered_rect, split_h, split_h_flex,
    split_h_n, split_h_n_flex, split_h_n_spaced, split_h_spaced, split_v, split_v_flex, split_v_n,
    split_v_n_flex, split_v_n_spaced, split_v_spaced,
};
pub use perf::{
    DEFAULT_LAYER, DefaultPerfRenderer, FRAME_HISTORY, PerfOverlayApp, PerfOverlayMode,
    PerfRenderer, default_is_toggle_key,
};
pub use retroglyph_core::{Layer, StyledSurface, Surface};
pub use state::{ListState, ScrollPhysics, ScrollState, SelectionWrap, TextInputState};
pub use style::{BoxStyle, Sides};
pub use text::{draw_clipped, truncate, truncate_owned};
pub use theme::Theme;
pub use ui::Ui;
pub use widget::{
    AnimatedPerfOverlay, AnimatedWidget, BorderType, BoxBorder, Button, Gauge, HighlightSpacing,
    InteractiveWidget, List, ListDirection, Log, Measure, Meter, Modal, Panel, PanelTitle,
    Paragraph, PerfOverlay, PrintLine, ProgressBar, Scrollbar, Sparkline, StatBar, StatefulWidget,
    Table, Tabs, Text, TextInput, TitlePosition, Widget,
};
