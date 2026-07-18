//! Immediate-mode drawing helpers over a [`Rect`](retroglyph_core::Rect).
//!
//! Box borders, filled panels, gauges, tables, sparklines, and a small
//! constraint-based [`Rect`](retroglyph_core::Rect) splitter with
//! ratatui-style `Fixed`/`Percent`/`Fill`/`Min`/`Max` constraints and `Flex`
//! alignment ([`layout`]).
//!
//! Every widget ([`widget`]) is a builder struct that draws itself into a
//! [`Terminal`](retroglyph_core::Terminal) via [`Widget`]/[`StatefulWidget`]
//! and retains no state of its own -- state that outlives one render call
//! (a selection index, a scroll offset) lives in [`ListState`] instead. A
//! handful of things that are genuinely just functions ([`fill_rect`],
//! [`thumb_geometry`]/[`offset_for_pos`]) stay free functions in [`draw`]
//! rather than pretending to be widgets. Three more independent layers
//! build on top:
//!
//! - [`Widget`]/[`StatefulWidget`] ([`widget`]) let callers box or store
//!   heterogeneous widgets, e.g. a `Vec<Box<dyn Widget<B>>>` of panes to
//!   render each frame.
//! - [`Interaction`] ([`interact`]) for hover/click/drag/focus tracking
//!   without a retained widget tree -- the sibling of [`ListState`] for
//!   widgets that don't have a natural selection index of their own.
//! - [`BoxStyle`] ([`style`]) for a Lip-Gloss-style box model (padding,
//!   border, margin) rendered into a standalone `Grid`.
//! - [`join_h`]/[`join_v`] ([`block`]) to compose several `Grid`s -- e.g.
//!   `BoxStyle::render` output -- into one before drawing it.
//! - [`Theme`] ([`theme`]) for named color roles (an app picks
//!   [`Theme::DARK`]/[`Theme::LIGHT`], or builds its own), independent of
//!   how the app decides which one is active.
//!
//! This crate is itself optional: games that draw manually depend only on
//! `retroglyph-core`.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::items_after_statements
)]

// Compile the code blocks in this crate's own README as doctests so its quick start is
// type-checked on every test run and cannot silently rot. The `cfg(doctest)` gate keeps this out
// of the rendered crate documentation -- see `retroglyph-crossterm`'s matching include for the
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
pub mod widget;

pub use align::Align;
pub use block::{blit_into, join_h, join_v};
pub use draw::{fill_rect, offset_for_pos, thumb_geometry};
pub use interact::{
    DEFAULT_DRAG_THRESHOLD, Density, FocusRing, HitTester, Interaction, Pointer, Response, Sense,
    Shortcuts,
};
pub use layout::{
    Constraint, Flex, centered_rect, split_h, split_h_flex, split_h_spaced, split_v, split_v_flex,
    split_v_spaced,
};
pub use state::{ListState, ScrollPhysics, ScrollState, SelectionWrap};
pub use style::{BoxStyle, Sides};
pub use text::truncate;
pub use theme::Theme;
#[cfg(feature = "egc")]
pub use widget::Paragraph;
pub use widget::{
    BoxBorder, Button, Gauge, List, Log, Measure, Meter, Modal, Panel, PrintLine, ProgressBar,
    Scrollbar, Sparkline, StatBar, StatefulWidget, Table, Tabs, Text, Widget,
};
