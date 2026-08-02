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
//! selection index, a scroll offset) lives in [`ListState`] instead. A
//! handful of things that are genuinely just functions ([`fill_rect`],
//! [`thumb_geometry`]/[`offset_for_pos`]) stay free functions in [`draw`]
//! rather than pretending to be widgets. Three more independent layers
//! build on top:
//!
//! - [`Widget`]/[`StatefulWidget`] ([`widget`]) render into a [`Surface`],
//!   an area-relative, single-layer view over a [`Grid`](retroglyph_core::Grid), and let
//!   callers box or store heterogeneous widgets, e.g. a `Vec<Box<dyn Widget>>` of panes to
//!   render each frame, with no `Backend` type parameter, since drawing touches nothing but
//!   cells. [`AnimatedWidget`] is `StatefulWidget`'s sibling for state that evolves with
//!   wall-clock time (e.g. [`ScrollState`]'s momentum physics) instead of only in response to
//!   input -- see its own docs. [`InteractiveWidget`] is the sibling trait for widgets that also
//!   read a [`Response`] (`Button`, `Scrollbar`, `List`, `Tabs`).
//! - [`Interaction`] ([`interact`]) for hover/click/drag/focus tracking
//!   without a retained widget tree: the sibling of [`ListState`] for
//!   widgets that don't have a natural selection index of their own. [`Ui`] pairs one frame's
//!   [`Surface`] with an `Interaction`, via [`Interaction::frame`]: [`Ui::show`] is how an
//!   [`InteractiveWidget`] gets hit-tested and drawn from the one area/id a call site names.
//! - [`BoxStyle`] ([`style`]) for a Lip-Gloss-style box model (padding,
//!   border, margin) rendered into a standalone `Grid`.
//! - [`join_h`]/[`join_v`] ([`block`]) to compose several `Grid`s (e.g.
//!   `BoxStyle::render` output) into one before drawing it.
//! - [`Theme`] ([`theme`]) for named color roles (an app picks
//!   [`Theme::DARK`]/[`Theme::LIGHT`], or builds its own), independent of
//!   how the app decides which one is active.
//!
//! This crate is itself optional: games that draw manually depend only on
//! `retroglyph-core`.
//!
//! # Features
//!
//! - `dev` (⚪ optional): forwards `retroglyph-core`'s `dev` feature, forcing development
//!   diagnostics on in a build that would otherwise compile them out.
//! - `egc` (⚪ optional): forwards to `retroglyph-core`'s `egc` feature; enables `Paragraph`'s
//!   grapheme-cluster-aware word-wrap.
//! - `serde` (⚪ optional): `Serialize`/`Deserialize` impls for [`Theme`] and `Density`, forwarding
//!   to `retroglyph-core`'s `serde` feature ([`Theme`] round-trips through `Color`'s own `serde`
//!   impl).

#![cfg_attr(docsrs, feature(doc_cfg))]

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
pub mod state;
pub mod style;
pub mod text;
pub mod theme;
pub mod ui;
pub mod widget;

pub use align::Align;
pub use block::{blit_into, join_h, join_v};
pub use draw::{fill_rect, offset_for_pos, thumb_geometry};
pub use interact::{
    Consumed, DEFAULT_DRAG_THRESHOLD, Density, FocusRing, HitTester, Interaction, Pointer,
    Response, Sense, Shortcuts,
};
pub use layout::{
    Constraint, Flex, centered_rect, split_h, split_h_flex, split_h_spaced, split_v, split_v_flex,
    split_v_spaced,
};
pub use retroglyph_core::{Layer, StyledSurface, Surface};
pub use state::{ListState, ScrollPhysics, ScrollState, SelectionWrap, TextInputState};
pub use style::{BoxStyle, Sides};
pub use text::{truncate, truncate_owned};
pub use theme::Theme;
pub use ui::Ui;
#[cfg(feature = "egc")]
pub use widget::Paragraph;
pub use widget::{
    AnimatedPerfOverlay, AnimatedWidget, BoxBorder, Button, Gauge, InteractiveWidget, List, Log,
    Measure, Meter, Modal, Panel, PerfOverlay, PrintLine, ProgressBar, Scrollbar, Sparkline,
    StatBar, StatefulWidget, Table, Tabs, Text, TextInput, Widget,
};
