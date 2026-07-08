//! Immediate-mode drawing helpers over a [`Rect`](retroglyph_core::Rect).
//!
//! Box borders, filled panels, gauges, lists, tabs, sparklines, and a small
//! constraint-based [`Rect`](retroglyph_core::Rect) splitter with
//! ratatui-style `Fixed`/`Percent`/`Fill`/`Min`/`Max` constraints and `Flex`
//! alignment ([`layout`]).
//!
//! Every widget is primarily a free function that draws directly to a
//! [`Terminal`](retroglyph_core::Terminal) and retains no state. Four
//! optional layers build on top of that free-function core, each usable
//! independently:
//!
//! - [`Widget`]/[`StatefulWidget`] ([`widget`]) for callers who want to box
//!   or store heterogeneous widgets, backed by [`ListState`] for selection
//!   and scroll position.
//! - [`Interaction`] ([`interact`]) for hover/click/drag/focus tracking
//!   without a retained widget tree -- the sibling of [`ListState`] for
//!   widgets that don't have a natural selection index of their own.
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

pub mod block;
pub mod draw;
pub mod interact;
pub mod layout;
pub mod state;
pub mod style;
pub mod text;
pub mod widget;

pub use block::{blit_into, join_h, join_v};
pub use draw::{
    draw_box, fill_rect, gauge, log, meter_ramp, modal, offset_for_pos, panel, print_line,
    progress_bar, scrollbar, sparkline, stat_bar, table, thumb_geometry,
};
pub use interact::{
    DEFAULT_DRAG_THRESHOLD, FocusRing, HitTester, Interaction, Pointer, Response, Sense,
};
pub use layout::{Constraint, Flex, centered_rect, split_h, split_h_flex, split_v, split_v_flex};
pub use state::ListState;
pub use style::{BoxStyle, Sides};
pub use text::truncate;
#[cfg(feature = "egc")]
pub use widget::Paragraph;
pub use widget::{Measure, Panel, StatefulWidget, Table, Widget};
