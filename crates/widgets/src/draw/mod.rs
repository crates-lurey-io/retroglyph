//! [`fill_rect`] and [`thumb_geometry`]/[`offset_for_pos`]: the handful of
//! things genuinely useful as standalone functions rather than
//! [`Widget`](crate::widget::Widget)s.
//!
//! `fill_rect` is a one-shot fill with no configuration worth building.
//! `thumb_geometry`/`offset_for_pos` are pure position/size arithmetic with
//! no [`Terminal`](retroglyph_core::Terminal) involved, reused for
//! hit-testing independently of drawing a
//! [`Scrollbar`](crate::widget::Scrollbar).
//!
//! Everything else that used to live here -- `panel`, `modal`,
//! `progress_bar`, `print_line`, `log`, `gauge`, `stat_bar`, `sparkline`,
//! `table`, `meter_ramp`, `scrollbar` -- is now a [`Widget`](crate::widget::Widget)
//! (or [`StatefulWidget`](crate::widget::StatefulWidget)) struct under
//! [`widget`](crate::widget), one file each, so there's a single way to draw
//! each of them instead of a free function and a builder both doing the same
//! thing.

mod primitives;
mod scrollbar;

pub use primitives::fill_rect;
pub use scrollbar::{offset_for_pos, thumb_geometry};

// Box-drawing codepoints are crate-internal only (reused by `style.rs` and
// `widget::BoxBorder`), not part of the public API.
pub(crate) use primitives::{BL, BR, H, TL, TR, V};
