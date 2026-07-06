//! Immediate-mode drawing helpers over a [`Rect`](retroglyph_core::Rect).
//!
//! Box borders, filled panels, gauges, lists, tabs, sparklines, and a small
//! constraint-based [`Rect`](retroglyph_core::Rect) splitter with
//! ratatui-style `Fixed`/`Percent`/`Fill`/`Min`/`Max` constraints and `Flex`
//! alignment ([`layout`]).
//!
//! Every widget is primarily a free function that draws directly to a
//! [`Terminal`](retroglyph_core::Terminal) and retains no state. Three
//! optional layers build on top of that free-function core, each usable
//! independently:
//!
//! - [`Widget`]/[`StatefulWidget`] ([`widget`]) for callers who want to box
//!   or store heterogeneous widgets, backed by [`ListState`] for selection
//!   and scroll position.
//! - [`BoxStyle`] ([`style`]) for a Lip-Gloss-style box model (padding,
//!   border, margin) rendered into a standalone `Grid`.
//! - [`join_h`]/[`join_v`] ([`block`]) to compose several `Grid`s -- e.g.
//!   `BoxStyle::render` output -- into one before drawing it.
//!
//! None of this is a replacement for calling the free functions directly;
//! this crate is itself optional, since games that draw manually depend
//! only on `retroglyph-core`.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::items_after_statements
)]

/// `join_h`/`join_v`: compose `Grid`s before drawing them.
pub mod block;
/// Box borders, filled panels, gauges, lists, tabs, and sparklines.
pub mod draw;
/// A small constraint-based [`Rect`](retroglyph_core::Rect) splitter.
pub mod layout;
/// Reusable, headless widget state (selection, scroll offset).
pub mod state;
/// A Lip-Gloss-style box model (padding/border/margin) over a `Grid`.
pub mod style;
/// Single-line column-clipping, unicode-width aware.
pub mod text;
/// Optional `Widget`/`StatefulWidget` traits, as a thin adapter over `draw`.
pub mod widget;

pub use block::{blit_into, join_h, join_v};
pub use draw::*;
pub use layout::{Constraint, Flex, split_h, split_h_flex, split_v, split_v_flex};
pub use state::ListState;
pub use style::{BoxStyle, Sides};
pub use text::truncate;
#[cfg(feature = "egc")]
pub use widget::Paragraph;
pub use widget::{Measure, Panel, StatefulWidget, Table, Widget};
