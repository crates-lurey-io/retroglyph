//! Text layout: measurement, word wrapping, and bounded alignment.
//!
//! [`HAlign`] and [`VAlign`] are plain data (no allocation, no text handling) and are always
//! available regardless of the `egc` feature; [`Surface::print_aligned`](crate::surface::Surface::print_aligned)
//! uses them directly. [`crate::text::Span`] and [`crate::text::Line`] provide styled text
//! primitives, and [`TextLayout`] is a builder that word-wraps a [`crate::text::Line`] to a
//! bounded [`Rect`](crate::grid::Rect), then positions it with independent horizontal
//! ([`HAlign`]) and vertical ([`VAlign`]) alignment. Measure the result before rendering with
//! [`TextLayout::measure`]. [`wrap`] exposes that same word-wrap pass standalone, for callers
//! that need the broken-apart [`crate::text::Line`]s rather than a rendered surface.
//!
//! [`TextLayout`] and [`wrap`] are only available when the `egc` feature is enabled (requires
//! `alloc`): both call into `unicode-segmentation`/`unicode-width` directly, with no non-`egc`
//! fallback path.

mod align;
#[cfg(feature = "egc")]
mod text_layout;
#[cfg(feature = "egc")]
mod word_wrap;

pub use align::{HAlign, VAlign};
#[cfg(feature = "egc")]
pub use text_layout::{TextLayout, TextMetrics};
#[cfg(feature = "egc")]
pub use word_wrap::wrap;
