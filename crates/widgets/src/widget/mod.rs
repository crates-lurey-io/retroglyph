//! Optional `Widget`/`StatefulWidget` traits over the free-function drawing
//! helpers in [`draw`](crate::draw).
//!
//! Nothing in this crate requires implementing these traits: every existing
//! helper (`panel`, `gauge`, `table`, ...) works exactly as before without
//! them. They exist for callers who want to box/store heterogeneous widgets
//! (e.g. a `Vec<Box<dyn Widget<B>>>` of panes to render each frame) instead
//! of calling free functions directly. Most concrete widgets below (one file
//! per widget) are thin adapters that call straight through to their
//! corresponding [`crate::draw`] function and add no drawing logic of their
//! own -- [`Panel`] and [`Table`] are both like this. `Paragraph` (behind the
//! `egc` feature) is the exception: there is no `draw::paragraph` free
//! function, because it needs `retroglyph_core::layout::TextLayout`'s
//! grapheme-aware word-wrap in order to also implement [`Measure`], which
//! free functions have no way to model.
use retroglyph_core::{Backend, Rect, Terminal};

mod panel;
#[cfg(feature = "egc")]
mod paragraph;
mod table;

pub use panel::Panel;
#[cfg(feature = "egc")]
pub use paragraph::Paragraph;
pub use table::Table;

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
