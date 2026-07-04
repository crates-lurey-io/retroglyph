//! Immediate-mode drawing helpers over a [`Rect`](retroglyph_core::Rect).
//!
//! Box borders, filled panels, gauges, lists, tabs, sparklines, and a small
//! constraint-based [`Rect`](retroglyph_core::Rect) splitter ([`layout`]).
//! Each widget is a free function that draws directly to a [`Terminal`](retroglyph_core::Terminal)
//! and retains no state, so there is no `Widget` trait to implement or store.
//! This crate is optional: games that draw manually depend only on `retroglyph-core`.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::items_after_statements
)]

/// Box borders, filled panels, gauges, lists, tabs, and sparklines.
pub mod draw;
/// A small constraint-based [`Rect`](retroglyph_core::Rect) splitter.
pub mod layout;

pub use draw::*;
pub use layout::{Constraint, split_h, split_v};
