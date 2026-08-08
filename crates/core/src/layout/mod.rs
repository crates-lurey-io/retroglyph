//! Text layout: measurement, word wrapping, and bounded alignment.
//!
//! [`HAlign`](crate::layout::HAlign) and [`VAlign`](crate::layout::VAlign) are plain data (no allocation, no text handling) and are always
//! available regardless of the `egc` feature; [`Surface::print_aligned`](crate::surface::Surface::print_aligned)
//! uses them directly. [`crate::text::Span`] and [`crate::text::Line`] provide styled text
//! primitives, and [`TextLayout`](crate::layout::TextLayout) is a builder that word-wraps a [`crate::text::Line`] to a
//! bounded [`Rect`](crate::grid::Rect), then positions it with independent horizontal
//! ([`HAlign`](crate::layout::HAlign)) and vertical ([`VAlign`](crate::layout::VAlign)) alignment. Measure the result before rendering with
//! [`TextLayout::measure`](crate::layout::TextLayout::measure). [`wrap`] exposes that same word-wrap pass standalone, for callers
//! that need the broken-apart [`crate::text::Line`]s rather than a rendered surface.
//!
//! [`TextLayout`](crate::layout::TextLayout), [`wrap`], and [`wrap_str`] are only available when the `egc` feature is
//! enabled (requires `alloc`): all three call into `unicode-segmentation`/`unicode-width`
//! directly, with no non-`egc` fallback path.

mod align;
#[cfg(feature = "egc")]
mod text_layout;
#[cfg(feature = "egc")]
mod word_wrap;

pub use align::{HAlign, VAlign};
#[cfg(feature = "egc")]
pub use text_layout::TextLayout;
#[cfg(feature = "egc")]
pub use word_wrap::{wrap, wrap_str};
