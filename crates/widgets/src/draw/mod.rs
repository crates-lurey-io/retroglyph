//! Drawing helpers: box borders, filled rects, panels, gauges, sparklines,
//! and tables.
//!
//! Split into two source-only tiers, both private modules re-exported flat
//! below (see [`crate`] for the public surface):
//!
//! - Primitives: [`draw_box`], [`fill_rect`], [`panel`], [`modal`],
//!   [`progress_bar`], [`print_line`], and [`log`]. These take styles as
//!   parameters and bake in no color opinions of their own -- reusable
//!   building blocks for any theme.
//! - Composite widgets: [`gauge`], [`stat_bar`], [`sparkline`], [`table`],
//!   and [`meter_ramp`]. These are built from the primitives above but
//!   hardcode a specific dark-theme palette, because they exist for the
//!   system-monitor dashboard demo rather than as theme-agnostic
//!   primitives.
//! - [`scrollbar`] (plus its geometry helpers [`thumb_geometry`] and
//!   [`offset_for_pos`]): a vertical track+thumb indicator, theme-agnostic
//!   like the primitives tier. Deliberately independent of
//!   [`crate::interact`] -- see its own doc comment for how to make one
//!   draggable using [`Interaction`](crate::Interaction) instead.
//!
//! Both tiers are re-exported flat from this module (and from the crate
//! root), so `retroglyph_widgets::gauge(...)` and
//! `retroglyph_widgets::draw::gauge(...)` both work; the split is purely
//! about where the source lives.

mod composite;
pub(crate) mod primitives;
mod scrollbar;

pub use composite::{gauge, meter_ramp, sparkline, stat_bar, table};
pub use primitives::{draw_box, fill_rect, log, modal, panel, print_line, progress_bar};
pub use scrollbar::{offset_for_pos, scrollbar, thumb_geometry};

// Box-drawing codepoints are crate-internal only (reused by `style.rs`), not
// part of the public API.
pub(crate) use primitives::{BL, BR, H, TL, TR, V};
