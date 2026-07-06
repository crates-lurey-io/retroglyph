//! Immediate-mode drawing helpers over a [`Rect`](retroglyph_core::Rect).
//!
//! Box borders, filled panels, gauges, lists, tabs, sparklines, and a small
//! constraint-based [`Rect`](retroglyph_core::Rect) splitter ([`layout`]).
//!
//! Every widget is primarily a free function that draws directly to a
//! [`Terminal`](retroglyph_core::Terminal) and retains no state; the
//! [`Widget`]/[`StatefulWidget`] traits in [`widget`] are optional sugar over
//! those functions for callers who want to box or store widgets, not a
//! replacement for them.
//!
//! This crate is optional: games that draw manually depend only on
//! `retroglyph-core`.

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
/// Single-line column-clipping, unicode-width aware.
pub mod text;
/// Optional `Widget`/`StatefulWidget` traits, as a thin adapter over `draw`.
pub mod widget;

pub use block::{blit_into, join_h, join_v};
pub use draw::*;
pub use layout::{Constraint, Flex, split_h, split_h_flex, split_v, split_v_flex};
pub use state::ListState;
pub use text::truncate;
#[cfg(feature = "egc")]
pub use widget::Paragraph;
pub use widget::{Measure, Panel, StatefulWidget, Table, Widget};
