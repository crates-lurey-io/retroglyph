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
//! use [`Meter`] for their ratio-to-color ramp. [`Paragraph`], [`List`],
//! [`Table`], [`Log`], and [`Panel`] additionally implement [`Measure`], so
//! a caller can report a height before rendering instead of guessing a
//! fixed height or a full-remaining-space fill: [`Paragraph`] reports its
//! wrapped line count (with the `egc` feature enabled, via
//! `retroglyph_core::layout::TextLayout`'s grapheme-aware word-wrap,
//! otherwise via its own `char`-boundary-safe fallback); [`List`]/[`Table`]/
//! [`Log`] report their item/row/message count directly, since none of them
//! wrap; [`Panel`] reports its border-plus-padding chrome height, since it
//! owns no content of its own to measure.
use retroglyph_core::Frame;

use crate::Response;
use crate::Sense;
use crate::Surface;

mod bar;
mod border_type;
mod box_border;
mod button;
mod gauge;
mod highlight_spacing;
mod list;
mod list_direction;
mod log;
mod meter;
mod modal;
mod panel;
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
mod text_input;
mod window;

pub use border_type::BorderType;
pub use box_border::BoxBorder;
pub use button::Button;
pub use gauge::Gauge;
pub use highlight_spacing::HighlightSpacing;
pub use list::List;
pub use list_direction::ListDirection;
pub use log::Log;
pub use meter::Meter;
pub use modal::Modal;
pub use panel::Panel;
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
pub use text_input::TextInput;

/// A type that draws itself into a [`Surface`], without retaining any
/// state: the minimal shape shared by every widget-like consumer.
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
    ///
    /// Coordinates on every `Surface` drawing method are local to `surface` itself, where
    /// `(0, 0)` is `surface.area()`'s own top-left corner, not the underlying grid's. Placement
    /// should therefore be built from [`Surface::width`]/[`Surface::height`] (or
    /// [`Surface::local_area`] for a rect), never from `surface.area()`'s own
    /// [`left`](retroglyph_core::Rect::left)/[`top`](retroglyph_core::Rect::top): those are
    /// absolute grid coordinates, and passing them straight to a drawing call only lands
    /// correctly for a surface whose area happens to start at the grid origin.
    fn render(&self, surface: &mut Surface<'_>);
}

/// Like [`Widget`], but for widgets that read (and may update) externally
/// owned state (a selection index, a scroll offset) that outlives a
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
    ///
    /// See [`Widget::render`]'s doc for why placement should come from
    /// [`Surface::width`]/[`Surface::height`]/[`Surface::local_area`], not `surface.area()`'s own
    /// [`left`](retroglyph_core::Rect::left)/[`top`](retroglyph_core::Rect::top).
    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State);
}

/// A widget that can report the height it needs for a given width, before
/// ever being rendered.
///
/// Lets a caller size a pane to fit content (e.g. a wrapped `Paragraph`)
/// instead of guessing a fixed height up front.
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
/// enforces that call happening before `render` rather than after it: the two orders differ by
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
    /// rendering, e.g. [`crate::ScrollState`].
    type State;

    /// Advances `state` by `frame.delta`, then draws this widget into `surface.area()`, both in
    /// the same call, so there's exactly one place, not two independently ordered ones, where
    /// time-based state moves forward.
    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State, frame: &Frame);
}

/// A widget that renders itself styled by an already-resolved [`Response`], with the [`Sense`]
/// it needs fixed by the widget rather than chosen at the call site.
///
/// A composite widget like [`Button`], [`Scrollbar`], [`List`], or [`Tabs`] needs to know what
/// happened to it this frame (hovered, pressed, clicked, dragged) to pick its style and resolve
/// its own hit-testing, but never calls
/// [`Interaction::interact`](crate::Interaction::interact) itself: doing so would let it register
/// the wrong rect, or let a call site register it with a [`Sense`] its presentation doesn't
/// match (a click handler drawn without ever showing a hover state, say). [`sense`](Self::sense)
/// fixes what the widget needs so a call site can't get that pairing wrong, and
/// [`render`](Self::render) takes the resulting [`Response`] as a plain argument rather than
/// calling `interact` itself: the widget never receives an
/// [`Interaction`](crate::Interaction), and has no `Id` type parameter, so it can't call
/// `interact` with the wrong rect because it has nothing to call `interact` on.
///
/// `type State` covers widgets with no state ([`Button`], [`Tabs`]: `()`), a scroll position
/// ([`Scrollbar`]: [`ScrollState`](crate::ScrollState)), or a selection/scroll index ([`List`]:
/// [`ListState`](crate::ListState)), the same [`Widget`]/[`StatefulWidget`] split applied to
/// interactive widgets rather than a separate `InteractiveStatefulWidget` trait.
///
/// Has no generic method, so `dyn InteractiveWidget<State = ()>` is object-safe,
/// e.g. a `Vec<Box<dyn InteractiveWidget<State = ()>>>` of heterogeneous stateless widgets.
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Grid, Rect, Style};
/// use retroglyph_widgets::{InteractiveWidget, Response, Sense, Surface};
///
/// struct Marker(char);
///
/// impl InteractiveWidget for Marker {
///     type State = ();
///
///     fn sense(&self) -> Sense {
///         Sense::click()
///     }
///
///     fn render(&self, surface: &mut Surface<'_>, _state: &mut Self::State, response: Response) {
///         let style = if response.hovered() { Style::new().bg(retroglyph_core::Color::RED) } else { Style::new() };
///         surface.put((0, 0), self.0, style);
///     }
/// }
/// ```
pub trait InteractiveWidget {
    /// State that outlives one render call, e.g. a selection index or scroll offset. `()` for
    /// widgets that have none.
    type State;

    /// What this widget needs from the pointer and keyboard, fixed by the widget: a call site
    /// cannot register it with a [`Sense`] its presentation doesn't match.
    fn sense(&self) -> Sense;

    /// Draw into `surface`, styled by `response`, which the caller already resolved (via
    /// [`Interaction::interact`](crate::Interaction::interact)) for `surface.area()` and
    /// [`sense`](Self::sense).
    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State, response: Response);
}

#[cfg(test)]
mod interactive_widget_tests {
    use retroglyph_core::{Grid, Rect};

    use super::{InteractiveWidget, Response, Sense, Surface};

    struct Dot;

    impl InteractiveWidget for Dot {
        type State = ();

        fn sense(&self) -> Sense {
            Sense::click()
        }

        fn render(&self, surface: &mut Surface<'_>, _state: &mut Self::State, response: Response) {
            let glyph = if response.hovered() { '*' } else { '.' };
            surface.put((0, 0), glyph, retroglyph_core::Style::new());
        }
    }

    /// `InteractiveWidget<State = ()>` must be object-safe: no generic method, no `Id` parameter.
    #[test]
    fn is_object_safe() {
        let widgets: Vec<Box<dyn InteractiveWidget<State = ()>>> =
            vec![Box::new(Dot), Box::new(Dot)];

        let area = Rect::new(0, 0, 1, 1);
        let mut grid = Grid::new(1, 1);
        for widget in &widgets {
            widget.render(
                &mut Surface::new(&mut grid, area, 0),
                &mut (),
                Response::default(),
            );
        }
        assert_eq!(grid[retroglyph_core::Pos::new(0, 0)].glyph(), '.');
    }
}
