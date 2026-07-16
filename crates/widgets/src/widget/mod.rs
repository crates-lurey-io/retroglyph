//! `Widget`/`StatefulWidget` structs: one file per widget, each a builder
//! that owns its own drawing logic.
//!
//! `new()` takes only the arguments a widget cannot mean anything without
//! (the content: a value, a label, a slice of samples/rows). Every other
//! knob -- styles, offsets, titles -- has a default and is set through a
//! chainable `#[must_use] fn field(mut self, ...) -> Self` method, the same
//! shape as [`Panel::title`] or [`Log::offset`]. See `crates/widgets/AGENTS.md`
//! for the rule this is enforcing and why.
//!
//! A few widgets share logic: [`Gauge`] and [`StatBar`] both delegate to a
//! crate-private `bar` module, and [`Sparkline`]/[`Gauge`]/[`StatBar`] all
//! use [`Meter`] for their ratio-to-color ramp. [`Paragraph`] (behind the
//! `egc` feature) additionally implements [`Measure`], since it needs
//! `retroglyph_core::layout::TextLayout`'s grapheme-aware word-wrap to
//! report a height before rendering.
use retroglyph_core::{Backend, Rect, Terminal};

mod bar;
mod box_border;
mod gauge;
mod log;
mod meter;
mod modal;
mod panel;
#[cfg(feature = "egc")]
mod paragraph;
mod print_line;
mod progress_bar;
mod scrollbar;
mod sparkline;
mod stat_bar;
mod table;
mod text;
mod window;

pub use box_border::BoxBorder;
pub use gauge::Gauge;
pub use log::Log;
pub use meter::Meter;
pub use modal::Modal;
pub use panel::Panel;
#[cfg(feature = "egc")]
pub use paragraph::Paragraph;
pub use print_line::PrintLine;
pub use progress_bar::ProgressBar;
pub use scrollbar::Scrollbar;
pub use sparkline::Sparkline;
pub use stat_bar::StatBar;
pub use table::Table;
pub use text::Text;

/// A type that draws itself into a terminal area, without retaining any
/// state — the minimal shape shared by every widget-like consumer.
pub trait Widget<B: Backend> {
    /// Draw this widget into `area`.
    fn render(self, area: Rect, term: &mut Terminal<B>);
}

/// Like [`Widget`], but for widgets that read (and may update) externally
/// owned state — a selection index, a scroll offset — that outlives a
/// single render call. See [`crate::ListState`].
pub trait StatefulWidget<B: Backend> {
    /// The externally owned state this widget reads and/or updates while
    /// rendering.
    type State;

    /// Draw this widget into `area`, using and/or updating `state`.
    fn render(self, area: Rect, term: &mut Terminal<B>, state: &mut Self::State);
}

/// A widget that can report the height it needs for a given width, before
/// ever being rendered.
///
/// Lets a caller size a pane to fit content (e.g. a wrapped `Paragraph`,
/// behind the `egc` feature) instead of guessing a fixed height up front.
/// Independent of any [`Backend`]: sizing is pure content math, not drawing.
pub trait Measure {
    /// The number of rows this widget would need to render at `width`
    /// columns.
    fn height_for(&self, width: u16) -> u16;
}
