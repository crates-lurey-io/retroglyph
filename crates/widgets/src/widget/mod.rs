//! `Widget`/`StatefulWidget` structs: one file per widget, each a builder
//! that owns its own drawing logic.
//!
//! `new()` takes only the arguments a widget cannot mean anything without
//! (the content: a value, a label, a slice of samples/rows). Every other
//! knob (styles, offsets, titles) has a default and is set through a
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
use retroglyph_core::Frame;

use crate::Surface;

mod bar;
mod box_border;
mod button;
mod gauge;
mod list;
mod log;
mod meter;
mod modal;
mod panel;
#[cfg(feature = "egc")]
mod paragraph;
mod perf_overlay;
mod print_line;
mod progress_bar;
mod scrollbar;
mod sparkline;
mod stat_bar;
mod table;
mod tabs;
mod text;
mod window;

pub use box_border::BoxBorder;
pub use button::Button;
pub use gauge::Gauge;
pub use list::List;
pub use log::Log;
pub use meter::Meter;
pub use modal::Modal;
pub use panel::Panel;
#[cfg(feature = "egc")]
pub use paragraph::Paragraph;
pub use perf_overlay::{AnimatedPerfOverlay, PerfOverlay};
pub use print_line::PrintLine;
pub use progress_bar::ProgressBar;
pub use scrollbar::Scrollbar;
pub use sparkline::Sparkline;
pub use stat_bar::StatBar;
pub use table::Table;
pub use tabs::Tabs;
pub use text::Text;

/// A type that draws itself into a [`Surface`], without retaining any
/// state — the minimal shape shared by every widget-like consumer.
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Grid, Rect, Style};
/// use retroglyph_widgets::{Surface, Widget};
///
/// struct Marker(char);
///
/// impl Widget for Marker {
///     fn render(&self, surface: &mut Surface<'_>) {
///         surface.put((0, 0), self.0, Style::new());
///     }
/// }
///
/// let area = Rect::new(0, 0, 4, 1);
/// let mut grid = Grid::new(4, 1);
/// Marker('*').render(&mut Surface::new(&mut grid, area, 0));
/// ```
pub trait Widget {
    /// Draw this widget into `surface`, filling `surface.area()`.
    fn render(&self, surface: &mut Surface<'_>);
}

/// Like [`Widget`], but for widgets that read (and may update) externally
/// owned state — a selection index, a scroll offset — that outlives a
/// single render call. See [`crate::ListState`].
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Grid, Rect, Style};
/// use retroglyph_widgets::{Surface, StatefulWidget};
///
/// struct Counter;
///
/// impl StatefulWidget for Counter {
///     type State = u32;
///
///     fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State) {
///         *state += 1;
///         surface.put((0, 0), 'x', Style::new());
///     }
/// }
///
/// let area = Rect::new(0, 0, 4, 1);
/// let mut grid = Grid::new(4, 1);
/// let mut renders = 0;
/// Counter.render(&mut Surface::new(&mut grid, area, 0), &mut renders);
/// assert_eq!(renders, 1);
/// ```
pub trait StatefulWidget {
    /// The externally owned state this widget reads and/or updates while
    /// rendering.
    type State;

    /// Draw this widget into `surface`, filling `surface.area()`, using
    /// and/or updating `state`.
    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State);
}

/// A widget that can report the height it needs for a given width, before
/// ever being rendered.
///
/// Lets a caller size a pane to fit content (e.g. a wrapped `Paragraph`,
/// behind the `egc` feature) instead of guessing a fixed height up front.
/// Sizing is pure content math, not drawing.
///
/// # Examples
///
/// ```
/// use retroglyph_widgets::Measure;
///
/// struct FixedHeight(u16);
///
/// impl Measure for FixedHeight {
///     fn height_for(&self, _width: u16) -> u16 {
///         self.0
///     }
/// }
///
/// assert_eq!(FixedHeight(3).height_for(80), 3);
/// ```
pub trait Measure {
    /// The number of rows this widget would need to render at `width`
    /// columns.
    fn height_for(&self, width: u16) -> u16;
}

/// Like [`StatefulWidget`], but for widgets whose state evolves with wall-clock time.
///
/// Covers state like [`crate::ScrollState`]'s momentum/rubber-band physics or a
/// [`Tween`](retroglyph_core::Tween)-driven transition, which advance on their own rather than
/// only in response to input.
///
/// [`StatefulWidget`] has no way to reach the [`Frame`] an [`App`](retroglyph_core::App) already
/// receives every frame, so a widget with time-based state has nowhere to advance it: not in
/// `render` (no `Frame` parameter), and not in a second, app-defined call, because nothing
/// enforces that call happening before `render` rather than after it -- the two orders differ by
/// one frame of animation, silently. `AnimatedWidget` closes that gap with a single call that both
/// advances and draws, so the ordering question doesn't arise. See [`Scrollbar`]'s impl for a
/// worked example: it ticks [`crate::ScrollState`]'s physics forward by `frame.delta`, then draws
/// the thumb at the resulting offset, in one call.
///
/// A sibling of [`StatefulWidget`], not a replacement: a widget with no time-based state (a
/// selection index that only moves on a keypress, say) has no use for `frame` and should keep
/// implementing [`StatefulWidget`] instead. Nothing stops a widget from implementing both, the way
/// [`Scrollbar`] implements [`Widget`] (a plain, offset-at-a-fixed-value track+thumb) alongside
/// this trait (an animated one driven by [`crate::ScrollState`]).
///
/// # Examples
///
/// ```
/// use core::time::Duration;
/// use retroglyph_core::{Frame, Grid, Rect};
/// use retroglyph_widgets::{AnimatedWidget, Surface};
///
/// struct Blinker;
///
/// impl AnimatedWidget for Blinker {
///     type State = Duration;
///
///     fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State, frame: &Frame) {
///         *state += frame.delta;
///         let on = state.as_millis() / 500 % 2 == 0;
///         surface.put((0, 0), if on { '*' } else { ' ' }, retroglyph_core::Style::new());
///     }
/// }
///
/// let area = Rect::new(0, 0, 4, 1);
/// let mut grid = Grid::new(4, 1);
/// let mut state = Duration::ZERO;
/// let frame = Frame { delta: Duration::from_millis(100), frame: 0 };
/// Blinker.render(&mut Surface::new(&mut grid, area, 0), &mut state, &frame);
/// assert_eq!(state, Duration::from_millis(100));
/// ```
pub trait AnimatedWidget {
    /// The externally owned, time-evolving state this widget reads and/or updates while
    /// rendering -- e.g. [`crate::ScrollState`].
    type State;

    /// Advances `state` by `frame.delta`, then draws this widget into `surface.area()`, both in
    /// the same call, so there's exactly one place, not two independently ordered ones, where
    /// time-based state moves forward.
    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State, frame: &Frame);
}
